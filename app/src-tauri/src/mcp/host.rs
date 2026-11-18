//! The tool host: one place that decides how every tool call runs (build plan 8.2).
//! Read and safe tools run at once; write tools run only after the user approves a card
//! showing the exact payload; never tools do not exist.
use crate::connectors::{Connector, Risk, ToolSpec};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

pub const MAX_STEPS: u32 = 8;
pub const STEP_LIMIT: &str = "Step limit reached for this turn. Answer with what you have.";
pub const DECLINED: &str = "Declined by the user. Nothing was changed.";
pub const EXPIRED: &str = "The user did not answer within 10 minutes. Nothing was changed.";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Decision {
    Approved,
    Declined,
    Expired,
}

/// Asks the user about one write. Implemented by `Approvals`.
#[async_trait]
pub trait Gate: Send + Sync {
    /// Shows a card and waits. Returns the approval id and the answer.
    async fn ask(&self, tool: &str, payload: Value, turn: Option<i64>) -> anyhow::Result<(i64, Decision)>;
    /// Records how an approved write ended.
    fn finish(&self, id: i64, ok: bool, result: &str);
}

/// Refuses every write. Used until the approval cards are wired in.
pub struct NoGate;

#[async_trait]
impl Gate for NoGate {
    async fn ask(&self, _tool: &str, _payload: Value, _turn: Option<i64>) -> anyhow::Result<(i64, Decision)> {
        anyhow::bail!("Approvals are not available yet.")
    }
    fn finish(&self, _id: i64, _ok: bool, _result: &str) {}
}

/// Which chat turn is running, and how many tool steps it has used.
#[derive(Clone, Default)]
pub struct TurnTracker(Arc<TurnInner>);

#[derive(Default)]
struct TurnInner {
    current: Mutex<Option<i64>>,
    steps: AtomicU32,
}

impl TurnTracker {
    pub fn begin(&self, message_id: i64) {
        *self.0.current.lock().unwrap() = Some(message_id);
        self.0.steps.store(0, Ordering::SeqCst);
    }

    pub fn end(&self) {
        *self.0.current.lock().unwrap() = None;
    }

    pub fn current(&self) -> Option<i64> {
        *self.0.current.lock().unwrap()
    }

    /// Counts one step and returns the new count.
    pub fn step(&self) -> u32 {
        self.0.steps.fetch_add(1, Ordering::SeqCst) + 1
    }
}

pub struct ToolHost {
    /// Built-in connectors and remote servers. Remote ones come and go at run time.
    connectors: std::sync::RwLock<Vec<Arc<dyn Connector>>>,
    /// Built-in connectors that are turned off: their tools are not listed and do not run.
    hidden: std::sync::RwLock<std::collections::HashSet<String>>,
    gate: Arc<dyn Gate>,
    turns: TurnTracker,
    log: Option<Arc<crate::store::Store>>,
}

impl ToolHost {
    pub fn new(connectors: Vec<Arc<dyn Connector>>, gate: Arc<dyn Gate>, turns: TurnTracker) -> Arc<ToolHost> {
        Arc::new(ToolHost { connectors: connectors.into(), hidden: Default::default(), gate, turns, log: None })
    }

    /// Like `new`, and writes every call to the Activity log.
    pub fn new_logged(connectors: Vec<Arc<dyn Connector>>, gate: Arc<dyn Gate>, turns: TurnTracker, store: Arc<crate::store::Store>) -> Arc<ToolHost> {
        Arc::new(ToolHost { connectors: connectors.into(), hidden: Default::default(), gate, turns, log: Some(store) })
    }

    fn snapshot(&self) -> Vec<Arc<dyn Connector>> {
        self.connectors.read().unwrap().clone()
    }

    /// The connectors that are on.
    fn active(&self) -> Vec<Arc<dyn Connector>> {
        let hidden = self.hidden.read().unwrap();
        self.snapshot().into_iter().filter(|c| !hidden.contains(c.id())).collect()
    }

    /// Turns built-in connectors off by id (Settings > Connectors). The rest are on.
    pub fn set_hidden(&self, ids: impl IntoIterator<Item = String>) {
        *self.hidden.write().unwrap() = ids.into_iter().collect();
    }

    /// Every tool the model may see. `Never` tools are not listed.
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.active().iter().flat_map(|c| c.tools()).filter(|t| t.risk != Risk::Never).collect()
    }

    fn find(&self, name: &str) -> Option<(Arc<dyn Connector>, ToolSpec)> {
        self.active().into_iter().find_map(|c| c.tools().into_iter().find(|t| t.name == name).map(|t| (c.clone(), t)))
    }

    /// Adds a connector at the end, after the built-in ones.
    pub fn add(&self, c: Arc<dyn Connector>) {
        self.connectors.write().unwrap().push(c);
    }

    /// Swaps the connector with the same id, or adds it.
    pub fn replace(&self, c: Arc<dyn Connector>) {
        let mut list = self.connectors.write().unwrap();
        match list.iter().position(|x| x.id() == c.id()) {
            Some(i) => list[i] = c,
            None => list.push(c),
        }
    }

    /// Removes a connector by id. A call already running keeps its own handle and ends.
    pub fn remove(&self, id: &str) -> bool {
        let mut list = self.connectors.write().unwrap();
        let n = list.len();
        list.retain(|c| c.id() != id);
        list.len() != n
    }

    /// Changes when a tool the model sees is added, removed, renamed, or gets a new
    /// description or schema. Not when only its risk changes: the model does not see that.
    pub fn tools_hash(&self) -> String {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for t in self.specs() {
            t.name.hash(&mut h);
            t.description.hash(&mut h);
            t.schema.to_string().hash(&mut h);
        }
        format!("{:016x}", h.finish())
    }

    pub fn connector(&self, id: &str) -> Option<Arc<dyn Connector>> {
        self.snapshot().into_iter().find(|c| c.id() == id)
    }

    pub async fn call(&self, name: &str, args: Value) -> Result<Value, String> {
        let started = std::time::Instant::now();
        let risk = self.find(name).map(|(_, s)| s.risk);
        let out = self.call_inner(name, args).await;
        if let (Some(store), Some(risk)) = (&self.log, risk) {
            let preview = match &out {
                Ok(v) => crate::runtime::preview(&v.to_string(), 200),
                Err(e) => crate::runtime::preview(e, 200),
            };
            let risk = match risk {
                Risk::Read => "read",
                Risk::Safe => "safe",
                Risk::Write => "write",
                Risk::Never => "never",
            };
            let _ = store.log_tool(name, risk, out.is_ok(), started.elapsed().as_millis() as i64, Some(&preview));
        }
        out
    }

    async fn call_inner(&self, name: &str, args: Value) -> Result<Value, String> {
        let Some((conn, spec)) = self.find(name) else { return Err(format!("Unknown tool: {name}")) };
        if spec.risk == Risk::Never {
            return Err(format!("Unknown tool: {name}"));
        }
        if self.turns.step() > MAX_STEPS {
            return Err(STEP_LIMIT.into());
        }
        if spec.risk != Risk::Write {
            return conn.call(name, args).await;
        }
        let payload = conn.prepare(name, args).await?;
        let (id, decision) = self.gate.ask(name, payload.clone(), self.turns.current()).await.map_err(|e| e.to_string())?;
        match decision {
            Decision::Declined => Err(DECLINED.into()),
            Decision::Expired => Err(EXPIRED.into()),
            Decision::Approved => {
                let out = conn.execute(name, payload).await;
                match &out {
                    Ok(v) => self.gate.finish(id, true, &crate::runtime::preview(&v.to_string(), 200)),
                    Err(e) => self.gate.finish(id, false, e),
                }
                out
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::connectors::ConnectorStatus;
    use serde_json::json;

    pub struct FakeConnector {
        pub executed: Mutex<Vec<Value>>,
        pub prepare_fails: bool,
    }

    impl FakeConnector {
        pub fn new() -> Arc<FakeConnector> {
            Arc::new(FakeConnector { executed: Default::default(), prepare_fails: false })
        }
    }

    #[async_trait]
    impl Connector for FakeConnector {
        fn id(&self) -> &'static str {
            "fake"
        }
        fn tools(&self) -> Vec<ToolSpec> {
            let s = json!({"type": "object"});
            vec![
                ToolSpec { name: "fake_read".into(), description: "r".into(), schema: s.clone(), risk: Risk::Read },
                ToolSpec { name: "fake_write".into(), description: "w".into(), schema: s.clone(), risk: Risk::Write },
                ToolSpec { name: "fake_forward".into(), description: "n".into(), schema: s, risk: Risk::Never },
            ]
        }
        fn status(&self) -> ConnectorStatus {
            ConnectorStatus::Connected { account: "x".into() }
        }
        async fn call(&self, _tool: &str, args: Value) -> Result<Value, String> {
            Ok(json!({"read": args}))
        }
        async fn prepare(&self, _tool: &str, args: Value) -> Result<Value, String> {
            if self.prepare_fails {
                return Err("bad args".into());
            }
            Ok(json!({"prepared": args, "end": "18:30"}))
        }
        async fn execute(&self, _tool: &str, payload: Value) -> Result<Value, String> {
            self.executed.lock().unwrap().push(payload.clone());
            Ok(json!({"done": true}))
        }
    }

    /// Answers every ask with `decision` and records the asks.
    pub struct FakeGate {
        pub decision: Decision,
        pub asks: Mutex<Vec<(String, Value, Option<i64>)>>,
        pub finished: Mutex<Vec<(i64, bool)>>,
    }

    impl FakeGate {
        pub fn new(decision: Decision) -> Arc<FakeGate> {
            Arc::new(FakeGate { decision, asks: Default::default(), finished: Default::default() })
        }
    }

    #[async_trait]
    impl Gate for FakeGate {
        async fn ask(&self, tool: &str, payload: Value, turn: Option<i64>) -> anyhow::Result<(i64, Decision)> {
            self.asks.lock().unwrap().push((tool.into(), payload, turn));
            Ok((7, self.decision))
        }
        fn finish(&self, id: i64, ok: bool, _result: &str) {
            self.finished.lock().unwrap().push((id, ok));
        }
    }

    fn host(c: Arc<FakeConnector>, g: Arc<FakeGate>) -> (Arc<ToolHost>, TurnTracker) {
        let turns = TurnTracker::default();
        (ToolHost::new(vec![c, Arc::new(crate::connectors::bleep::BleepConnector)], g, turns.clone()), turns)
    }

    #[tokio::test]
    async fn a_connector_that_is_off_has_no_tools_and_cannot_run() {
        let (g, c) = (FakeGate::new(Decision::Declined), FakeConnector::new());
        let (h, _) = host(c, g);
        let before = h.tools_hash();
        h.set_hidden(["bleep".to_string()]);
        assert!(h.specs().iter().all(|t| t.name != "bleep_now"));
        assert!(h.call("bleep_now", json!({})).await.is_err());
        assert_ne!(h.tools_hash(), before, "the model gets a fresh session without the tools");
        h.set_hidden([]);
        assert!(h.call("bleep_now", json!({})).await.is_ok());
    }

    #[tokio::test]
    async fn read_tool_runs_at_once() {
        let (g, c) = (FakeGate::new(Decision::Declined), FakeConnector::new());
        let (h, _) = host(c, g.clone());
        assert_eq!(h.call("fake_read", json!({"q": 1})).await.unwrap(), json!({"read": {"q": 1}}));
        assert!(g.asks.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn write_tool_runs_prepared_payload_only_after_approval() {
        let (g, c) = (FakeGate::new(Decision::Approved), FakeConnector::new());
        let (h, turns) = host(c.clone(), g.clone());
        turns.begin(42);
        assert_eq!(h.call("fake_write", json!({"title": "Pay rent"})).await.unwrap(), json!({"done": true}));
        let prepared = json!({"prepared": {"title": "Pay rent"}, "end": "18:30"});
        assert_eq!(g.asks.lock().unwrap()[0], ("fake_write".to_string(), prepared.clone(), Some(42)));
        assert_eq!(*c.executed.lock().unwrap(), vec![prepared]);
        assert_eq!(*g.finished.lock().unwrap(), vec![(7, true)]);
    }

    #[tokio::test]
    async fn declined_write_returns_declined_and_never_executes() {
        let (g, c) = (FakeGate::new(Decision::Declined), FakeConnector::new());
        let (h, _) = host(c.clone(), g);
        assert_eq!(h.call("fake_write", json!({})).await, Err(DECLINED.to_string()));
        assert!(c.executed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn expired_write_returns_expired_and_never_executes() {
        let (g, c) = (FakeGate::new(Decision::Expired), FakeConnector::new());
        let (h, _) = host(c.clone(), g);
        assert_eq!(h.call("fake_write", json!({})).await, Err(EXPIRED.to_string()));
        assert!(c.executed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn ninth_step_in_a_turn_is_refused() {
        let (h, turns) = host(FakeConnector::new(), FakeGate::new(Decision::Approved));
        turns.begin(1);
        for _ in 0..MAX_STEPS {
            h.call("fake_read", json!({})).await.unwrap();
        }
        assert_eq!(h.call("fake_read", json!({})).await, Err(STEP_LIMIT.to_string()));
        turns.begin(2);
        assert!(h.call("fake_read", json!({})).await.is_ok());
    }

    #[tokio::test]
    async fn prepare_error_makes_no_card() {
        let g = FakeGate::new(Decision::Approved);
        let c = Arc::new(FakeConnector { executed: Default::default(), prepare_fails: true });
        let (h, _) = host(c.clone(), g.clone());
        assert_eq!(h.call("fake_write", json!({})).await, Err("bad args".to_string()));
        assert!(g.asks.lock().unwrap().is_empty());
        assert!(c.executed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn never_tools_cannot_be_called() {
        let (h, _) = host(FakeConnector::new(), FakeGate::new(Decision::Approved));
        assert!(h.call("fake_forward", json!({})).await.is_err());
        assert!(h.call("nope", json!({})).await.unwrap_err().contains("Unknown tool"));
    }

    #[tokio::test]
    async fn host_logs_each_call_with_outcome() {
        let d = tempfile::tempdir().unwrap();
        let store = Arc::new(crate::store::Store::open(&d.path().join("b.db")).unwrap());
        let h = ToolHost::new_logged(vec![FakeConnector::new()], FakeGate::new(Decision::Declined), TurnTracker::default(), store.clone());
        h.call("fake_read", json!({"q": 1})).await.unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert!(h.call("fake_write", json!({})).await.is_err());
        let items = store.activity(10).unwrap();
        let tools: Vec<_> = items.iter().filter(|i| i.kind == "tool").collect();
        assert_eq!(tools.len(), 2);
        assert_eq!((tools[0].name.as_str(), tools[0].ok, tools[0].detail.as_deref()), ("fake_write", false, Some(DECLINED)));
        assert_eq!((tools[1].name.as_str(), tools[1].ok), ("fake_read", true));
        assert!(tools[1].detail.as_deref().unwrap().contains("read"));
    }

    /// A connector with a name chosen at run time, like a remote server.
    struct Named {
        id: String,
        tools: Vec<String>,
        delay_ms: u64,
    }

    #[async_trait]
    impl Connector for Named {
        fn id(&self) -> &str {
            &self.id
        }
        fn tools(&self) -> Vec<ToolSpec> {
            self.tools.iter().map(|t| ToolSpec { name: t.clone().into(), description: format!("[{}] tool", self.id).into(), schema: json!({"type": "object"}), risk: Risk::Read }).collect()
        }
        fn status(&self) -> ConnectorStatus {
            ConnectorStatus::NeedsSetup
        }
        async fn call(&self, tool: &str, _args: Value) -> Result<Value, String> {
            tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
            Ok(json!({"tool": tool}))
        }
    }

    fn named(id: &str, tools: &[&str], delay_ms: u64) -> Arc<Named> {
        Arc::new(Named { id: id.into(), tools: tools.iter().map(|t| t.to_string()).collect(), delay_ms })
    }

    fn names(h: &ToolHost) -> Vec<String> {
        h.specs().iter().map(|t| t.name.to_string()).collect()
    }

    #[tokio::test]
    async fn add_remove_replace_change_the_tool_set() {
        let (h, _) = host(FakeConnector::new(), FakeGate::new(Decision::Approved));
        let before = names(&h);
        h.add(named("wiki", &["wiki_ask"], 0));
        assert_eq!(names(&h).last().map(String::as_str), Some("wiki_ask"));
        assert_eq!(h.call("wiki_ask", json!({})).await.unwrap(), json!({"tool": "wiki_ask"}));
        h.replace(named("wiki", &["wiki_read", "wiki_search"], 0));
        assert!(!names(&h).contains(&"wiki_ask".to_string()));
        assert!(names(&h).ends_with(&["wiki_read".to_string(), "wiki_search".to_string()]));
        assert!(h.remove("wiki"));
        assert_eq!(names(&h), before, "built-in tools are unchanged");
        assert_eq!(h.call("wiki_read", json!({})).await.unwrap_err(), "Unknown tool: wiki_read");
        assert!(!h.remove("wiki"), "already gone");
        assert!(h.connector("fake").is_some() && h.connector("wiki").is_none());
    }

    #[tokio::test]
    async fn tools_hash_follows_what_the_model_sees() {
        let (h, _) = host(FakeConnector::new(), FakeGate::new(Decision::Approved));
        let a = h.tools_hash();
        assert_eq!(a, h.tools_hash(), "stable");
        h.add(named("wiki", &["wiki_ask"], 0));
        let b = h.tools_hash();
        assert_ne!(a, b);
        h.remove("wiki");
        assert_eq!(h.tools_hash(), a);
    }

    #[tokio::test]
    async fn a_call_started_before_remove_still_ends() {
        let (h, _) = host(FakeConnector::new(), FakeGate::new(Decision::Approved));
        h.add(named("wiki", &["wiki_ask"], 80));
        let h2 = h.clone();
        let call = tokio::spawn(async move { h2.call("wiki_ask", json!({})).await });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        h.remove("wiki");
        assert_eq!(call.await.unwrap().unwrap(), json!({"tool": "wiki_ask"}));
    }

    #[test]
    fn never_tools_are_not_listed() {
        let (h, _) = host(FakeConnector::new(), FakeGate::new(Decision::Approved));
        assert_eq!(names(&h), vec!["fake_read", "fake_write", "bleep_now"]);
    }

    #[test]
    fn all_tool_names_are_snake_case() {
        let (h, _) = host(FakeConnector::new(), FakeGate::new(Decision::Approved));
        for t in h.specs() {
            assert!(t.name.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{}", t.name);
        }
    }
}
