//! Changing the runtime without a restart, and the daily spend cap.
//!
//! `SwitchRuntime` is the one runtime every part of bleep holds. Settings swaps what is
//! inside. `CappedRuntime` wraps it for proactive work (triage, screenshot checks, patterns),
//! which stops for the day once API spend reaches the cap.
use super::*;
use crate::store::Store;
use std::sync::{Arc, RwLock};

pub struct SwitchRuntime {
    inner: RwLock<Arc<dyn AgentRuntime>>,
    generation: std::sync::atomic::AtomicU64,
}

impl SwitchRuntime {
    pub fn new(rt: Arc<dyn AgentRuntime>) -> Arc<SwitchRuntime> {
        Arc::new(SwitchRuntime { inner: RwLock::new(rt), generation: Default::default() })
    }

    pub fn set(&self, rt: Arc<dyn AgentRuntime>) {
        *self.inner.write().unwrap() = rt;
        self.generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn current(&self) -> Arc<dyn AgentRuntime> {
        self.inner.read().unwrap().clone()
    }
}

#[async_trait]
impl AgentRuntime for SwitchRuntime {
    fn id(&self) -> &'static str {
        self.current().id()
    }
    fn instance(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::SeqCst)
    }
    async fn health(&self) -> RuntimeHealth {
        self.current().health().await
    }
    async fn start_session(&self, cfg: SessionConfig) -> Result<SessionHandle> {
        self.current().start_session(cfg).await
    }
    async fn send(&self, s: &SessionHandle, msg: UserMessage) -> Result<EventStream> {
        self.current().send(s, msg).await
    }
    async fn one_shot(&self, req: OneShot) -> Result<Value> {
        self.current().one_shot(req).await
    }
    async fn one_shot_with_usage(&self, req: OneShot) -> Result<(Value, Option<ShotUsage>)> {
        self.current().one_shot_with_usage(req).await
    }
}

pub const CAP_REACHED: &str = "Today's API spend cap is reached, so proactive work is paused until tomorrow.";

/// Today's API spend against the cap. `cap` gives the cap only while the API runtime is in use.
pub struct Budget {
    store: Arc<Store>,
    cap: Arc<dyn Fn() -> Option<f64> + Send + Sync>,
    today: Arc<dyn Fn() -> String + Send + Sync>,
}

impl Budget {
    pub fn new(store: Arc<Store>, cap: Arc<dyn Fn() -> Option<f64> + Send + Sync>) -> Arc<Budget> {
        Arc::new(Budget { store, cap, today: Arc::new(|| chrono::Local::now().date_naive().to_string()) })
    }

    pub fn with_today(store: Arc<Store>, cap: Arc<dyn Fn() -> Option<f64> + Send + Sync>, today: Arc<dyn Fn() -> String + Send + Sync>) -> Arc<Budget> {
        Arc::new(Budget { store, cap, today })
    }

    pub fn over(&self) -> bool {
        match (self.cap)() {
            // 0 (or a broken value) means no cap.
            Some(cap) if cap.is_finite() && cap > 0.0 => self.store.spend_on(&(self.today)()).map(|s| s.cost >= cap).unwrap_or(false),
            _ => false,
        }
    }
}

pub struct CappedRuntime {
    inner: Arc<dyn AgentRuntime>,
    budget: Arc<Budget>,
}

impl CappedRuntime {
    pub fn new(inner: Arc<dyn AgentRuntime>, budget: Arc<Budget>) -> Arc<CappedRuntime> {
        Arc::new(CappedRuntime { inner, budget })
    }
}

#[async_trait]
impl AgentRuntime for CappedRuntime {
    fn id(&self) -> &'static str {
        self.inner.id()
    }
    fn instance(&self) -> u64 {
        self.inner.instance()
    }
    async fn health(&self) -> RuntimeHealth {
        self.inner.health().await
    }
    async fn start_session(&self, cfg: SessionConfig) -> Result<SessionHandle> {
        self.inner.start_session(cfg).await
    }
    async fn send(&self, s: &SessionHandle, msg: UserMessage) -> Result<EventStream> {
        if self.budget.over() {
            anyhow::bail!(CAP_REACHED);
        }
        self.inner.send(s, msg).await
    }
    async fn one_shot(&self, req: OneShot) -> Result<Value> {
        Ok(self.one_shot_with_usage(req).await?.0)
    }
    async fn one_shot_with_usage(&self, req: OneShot) -> Result<(Value, Option<ShotUsage>)> {
        if self.budget.over() {
            anyhow::bail!(CAP_REACHED);
        }
        self.inner.one_shot_with_usage(req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::fake::FakeRuntime;
    use crate::store::api::ApiUsage;
    use serde_json::json;
    use std::sync::Mutex;

    fn shot() -> OneShot {
        OneShot { prompt: "p".into(), schema: json!({}), tier: Tier::Small, images: vec![] }
    }

    #[tokio::test]
    async fn switch_sends_to_whatever_is_inside_now() {
        let a = Arc::new(FakeRuntime::new(vec![]));
        let b = Arc::new(FakeRuntime::new(vec![]));
        a.one_shot_replies.lock().unwrap().push_back(Ok(json!("a")));
        b.one_shot_replies.lock().unwrap().push_back(Ok(json!("b")));
        let s = SwitchRuntime::new(a.clone());
        assert_eq!(s.one_shot(shot()).await.unwrap(), json!("a"));
        s.set(b.clone());
        assert_eq!(s.one_shot(shot()).await.unwrap(), json!("b"));
        assert_eq!(b.one_shots.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn the_cap_stops_proactive_calls_only_while_the_api_runs() {
        let d = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&d.path().join("b.db")).unwrap());
        let cap: Arc<Mutex<Option<f64>>> = Arc::new(Mutex::new(Some(1.0)));
        let c = cap.clone();
        let budget = Budget::with_today(store.clone(), Arc::new(move || *c.lock().unwrap()), Arc::new(|| "2026-11-15".into()));
        let fake = Arc::new(FakeRuntime::new(vec![]));
        let capped = CappedRuntime::new(fake.clone(), budget.clone());
        assert!(capped.one_shot(shot()).await.is_ok());
        store.log_api_usage(&ApiUsage { day: "2026-11-15", provider: "p", model: "m", purpose: "chat", tokens_in: 1, cache_read: 0, cache_write: 0, tokens_out: 1, cost: Some(1.0) }).unwrap();
        assert!(budget.over());
        assert_eq!(capped.one_shot(shot()).await.unwrap_err().to_string(), CAP_REACHED);
        assert_eq!(fake.one_shots.lock().unwrap().len(), 1, "the capped call never reached the runtime");
        *cap.lock().unwrap() = None;
        assert!(!budget.over(), "no cap while Claude Code runs");
        for odd in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            *cap.lock().unwrap() = Some(odd);
            assert!(!budget.over(), "{odd} means no cap");
        }
    }
}
