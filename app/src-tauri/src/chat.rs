//! The one continuous chat: persists every turn, streams runtime events to the UI,
//! and keeps the runtime session resumable across restarts.
use crate::runtime::{AgentEvent, AgentRuntime, McpEndpoint, SessionConfig, SessionHandle, StaleResume, Tier, UserMessage};
use crate::store::{Kind, MsgStatus, NewMessage, Role, Session, Store};
use futures::StreamExt;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

pub const SYSTEM_PROMPT: &str = "You are bleep, a small droid assistant that lives on the user's Mac. \
Be brief and warm. Use the bleep_now tool for the current date, time and time zone. Never guess them. \
You have no other tools yet. Do not use em dashes.";
pub const FRESH_SESSION_NOTE: &str = "Started a fresh Claude session. Your history is still here.";

#[derive(Serialize, Clone, Debug)]
pub struct ChatEmit {
    pub message_id: i64,
    pub event: AgentEvent,
}

#[derive(Debug)]
pub enum ChatError {
    Busy,
    Other(String),
}

impl std::fmt::Display for ChatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChatError::Busy => write!(f, "busy"),
            ChatError::Other(s) => write!(f, "{s}"),
        }
    }
}

impl From<anyhow::Error> for ChatError {
    fn from(e: anyhow::Error) -> Self {
        ChatError::Other(e.to_string())
    }
}

pub type Emit = Arc<dyn Fn(ChatEmit) + Send + Sync>;

pub struct ChatService {
    store: Arc<Store>,
    runtime: Arc<dyn AgentRuntime>,
    mcp: McpEndpoint,
    emit: Emit,
    session: Mutex<Option<(Session, SessionHandle)>>,
    busy: Arc<AtomicBool>,
    turn: Mutex<Option<JoinHandle<()>>>,
}

/// Clears the busy flag when the turn task ends, even if it panics.
struct BusyGuard(Arc<AtomicBool>);
impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl ChatService {
    pub fn new(store: Arc<Store>, runtime: Arc<dyn AgentRuntime>, mcp: McpEndpoint, emit: Emit) -> Self {
        Self { store, runtime, mcp, emit, session: Mutex::new(None), busy: Default::default(), turn: Mutex::new(None) }
    }

    pub fn runtime(&self) -> &Arc<dyn AgentRuntime> {
        &self.runtime
    }

    fn cfg(&self, resume_id: Option<String>) -> SessionConfig {
        SessionConfig { system_prompt: SYSTEM_PROMPT.into(), mcp: self.mcp.clone(), tier: Tier::Main, resume_id }
    }

    /// Resume the current session, or start one. A stale resume id starts fresh, once.
    pub async fn ensure_session(&self) -> anyhow::Result<()> {
        let mut slot = self.session.lock().await;
        if slot.is_some() {
            return Ok(());
        }
        let rid = self.runtime.id();
        let session = match self.store.current_session(rid)? {
            Some(s) => s,
            None => self.store.create_session(rid)?,
        };
        let handle = match self.runtime.start_session(self.cfg(session.runtime_session_id.clone())).await {
            Ok(h) => (session, h),
            Err(e) if session.runtime_session_id.is_some() && e.downcast_ref::<StaleResume>().is_some() => {
                eprintln!("bleep: resume failed, starting fresh: {e}");
                self.store.end_session(session.id)?;
                let fresh = self.store.create_session(rid)?;
                let h = self.runtime.start_session(self.cfg(None)).await?;
                self.store.insert_message(NewMessage { role: Role::System, kind: Kind::System, content: FRESH_SESSION_NOTE.into(), status: MsgStatus::Done, session_id: Some(fresh.id) })?;
                (fresh, h)
            }
            Err(e) => return Err(e),
        };
        *slot = Some(handle);
        Ok(())
    }

    /// Saves the user row and a streaming assistant row, returns the assistant id at once,
    /// and streams the turn in the background.
    pub async fn send(&self, text: String) -> Result<i64, ChatError> {
        if self.busy.swap(true, Ordering::SeqCst) {
            return Err(ChatError::Busy);
        }
        let guard = BusyGuard(self.busy.clone());
        self.ensure_session().await?;
        let (session_id, handle) = {
            let slot = self.session.lock().await;
            let (s, h) = slot.as_ref().expect("session ensured");
            (s.id, h.clone())
        };
        self.store.insert_message(NewMessage { role: Role::User, kind: Kind::Chat, content: text.clone(), status: MsgStatus::Done, session_id: Some(session_id) })?;
        let aid = self.store.insert_message(NewMessage { role: Role::Assistant, kind: Kind::Chat, content: String::new(), status: MsgStatus::Streaming, session_id: Some(session_id) })?;
        let stream = match self.runtime.send(&handle, UserMessage { text }).await {
            Ok(s) => s,
            Err(e) => {
                let msg = format!("bleep could not reach Claude Code: {e}");
                self.store.finish_message(aid, &msg, &Value::Array(vec![]), None, MsgStatus::Error)?;
                return Err(ChatError::Other(msg));
            }
        };
        let (store, emit) = (self.store.clone(), self.emit.clone());
        let task = tokio::spawn(async move {
            let _guard = guard;
            run_turn(store, emit, session_id, aid, stream).await;
        });
        *self.turn.lock().await = Some(task);
        Ok(aid)
    }

    /// Waits for the running turn, if any (tests and shutdown).
    pub async fn wait_idle(&self) {
        if let Some(t) = self.turn.lock().await.take() {
            let _ = t.await;
        }
    }
}

async fn run_turn(store: Arc<Store>, emit: Emit, session_id: i64, aid: i64, mut stream: crate::runtime::EventStream) {
    let mut text = String::new();
    let mut steps: Vec<Value> = vec![];
    let mut tokens = None;
    let mut errored = false;
    // Stays Error if the stream ends without Done.
    let mut status = MsgStatus::Error;
    while let Some(ev) = stream.next().await {
        emit(ChatEmit { message_id: aid, event: ev.clone() });
        match ev {
            AgentEvent::TextDelta { text: t } => text.push_str(&t),
            AgentEvent::ToolStart { id, name, args } => steps.push(json!({"id": id, "name": name, "args": args, "ok": null, "preview": null})),
            AgentEvent::ToolResult { id, ok, preview } => {
                if let Some(s) = steps.iter_mut().find(|s| s["id"] == id) {
                    s["ok"] = json!(ok);
                    s["preview"] = json!(preview);
                }
            }
            AgentEvent::Usage { input, output } => tokens = Some((input + output) as i64),
            AgentEvent::RateLimited { .. } => {}
            AgentEvent::Error { message, .. } => {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&message);
                errored = true;
            }
            AgentEvent::Done { runtime_session_id } => {
                status = if errored { MsgStatus::Error } else { MsgStatus::Done };
                if let Some(id) = runtime_session_id {
                    let _ = store.set_runtime_session_id(session_id, &id);
                }
                break;
            }
        }
    }
    let _ = store.finish_message(aid, &text, &Value::Array(steps), tokens, status);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::fake::FakeRuntime;
    use crate::runtime::{AgentEvent, McpEndpoint};
    use crate::store::{MsgStatus, Role};
    use std::sync::atomic::Ordering;
    use std::sync::Mutex as StdMutex;

    struct Rig {
        _dir: tempfile::TempDir,
        store: Arc<Store>,
        rt: Arc<FakeRuntime>,
        emitted: Arc<StdMutex<Vec<ChatEmit>>>,
    }

    fn rig(script: Vec<Vec<AgentEvent>>) -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&dir.path().join("b.db")).unwrap());
        Rig { _dir: dir, store, rt: Arc::new(FakeRuntime::new(script)), emitted: Default::default() }
    }

    fn service(r: &Rig) -> ChatService {
        let e = r.emitted.clone();
        ChatService::new(
            r.store.clone(),
            r.rt.clone(),
            McpEndpoint { url: "http://127.0.0.1:1/mcp".into(), token: "t".into() },
            Arc::new(move |m| e.lock().unwrap().push(m)),
        )
    }

    fn done(sid: &str) -> AgentEvent {
        AgentEvent::Done { runtime_session_id: Some(sid.into()) }
    }

    #[tokio::test]
    async fn send_persists_both_rows() {
        let r = rig(vec![vec![AgentEvent::TextDelta { text: "hi".into() }, AgentEvent::Usage { input: 100, output: 2 }, done("s1")]]);
        let chat = service(&r);
        chat.ensure_session().await.unwrap();
        let aid = chat.send("hello".into()).await.unwrap();
        chat.wait_idle().await;
        let h = r.store.page_messages(None, 50).unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!((h[0].role, h[0].content.as_str(), h[0].status), (Role::User, "hello", MsgStatus::Done));
        assert_eq!((h[1].id, h[1].role, h[1].content.as_str(), h[1].status), (aid, Role::Assistant, "hi", MsgStatus::Done));
        assert_eq!(h[1].tokens, Some(102));
        assert_eq!(r.store.current_session("claude").unwrap().unwrap().runtime_session_id.as_deref(), Some("s1"));
        assert_eq!(r.emitted.lock().unwrap().len(), 3);
        assert!(r.emitted.lock().unwrap().iter().all(|e| e.message_id == aid));
    }

    #[tokio::test]
    async fn second_service_resumes() {
        let r = rig(vec![vec![done("s1")]]);
        let chat = service(&r);
        chat.ensure_session().await.unwrap();
        chat.send("a".into()).await.unwrap();
        chat.wait_idle().await;
        let chat2 = service(&r);
        chat2.ensure_session().await.unwrap();
        let started = r.rt.started.lock().unwrap();
        assert_eq!(started.last().unwrap().resume_id.as_deref(), Some("s1"));
        assert!(started.last().unwrap().system_prompt.contains("bleep_now"));
    }

    #[tokio::test]
    async fn stale_resume_starts_fresh() {
        let r = rig(vec![]);
        let old = r.store.create_session("claude").unwrap();
        r.store.set_runtime_session_id(old.id, "gone").unwrap();
        r.rt.fail_start_with_resume.store(true, Ordering::SeqCst);
        let chat = service(&r);
        chat.ensure_session().await.unwrap();
        let cur = r.store.current_session("claude").unwrap().unwrap();
        assert_ne!(cur.id, old.id, "old session ended, new one made");
        assert_eq!(r.rt.started.lock().unwrap().last().unwrap().resume_id, None);
        let h = r.store.page_messages(None, 50).unwrap();
        assert_eq!(h.last().unwrap().content, FRESH_SESSION_NOTE);
        assert_eq!(h.last().unwrap().role, Role::System);
    }

    #[tokio::test]
    async fn other_start_error_keeps_the_session() {
        let r = rig(vec![]);
        let old = r.store.create_session("claude").unwrap();
        r.store.set_runtime_session_id(old.id, "keep-me").unwrap();
        *r.rt.fail_start_with.lock().unwrap() = Some("Not logged in".into());
        let chat = service(&r);
        assert!(chat.ensure_session().await.is_err());
        let cur = r.store.current_session("claude").unwrap().unwrap();
        assert_eq!((cur.id, cur.runtime_session_id.as_deref()), (old.id, Some("keep-me")));
        assert!(r.store.page_messages(None, 50).unwrap().is_empty(), "no fresh-session note");
    }

    #[tokio::test]
    async fn busy_while_streaming() {
        let r = rig(vec![vec![AgentEvent::TextDelta { text: "par".into() }]]);
        r.rt.hang_after_script.store(true, Ordering::SeqCst);
        let chat = service(&r);
        chat.ensure_session().await.unwrap();
        chat.send("one".into()).await.unwrap();
        assert!(matches!(chat.send("two".into()).await, Err(ChatError::Busy)));
        assert_eq!(r.rt.sent.lock().unwrap().as_slice(), ["one"]);
    }

    #[tokio::test]
    async fn tool_steps_saved() {
        let r = rig(vec![vec![
            AgentEvent::ToolStart { id: "t1".into(), name: "bleep_now".into(), args: serde_json::json!({}) },
            AgentEvent::ToolResult { id: "t1".into(), ok: true, preview: "{}".into() },
            AgentEvent::TextDelta { text: "It is noon.".into() },
            done("s1"),
        ]]);
        let chat = service(&r);
        chat.ensure_session().await.unwrap();
        chat.send("time?".into()).await.unwrap();
        chat.wait_idle().await;
        let m = r.store.page_messages(None, 1).unwrap().remove(0);
        assert_eq!(m.tool_steps[0]["name"], "bleep_now");
        assert_eq!(m.tool_steps[0]["ok"], true);
        assert_eq!(m.content, "It is noon.");
    }

    #[tokio::test]
    async fn error_keeps_partial_text() {
        let r = rig(vec![
            vec![AgentEvent::TextDelta { text: "half".into() }, AgentEvent::Error { message: "lost".into(), retryable: true }, AgentEvent::Done { runtime_session_id: None }],
            vec![done("s2")],
        ]);
        let chat = service(&r);
        chat.ensure_session().await.unwrap();
        chat.send("x".into()).await.unwrap();
        chat.wait_idle().await;
        let m = r.store.page_messages(None, 1).unwrap().remove(0);
        assert_eq!((m.content.as_str(), m.status), ("half\nlost", MsgStatus::Error));
        assert!(chat.send("again".into()).await.is_ok(), "not busy after an error");
    }

    #[tokio::test]
    async fn runtime_send_failure_closes_the_row() {
        let r = rig(vec![]);
        r.rt.fail_send.store(true, Ordering::SeqCst);
        let chat = service(&r);
        assert!(chat.send("x".into()).await.is_err());
        let m = r.store.page_messages(None, 1).unwrap().remove(0);
        assert_eq!((m.role, m.status), (Role::Assistant, MsgStatus::Error));
        assert!(!m.content.is_empty());
        r.rt.fail_send.store(false, Ordering::SeqCst);
        assert!(chat.send("y".into()).await.is_ok(), "not left busy");
    }

    #[tokio::test]
    async fn send_starts_session_if_needed() {
        let r = rig(vec![vec![done("s1")]]);
        let chat = service(&r);
        chat.send("first".into()).await.unwrap();
        chat.wait_idle().await;
        assert_eq!(r.rt.started.lock().unwrap().len(), 1);
    }
}
