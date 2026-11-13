//! Watches my Codex sessions through the shared app-server daemon (build plan 8.11).
//! bleep connects as one more client over the daemon's Unix socket (JSON-RPC in
//! WebSocket frames), subscribes to a thread only while it waits on an approval, answers
//! the asks its rules allow, and leaves the rest to the terminal or to my tap in bleep.
//! It never starts the daemon.
use super::hub::AgentWatch;
use super::rules::{split_command, Ask};
use super::{AgentSession, AgentState, AgentTool, AskDecision, AskKind};
use crate::store::Store;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub const POLL: Duration = Duration::from_secs(10);
pub const BACKOFF_START: Duration = Duration::from_secs(10);
pub const BACKOFF_MAX: Duration = Duration::from_secs(300);

#[async_trait]
pub trait RpcPipe: Send + Sync {
    async fn send(&self, msg: Value) -> Result<(), String>;
    /// `None` when the connection closed.
    async fn recv(&self) -> Option<Value>;
}

pub fn control_socket(codex_home: &Path) -> PathBuf {
    codex_home.join("app-server-control").join("app-server-control.sock")
}

/// The daemon runs when its control socket (a symlink) points at a live file.
pub fn daemon_running(codex_home: &Path) -> bool {
    control_socket(codex_home).exists()
}

pub fn next_backoff(d: Duration) -> Duration {
    (d * 2).min(BACKOFF_MAX)
}

/// `/bin/zsh -lc '<cmd>'` → `<cmd>`, when the wrapper reads cleanly.
fn unwrap_shell(cmd: &str) -> String {
    if let Some(segs) = split_command(cmd) {
        if let [seg] = segs.as_slice() {
            if let [sh, flag, inner] = seg.as_slice() {
                let name = sh.rsplit('/').next().unwrap_or(sh);
                if matches!(name, "sh" | "bash" | "zsh") && matches!(flag.as_str(), "-c" | "-lc") {
                    return inner.clone();
                }
            }
        }
    }
    cmd.to_string()
}

pub fn ask_from_codex(method: &str, params: &Value) -> Option<Ask> {
    let cwd = params.get("cwd").and_then(Value::as_str).unwrap_or("").to_string();
    let (tool_name, command) = match method {
        "item/commandExecution/requestApproval" => ("commandExecution", params.get("command").and_then(Value::as_str).map(unwrap_shell)),
        "item/fileChange/requestApproval" => ("fileChange", None),
        "item/permissions/requestApproval" => ("permissions", None),
        _ => return None,
    };
    Some(Ask { kind: AskKind::Permission, tool_name: tool_name.into(), command, paths: vec![], cwd, sandbox_off: false })
}

type Ws = tokio_tungstenite::WebSocketStream<tokio::net::UnixStream>;
type WsMsg = tokio_tungstenite::tungstenite::Message;

/// JSON-RPC in WebSocket text frames over the daemon's Unix socket.
pub struct WsPipe {
    tx: tokio::sync::Mutex<futures::stream::SplitSink<Ws, WsMsg>>,
    rx: tokio::sync::Mutex<futures::stream::SplitStream<Ws>>,
}

impl WsPipe {
    pub async fn connect(sock: &Path) -> Result<Arc<dyn RpcPipe>, String> {
        use futures::StreamExt;
        let s = tokio::net::UnixStream::connect(sock).await.map_err(|e| e.to_string())?;
        let (ws, _) = tokio_tungstenite::client_async("ws://localhost/", s).await.map_err(|e| e.to_string())?;
        let (tx, rx) = ws.split();
        Ok(Arc::new(WsPipe { tx: tokio::sync::Mutex::new(tx), rx: tokio::sync::Mutex::new(rx) }))
    }
}

#[async_trait]
impl RpcPipe for WsPipe {
    async fn send(&self, msg: Value) -> Result<(), String> {
        use futures::SinkExt;
        self.tx.lock().await.send(WsMsg::Text(msg.to_string().into())).await.map_err(|e| e.to_string())
    }

    async fn recv(&self) -> Option<Value> {
        use futures::StreamExt;
        let mut rx = self.rx.lock().await;
        loop {
            match rx.next().await? {
                Ok(WsMsg::Text(t)) => {
                    if let Ok(v) = serde_json::from_str(&t) {
                        return Some(v);
                    }
                }
                Ok(WsMsg::Close(_)) | Err(_) => return None,
                Ok(_) => {}
            }
        }
    }
}

enum Pending {
    Loaded,
    Read(String),
    Other,
}

#[derive(Default)]
struct State {
    pipe: Option<Arc<dyn RpcPipe>>,
    next_id: i64,
    pending: HashMap<i64, Pending>,
    subscribed: HashSet<String>,
    cwd: HashMap<String, String>,
}

pub struct CodexWatch {
    hub: Arc<AgentWatch>,
    store: Arc<Store>,
    state: tokio::sync::Mutex<State>,
}

pub const NOT_WAITING: &str = "This ask is no longer waiting.";

impl CodexWatch {
    pub fn new(hub: Arc<AgentWatch>, store: Arc<Store>) -> Arc<CodexWatch> {
        Arc::new(CodexWatch { hub, store, state: Default::default() })
    }

    /// Connects while `enabled` says so and the daemon runs, and reconnects with backoff.
    pub async fn run(self: Arc<Self>, codex_home: PathBuf, enabled: Arc<dyn Fn() -> bool + Send + Sync>) {
        let mut wait = BACKOFF_START;
        loop {
            if enabled() && daemon_running(&codex_home) {
                match WsPipe::connect(&control_socket(&codex_home)).await {
                    Ok(p) => {
                        self.serve(p, POLL).await;
                        wait = BACKOFF_START;
                    }
                    Err(e) => {
                        eprintln!("bleep: could not reach the Codex daemon: {e}");
                        wait = next_backoff(wait);
                    }
                }
            }
            tokio::time::sleep(wait).await;
        }
    }

    async fn request(&self, st: &mut State, method: &str, params: Value, kind: Pending) {
        st.next_id += 1;
        let id = st.next_id;
        st.pending.insert(id, kind);
        if let Some(p) = st.pipe.clone() {
            let _ = p.send(json!({"id": id, "method": method, "params": params})).await;
        }
    }

    fn session(&self, thread: &str, cwd: &str) -> AgentSession {
        let id = format!("codex:{thread}");
        let now = chrono::Utc::now();
        match self.store.agent_session(&id).ok().flatten() {
            Some(mut s) => {
                s.updated = now;
                s
            }
            None => AgentSession { id, tool: AgentTool::Codex, project: cwd.into(), title: None, transcript_path: None, state: AgentState::Running, last_ask: None, updated: now },
        }
    }

    /// Runs one connection until it closes.
    pub async fn serve(&self, pipe: Arc<dyn RpcPipe>, poll: Duration) {
        {
            let mut st = self.state.lock().await;
            *st = State { pipe: Some(pipe.clone()), ..State::default() };
            let info = json!({"clientInfo": {"name": "bleep", "title": "bleep", "version": env!("CARGO_PKG_VERSION")}});
            self.request(&mut st, "initialize", info, Pending::Other).await;
            let _ = pipe.send(json!({"method": "initialized"})).await;
        }
        let mut tick = tokio::time::interval(poll);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    let mut st = self.state.lock().await;
                    self.request(&mut st, "thread/loaded/list", json!({}), Pending::Loaded).await;
                }
                m = pipe.recv() => match m {
                    Some(v) => self.on_message(v).await,
                    None => break,
                },
            }
        }
        *self.state.lock().await = State::default();
        // Request ids belong to that connection. Whatever still waits is replayed when bleep
        // subscribes again, and then queued afresh.
        let _ = self.store.expire_queued_with_prefix("codex:");
    }

    async fn on_message(&self, v: Value) {
        let method = v.get("method").and_then(Value::as_str).map(str::to_string);
        let params = v.get("params").cloned().unwrap_or(Value::Null);
        match (method.as_deref(), v.get("id").cloned()) {
            (Some(m), Some(id)) => self.on_request(m, id, &params).await,
            (Some("serverRequest/resolved"), None) => {
                let thread = params.get("threadId").and_then(Value::as_str).unwrap_or("");
                let rpc = params.get("requestId").map(Value::to_string).unwrap_or_default();
                for a in self.store.queued_asks(&format!("codex:{thread}")).unwrap_or_default() {
                    if a.rpc_id.as_deref() == Some(rpc.as_str()) {
                        let _ = self.store.set_ask_decision(a.id, AskDecision::Expired, None);
                    }
                }
            }
            (Some("turn/completed"), None) => {
                let Some(thread) = params.get("threadId").and_then(Value::as_str) else { return };
                let mut st = self.state.lock().await;
                if st.subscribed.remove(thread) {
                    self.request(&mut st, "thread/unsubscribe", json!({"threadId": thread}), Pending::Other).await;
                }
                let cwd = st.cwd.get(thread).cloned().unwrap_or_default();
                drop(st);
                let mut s = self.session(thread, &cwd);
                self.hub.expire_queued(&s.id);
                s.state = AgentState::Done;
                s.last_ask = None;
                self.hub.session_seen(s);
            }
            (None, Some(id)) => self.on_response(id.as_i64().unwrap_or(-1), v.get("result").cloned().unwrap_or(Value::Null)).await,
            _ => {}
        }
    }

    async fn on_response(&self, id: i64, result: Value) {
        let mut st = self.state.lock().await;
        match st.pending.remove(&id) {
            Some(Pending::Loaded) => {
                for t in result.get("data").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                    self.request(&mut st, "thread/read", json!({"threadId": t}), Pending::Read(t.to_string())).await;
                }
            }
            Some(Pending::Read(t)) => {
                let th = result.get("thread").cloned().unwrap_or(Value::Null);
                let waiting = th.pointer("/status/activeFlags").and_then(Value::as_array).is_some_and(|f| f.iter().any(|x| x == "waitingOnApproval"));
                let cwd = th.get("cwd").and_then(Value::as_str).unwrap_or("").to_string();
                st.cwd.insert(t.clone(), cwd.clone());
                if waiting && !st.subscribed.contains(&t) {
                    st.subscribed.insert(t.clone());
                    // Subscribing replays the pending approval request to bleep.
                    self.request(&mut st, "thread/resume", json!({"threadId": t}), Pending::Other).await;
                    drop(st);
                    let mut s = self.session(&t, &cwd);
                    s.state = AgentState::Waiting;
                    self.hub.session_seen(s);
                }
            }
            _ => {}
        }
    }

    async fn on_request(&self, method: &str, id: Value, params: &Value) {
        let Some(mut ask) = ask_from_codex(method, params) else { return };
        let thread = params.get("threadId").and_then(Value::as_str).unwrap_or("").to_string();
        let rpc = id.to_string();
        let (pipe, cwd) = {
            let st = self.state.lock().await;
            (st.pipe.clone(), st.cwd.get(&thread).cloned().unwrap_or_default())
        };
        if ask.cwd.is_empty() {
            ask.cwd = cwd.clone();
        }
        let session = self.session(&thread, if cwd.is_empty() { &ask.cwd } else { &cwd });
        // A request replayed on a new subscription is the same ask.
        if self.store.queued_asks(&session.id).unwrap_or_default().iter().any(|a| a.rpc_id.as_deref() == Some(rpc.as_str())) {
            return;
        }
        let (_, decision) = self.hub.decide(&session, ask, Some(rpc)).await;
        if decision == AskDecision::Auto {
            if let Some(p) = pipe {
                let _ = p.send(json!({"id": id, "result": {"decision": "accept"}})).await;
            }
        }
    }

    /// My tap on a queued Codex ask.
    pub async fn answer(&self, ask_id: i64, approve: bool) -> Result<(), String> {
        let a = self.store.agent_ask(ask_id).ok().flatten().ok_or(NOT_WAITING)?;
        if a.decision != AskDecision::Queued {
            return Err(NOT_WAITING.into());
        }
        let rpc = a.rpc_id.clone().filter(|_| a.session_id.starts_with("codex:")).ok_or("Answer this one in the terminal.")?;
        let pipe = self.state.lock().await.pipe.clone().ok_or("Codex is not connected. Answer in Codex.")?;
        let to = if approve { AskDecision::Approved } else { AskDecision::Denied };
        if !self.store.set_ask_decision(ask_id, to, None).map_err(|e| e.to_string())? {
            return Err(NOT_WAITING.into());
        }
        let id: Value = serde_json::from_str(&rpc).unwrap_or(Value::String(rpc));
        let decision = if approve { "accept" } else { "decline" };
        pipe.send(json!({"id": id, "result": {"decision": decision}})).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agentwatch::hub::testing::hub;
    use std::sync::Mutex;
    use tokio::sync::mpsc;

    fn fx(name: &str) -> Value {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agents/codex").join(name);
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    /// Answers bleep's requests from the fixtures; tests push server messages in.
    struct FakePipe {
        sent: Mutex<Vec<Value>>,
        tx: mpsc::UnboundedSender<Option<Value>>,
        rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<Option<Value>>>,
    }

    impl FakePipe {
        fn new() -> Arc<FakePipe> {
            let (tx, rx) = mpsc::unbounded_channel();
            Arc::new(FakePipe { sent: Default::default(), tx, rx: tokio::sync::Mutex::new(rx) })
        }
        fn push(&self, v: Value) {
            self.tx.send(Some(v)).unwrap();
        }
        fn close(&self) {
            self.tx.send(None).unwrap();
        }
        fn sent_methods(&self) -> Vec<(String, Value)> {
            self.sent.lock().unwrap().iter().filter_map(|m| Some((m.get("method")?.as_str()?.to_string(), m.get("params").cloned().unwrap_or(Value::Null)))).collect()
        }
        fn responses(&self) -> Vec<Value> {
            self.sent.lock().unwrap().iter().filter(|m| m.get("method").is_none()).cloned().collect()
        }
    }

    #[async_trait]
    impl RpcPipe for FakePipe {
        async fn send(&self, msg: Value) -> Result<(), String> {
            self.sent.lock().unwrap().push(msg.clone());
            let (Some(id), Some(method)) = (msg.get("id").cloned(), msg.get("method").and_then(Value::as_str)) else { return Ok(()) };
            let result = match method {
                "initialize" => fx("initialize_response.json")["result"].clone(),
                "thread/loaded/list" => fx("loaded_list_response.json")["result"].clone(),
                "thread/read" if msg["params"]["threadId"] == "thr_01a1" => fx("thread_read_waiting.json")["result"].clone(),
                "thread/read" => fx("thread_read_idle.json")["result"].clone(),
                "thread/unsubscribe" => fx("unsubscribe_response.json")["result"].clone(),
                _ => json!({}),
            };
            self.tx.send(Some(json!({"id": id, "result": result}))).unwrap();
            Ok(())
        }
        async fn recv(&self) -> Option<Value> {
            self.rx.lock().await.recv().await.flatten()
        }
    }

    async fn until(what: &str, f: impl Fn() -> bool) {
        for _ in 0..200 {
            if f() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("timed out waiting for {what}");
    }

    struct Rig {
        _d: tempfile::TempDir,
        store: Arc<Store>,
        pipe: Arc<FakePipe>,
        cw: Arc<CodexWatch>,
    }

    async fn rig(auto: bool) -> Rig {
        let (d, store, h) = hub(auto);
        let pipe = FakePipe::new();
        let cw = CodexWatch::new(h, store.clone());
        let (c, p) = (cw.clone(), pipe.clone());
        tokio::spawn(async move { c.serve(p, Duration::from_millis(50)).await });
        let p = pipe.clone();
        until("subscribe", || p.sent_methods().iter().any(|(m, _)| m == "thread/resume")).await;
        Rig { _d: d, store, pipe, cw }
    }

    const SESSION: &str = "codex:thr_01a1";

    #[tokio::test]
    async fn subscribes_only_to_waiting_threads() {
        let r = rig(true).await;
        let m = r.pipe.sent_methods();
        assert_eq!(m[0].0, "initialize");
        assert_eq!(m[0].1["clientInfo"]["name"], "bleep");
        let resumed: Vec<_> = m.iter().filter(|(x, _)| x == "thread/resume").map(|(_, p)| p["threadId"].clone()).collect();
        assert!(resumed.iter().all(|t| t == "thr_01a1"), "{resumed:?}");
        tokio::time::sleep(Duration::from_millis(150)).await;
        let n = r.pipe.sent_methods().iter().filter(|(x, _)| x == "thread/resume").count();
        assert_eq!(n, 1, "subscribes once");
    }

    #[tokio::test]
    async fn auto_rule_accepts_over_rpc() {
        let r = rig(true).await;
        r.pipe.push(fx("request_approval_command.json"));
        let p = r.pipe.clone();
        until("accept", || p.responses().iter().any(|v| v["id"] == 0)).await;
        assert!(r.pipe.responses().contains(&json!({"id": 0, "result": {"decision": "accept"}})));
        let a = r.store.agent_asks_recent(1).unwrap().remove(0);
        assert_eq!((a.decision, a.rule.as_deref(), a.summary.as_str(), a.session_id.as_str()), (AskDecision::Auto, Some("npm test"), "wants to run npm test", SESSION));
    }

    #[tokio::test]
    async fn unsure_ask_is_left_for_the_terminal() {
        let r = rig(true).await;
        r.pipe.push(fx("request_approval_push.json"));
        let s = r.store.clone();
        until("queued", || !s.queued_asks(SESSION).unwrap().is_empty()).await;
        let q = r.store.queued_asks(SESSION).unwrap().remove(0);
        assert_eq!((q.rpc_id.as_deref(), q.summary.as_str()), (Some("1"), "wants to run git push --force origin main"));
        assert!(!r.pipe.responses().iter().any(|v| v["id"] == 1));
        assert_eq!(r.store.agent_session(SESSION).unwrap().unwrap().state, AgentState::Waiting);
    }

    #[tokio::test]
    async fn resolved_elsewhere_expires_and_later_tap_sends_nothing() {
        let r = rig(true).await;
        r.pipe.push(fx("request_approval_push.json"));
        let s = r.store.clone();
        until("queued", || !s.queued_asks(SESSION).unwrap().is_empty()).await;
        let id = r.store.queued_asks(SESSION).unwrap()[0].id;
        r.pipe.push(fx("server_request_resolved.json"));
        until("expired", || s.agent_ask(id).unwrap().unwrap().decision == AskDecision::Expired).await;
        assert!(r.cw.answer(id, true).await.is_err());
        assert!(!r.pipe.responses().iter().any(|v| v["id"] == 1));
    }

    #[tokio::test]
    async fn tap_in_bleep_answers_pending_request() {
        let r = rig(true).await;
        r.pipe.push(fx("request_approval_push.json"));
        let s = r.store.clone();
        until("queued", || !s.queued_asks(SESSION).unwrap().is_empty()).await;
        let id = r.store.queued_asks(SESSION).unwrap()[0].id;
        r.cw.answer(id, false).await.unwrap();
        assert!(r.pipe.responses().contains(&json!({"id": 1, "result": {"decision": "decline"}})));
        assert_eq!(r.store.agent_ask(id).unwrap().unwrap().decision, AskDecision::Denied);
        assert!(r.cw.answer(id, true).await.is_err(), "answers once");
    }

    #[tokio::test]
    async fn replayed_request_is_not_queued_twice() {
        let r = rig(true).await;
        r.pipe.push(fx("request_approval_push.json"));
        r.pipe.push(fx("request_approval_push.json"));
        r.pipe.push(fx("turn_completed.json"));
        let p = r.pipe.clone();
        until("unsubscribe", || p.sent_methods().iter().any(|(m, _)| m == "thread/unsubscribe")).await;
        assert_eq!(r.store.agent_asks_recent(10).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn unsubscribes_after_turn_completes() {
        let r = rig(true).await;
        r.pipe.push(fx("turn_completed.json"));
        let p = r.pipe.clone();
        until("unsubscribe", || p.sent_methods().iter().any(|(m, _)| m == "thread/unsubscribe")).await;
        let un = r.pipe.sent_methods().into_iter().find(|(m, _)| m == "thread/unsubscribe").unwrap();
        assert_eq!(un.1["threadId"], "thr_01a1");
        assert_eq!(r.store.agent_session(SESSION).unwrap().unwrap().state, AgentState::Done);
    }

    #[tokio::test]
    async fn disconnect_expires_queued_codex_asks() {
        let r = rig(true).await;
        r.pipe.push(fx("request_approval_push.json"));
        let s = r.store.clone();
        until("queued", || !s.queued_asks(SESSION).unwrap().is_empty()).await;
        let id = r.store.queued_asks(SESSION).unwrap()[0].id;
        r.pipe.close();
        until("expired", || s.agent_ask(id).unwrap().unwrap().decision == AskDecision::Expired).await;
        assert!(r.cw.answer(id, true).await.is_err());
    }

    #[tokio::test]
    async fn serve_returns_when_the_pipe_closes() {
        let (_d, store, h) = hub(true);
        let pipe = FakePipe::new();
        pipe.close();
        let cw = CodexWatch::new(h, store);
        tokio::time::timeout(Duration::from_secs(2), cw.serve(pipe, POLL)).await.unwrap();
        assert!(cw.answer(1, true).await.is_err());
    }

    #[test]
    fn backoff_doubles_to_five_minutes() {
        assert_eq!(next_backoff(BACKOFF_START), Duration::from_secs(20));
        assert_eq!(next_backoff(Duration::from_secs(200)), BACKOFF_MAX);
        assert_eq!(next_backoff(BACKOFF_MAX), BACKOFF_MAX);
    }

    #[test]
    fn shell_wrapper_is_unwrapped() {
        let a = ask_from_codex("item/commandExecution/requestApproval", &fx("request_approval_command.json")["params"]).unwrap();
        assert_eq!((a.tool_name.as_str(), a.command.as_deref(), a.cwd.as_str(), a.kind), ("commandExecution", Some("npm test"), "/Users/me/code/shop", AskKind::Permission));
        // The inner command is checked by the rules on its own, and `$` there still asks.
        let inner = ask_from_codex("item/commandExecution/requestApproval", &json!({"command": "/bin/zsh -lc 'npm test && echo $HOME'", "cwd": "/r"})).unwrap();
        assert_eq!(inner.command.as_deref(), Some("npm test && echo $HOME"));
        let rules: crate::agentwatch::rules::AgentRules = toml::from_str(crate::agentwatch::rules::DEFAULT_RULES).unwrap();
        assert!(!matches!(crate::agentwatch::rules::check(&rules, &inner), crate::agentwatch::rules::RuleVerdict::Allow { .. }));
        let raw = ask_from_codex("item/commandExecution/requestApproval", &json!({"command": "/bin/zsh -lc \"npm test\" && rm x", "cwd": "/r"})).unwrap();
        assert_eq!(raw.command.as_deref(), Some("/bin/zsh -lc \"npm test\" && rm x"), "not a plain wrapper: kept whole");
        let f = ask_from_codex("item/fileChange/requestApproval", &fx("request_approval_file_change.json")["params"]).unwrap();
        assert_eq!(f.tool_name, "fileChange");
        assert!(ask_from_codex("item/tool/requestUserInput", &json!({})).is_none());
    }

    #[test]
    fn daemon_check_follows_the_symlink() {
        let d = tempfile::tempdir().unwrap();
        assert!(!daemon_running(d.path()));
        let s = control_socket(d.path());
        std::fs::create_dir_all(s.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(d.path().join("gone.sock"), &s).unwrap();
        assert!(!daemon_running(d.path()), "dangling link");
        let _l = std::os::unix::net::UnixListener::bind(d.path().join("gone.sock")).unwrap();
        assert!(daemon_running(d.path()));
    }

    #[tokio::test]
    async fn ws_pipe_round_trips_over_unix_socket() {
        use futures::{SinkExt, StreamExt};
        let d = tempfile::tempdir().unwrap();
        let sock = d.path().join("s.sock");
        let l = tokio::net::UnixListener::bind(&sock).unwrap();
        tokio::spawn(async move {
            let (s, _) = l.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(s).await.unwrap();
            while let Some(Ok(m)) = ws.next().await {
                if let tokio_tungstenite::tungstenite::Message::Text(t) = m {
                    let v: Value = serde_json::from_str(&t).unwrap();
                    ws.send(tokio_tungstenite::tungstenite::Message::Text(json!({"id": v["id"], "result": "pong"}).to_string().into())).await.unwrap();
                }
            }
        });
        let p = WsPipe::connect(&sock).await.unwrap();
        p.send(json!({"id": 5, "method": "ping"})).await.unwrap();
        assert_eq!(p.recv().await.unwrap(), json!({"id": 5, "result": "pong"}));
    }
}
