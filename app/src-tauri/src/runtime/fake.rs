//! Scripted runtime for service tests.
use super::*;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub struct FakeRuntime {
    script: Mutex<VecDeque<Vec<AgentEvent>>>,
    pub started: Mutex<Vec<SessionConfig>>,
    pub sent: Mutex<Vec<String>>,
    pub fail_start_with_resume: AtomicBool,
    /// After the scripted events, never end the stream (a turn stuck mid-stream).
    pub hang_after_script: AtomicBool,
    pub fail_send: AtomicBool,
    pub fail_start_with: Mutex<Option<String>>,
    pub health: Mutex<Option<RuntimeHealth>>,
    /// Replies for `one_shot`, in order. Empty means `Value::Null`.
    pub one_shot_replies: Mutex<VecDeque<std::result::Result<Value, String>>>,
    /// Prompts `one_shot` received.
    pub one_shots: Mutex<Vec<String>>,
    /// How many images each `one_shot` carried.
    pub one_shot_images: Mutex<Vec<usize>>,
    /// Each `one_shot` waits this long first (tests of work that holds the chat).
    pub one_shot_delay_ms: std::sync::atomic::AtomicU64,
    pub id: &'static str,
}

impl FakeRuntime {
    pub fn new(script: Vec<Vec<AgentEvent>>) -> Self {
        Self {
            script: Mutex::new(script.into()),
            started: Mutex::new(vec![]),
            sent: Mutex::new(vec![]),
            fail_start_with_resume: AtomicBool::new(false),
            hang_after_script: AtomicBool::new(false),
            fail_send: AtomicBool::new(false),
            fail_start_with: Mutex::new(None),
            health: Mutex::new(None),
            one_shot_replies: Mutex::new(VecDeque::new()),
            one_shots: Mutex::new(vec![]),
            one_shot_images: Mutex::new(vec![]),
            one_shot_delay_ms: Default::default(),
            id: "claude",
        }
    }
}

#[async_trait]
impl AgentRuntime for FakeRuntime {
    fn id(&self) -> &'static str {
        self.id
    }

    async fn health(&self) -> RuntimeHealth {
        self.health.lock().unwrap().clone().unwrap_or(RuntimeHealth {
            runtime: "claude".into(),
            installed: true,
            version: Some("2.1.319".into()),
            version_ok: true,
            signed_in: true,
            plan: Some("pro".into()),
            hint: None,
        })
    }

    async fn start_session(&self, cfg: SessionConfig) -> Result<SessionHandle> {
        let resume_id = cfg.resume_id.clone();
        self.started.lock().unwrap().push(cfg);
        if let Some(msg) = self.fail_start_with.lock().unwrap().clone() {
            anyhow::bail!(msg);
        }
        if resume_id.is_some() && self.fail_start_with_resume.load(Ordering::SeqCst) {
            return Err(StaleResume("No conversation found with session ID".into()).into());
        }
        Ok(SessionHandle { resume_id })
    }

    async fn send(&self, _s: &SessionHandle, msg: UserMessage) -> Result<EventStream> {
        if self.fail_send.load(Ordering::SeqCst) {
            anyhow::bail!("spawn failed");
        }
        self.sent.lock().unwrap().push(msg.text);
        let turn = self.script.lock().unwrap().pop_front().unwrap_or_default();
        let s = futures::stream::iter(turn);
        if self.hang_after_script.load(Ordering::SeqCst) {
            Ok(Box::pin(futures::StreamExt::chain(s, futures::stream::pending())))
        } else {
            Ok(Box::pin(s))
        }
    }

    async fn one_shot(&self, req: OneShot) -> Result<Value> {
        let wait = self.one_shot_delay_ms.load(Ordering::SeqCst);
        if wait > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
        }
        self.one_shots.lock().unwrap().push(req.prompt);
        self.one_shot_images.lock().unwrap().push(req.images.len());
        match self.one_shot_replies.lock().unwrap().pop_front() {
            Some(Ok(v)) => Ok(v),
            Some(Err(e)) => anyhow::bail!(e),
            None => Ok(Value::Null),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    fn cfg() -> SessionConfig {
        SessionConfig {
            system_prompt: "p".into(),
            mcp: McpEndpoint { url: "http://127.0.0.1:1/mcp".into(), token: "t".into() },
            tier: Tier::Main,
            resume_id: None,
        }
    }

    #[tokio::test]
    async fn fake_runtime_replays_turns() {
        let script = vec![
            vec![AgentEvent::TextDelta { text: "a".into() }, AgentEvent::Done { runtime_session_id: Some("s1".into()) }],
            vec![AgentEvent::TextDelta { text: "b".into() }, AgentEvent::Done { runtime_session_id: Some("s1".into()) }],
        ];
        let rt = FakeRuntime::new(script.clone());
        let h = rt.start_session(cfg()).await.unwrap();
        let one: Vec<_> = rt.send(&h, UserMessage { text: "x".into() }).await.unwrap().collect().await;
        let two: Vec<_> = rt.send(&h, UserMessage { text: "y".into() }).await.unwrap().collect().await;
        assert_eq!(vec![one, two], script);
        assert_eq!(rt.started.lock().unwrap().len(), 1);
        assert_eq!(rt.sent.lock().unwrap().as_slice(), ["x", "y"]);
    }

    #[tokio::test]
    async fn fake_runtime_can_fail_resume() {
        let rt = FakeRuntime::new(vec![]);
        rt.fail_start_with_resume.store(true, std::sync::atomic::Ordering::SeqCst);
        let mut c = cfg();
        c.resume_id = Some("old".into());
        assert!(rt.start_session(c).await.is_err());
        assert!(rt.start_session(cfg()).await.is_ok());
    }

    #[test]
    fn agent_event_serializes_tagged() {
        let v = serde_json::to_value(AgentEvent::ToolStart { id: "1".into(), name: "bleep_now".into(), args: serde_json::json!({}) }).unwrap();
        assert_eq!(v["type"], "tool_start");
        assert_eq!(v["name"], "bleep_now");
    }
}
