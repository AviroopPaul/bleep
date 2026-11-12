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
    connectors: Vec<Arc<dyn Connector>>,
    gate: Arc<dyn Gate>,
    turns: TurnTracker,
}

impl ToolHost {
    pub fn new(connectors: Vec<Arc<dyn Connector>>, gate: Arc<dyn Gate>, turns: TurnTracker) -> Arc<ToolHost> {
        Arc::new(ToolHost { connectors, gate, turns })
    }

    /// Every tool the model may see. `Never` tools are not listed.
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.connectors.iter().flat_map(|c| c.tools()).filter(|t| t.risk != Risk::Never).collect()
    }

    fn find(&self, name: &str) -> Option<(Arc<dyn Connector>, ToolSpec)> {
        self.connectors.iter().find_map(|c| c.tools().into_iter().find(|t| t.name == name).map(|t| (c.clone(), t)))
    }

    pub async fn call(&self, name: &str, args: Value) -> Result<Value, String> {
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
                ToolSpec { name: "fake_read", description: "r", schema: s.clone(), risk: Risk::Read },
                ToolSpec { name: "fake_write", description: "w", schema: s.clone(), risk: Risk::Write },
                ToolSpec { name: "fake_forward", description: "n", schema: s, risk: Risk::Never },
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

    #[test]
    fn never_tools_are_not_listed() {
        let (h, _) = host(FakeConnector::new(), FakeGate::new(Decision::Approved));
        let names: Vec<&str> = h.specs().iter().map(|t| t.name).collect();
        assert_eq!(names, vec!["fake_read", "fake_write", "bleep_now"]);
    }

    #[test]
    fn all_tool_names_are_snake_case() {
        let (h, _) = host(FakeConnector::new(), FakeGate::new(Decision::Approved));
        for t in h.specs() {
            assert!(t.name.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{}", t.name);
        }
    }
}
