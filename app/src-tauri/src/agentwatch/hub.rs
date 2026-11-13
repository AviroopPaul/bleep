//! The agent watch hub: every hook event and Codex ask comes here. It keeps session
//! state, runs the rules, records each ask, and pings me when an agent needs me.
use super::claude::{self, ClaudeEvent};
use super::rules::{check, AgentRules, Ask, RuleVerdict};
use super::screenshot::{may_go_on, wants_look, ScreenshotChecker};
use super::socket::HookHandler;
use super::transcript::{read_tail, TAIL_BYTES};
use super::{AgentSession, AgentState, AgentTool, AskDecision, AskKind, HookMsg, NewAsk};
use crate::config::AgentsConfig;
use crate::proactive::scheduler::Clock;
use crate::store::Store;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const SCREENSHOT_RULE: &str = "screenshot check";

/// How the hub tells me an agent waits. The Notifier in the app.
pub trait AgentPing: Send + Sync {
    fn waiting(&self, project: &str, summary: &str, ask_id: i64, waiting_now: usize);
}

pub struct WatchDeps {
    pub store: Arc<Store>,
    pub rules: Arc<dyn Fn() -> AgentRules + Send + Sync>,
    pub cfg: Arc<dyn Fn() -> AgentsConfig + Send + Sync>,
    pub paused: Arc<AtomicBool>,
    pub checker: Arc<dyn ScreenshotChecker>,
    pub ping: Arc<dyn AgentPing>,
    /// Tells the UI the agent list changed.
    pub emit: Arc<dyn Fn() + Send + Sync>,
    pub now: Clock,
}

pub struct AgentWatch {
    d: WatchDeps,
}

impl AgentWatch {
    pub fn new(d: WatchDeps) -> Arc<AgentWatch> {
        Arc::new(AgentWatch { d })
    }

    fn save(&self, s: &AgentSession) {
        if let Err(e) = self.d.store.upsert_agent_session(s) {
            eprintln!("bleep: could not save agent session: {e}");
        }
        (self.d.emit)();
    }

    fn waiting_now(&self) -> usize {
        self.d.store.agent_sessions((self.d.now)()).unwrap_or_default().iter().filter(|s| s.state == AgentState::Waiting).count()
    }

    /// Rules decide (D12). With auto-approve on, not paused, and a rule match, the ask is
    /// approved; anything else is queued for me with the reason, and I get a ping.
    pub async fn decide(&self, session: &AgentSession, ask: Ask, rpc_id: Option<String>) -> (i64, AskDecision) {
        let cfg = (self.d.cfg)();
        let summary = claude::summary(&ask);
        let payload = json!({"tool_name": ask.tool_name, "command": ask.command, "paths": ask.paths, "cwd": ask.cwd});
        let (decision, rule, checks) = match check(&(self.d.rules)(), &ask) {
            RuleVerdict::Allow { rule } if !cfg.auto_approve => (AskDecision::Queued, Some(rule), vec!["auto-approve is off".to_string()]),
            RuleVerdict::Allow { rule } if self.d.paused.load(Ordering::SeqCst) => (AskDecision::Queued, Some(rule), vec!["bleep is paused".to_string()]),
            RuleVerdict::Allow { rule } => (AskDecision::Auto, Some(rule), vec![]),
            RuleVerdict::NeedsYou { why } => (AskDecision::Queued, None, vec![why]),
        };
        // A queued ask records which rule would have matched only in `checks`.
        let rule = if decision == AskDecision::Auto { rule } else { None };
        let new = NewAsk { session_id: session.id.clone(), kind: ask.kind, summary: summary.clone(), payload, checks, decision, rule, rpc_id };
        let id = self.d.store.insert_agent_ask(&new).unwrap_or(0);
        let mut s = session.clone();
        s.updated = (self.d.now)();
        if decision == AskDecision::Auto {
            s.state = AgentState::Running;
            self.save(&s);
        } else {
            s.state = AgentState::Waiting;
            s.last_ask = Some(summary.clone());
            self.save(&s);
            self.d.ping.waiting(&s.project, &summary, id, self.waiting_now());
        }
        (id, decision)
    }

    /// Saves a session as seen now (Codex threads).
    pub fn session_seen(&self, s: AgentSession) {
        self.save(&s);
    }

    /// Queued asks of a session were answered somewhere else (the terminal, or Codex).
    pub fn expire_queued(&self, session_id: &str) {
        for a in self.d.store.queued_asks(session_id).unwrap_or_default() {
            let _ = self.d.store.set_ask_decision(a.id, AskDecision::Expired, None);
        }
    }

    fn queue_question(&self, s: &mut AgentSession, question: &str) {
        let summary = claude::question_summary(question);
        let new = NewAsk { session_id: s.id.clone(), kind: AskKind::Question, summary: summary.clone(), payload: json!({"question": question}), checks: vec![], decision: AskDecision::Queued, rule: None, rpc_id: None };
        let id = self.d.store.insert_agent_ask(&new).unwrap_or(0);
        s.state = AgentState::Waiting;
        s.last_ask = Some(summary.clone());
        self.save(s);
        self.d.ping.waiting(&s.project, &summary, id, self.waiting_now());
    }

    fn session_for(&self, id: String, tool: AgentTool, project: &str, transcript: Option<String>) -> AgentSession {
        let now = (self.d.now)();
        match self.d.store.agent_session(&id).ok().flatten() {
            Some(mut s) => {
                s.updated = now;
                if transcript.is_some() {
                    s.transcript_path = transcript;
                }
                s
            }
            None => AgentSession { id, tool, project: project.into(), title: None, transcript_path: transcript, state: AgentState::Running, last_ask: None, updated: now },
        }
    }

    async fn claude(&self, event: &str, p: &Value) -> Option<Value> {
        let sid = p.get("session_id")?.as_str()?;
        let cwd = p.get("cwd").and_then(Value::as_str).unwrap_or("");
        let transcript = p.get("transcript_path").and_then(Value::as_str).map(str::to_string);
        let mut s = self.session_for(format!("claude:{sid}"), AgentTool::Claude, cwd, transcript);
        match claude::classify(event, p)? {
            ClaudeEvent::Start | ClaudeEvent::ToolDone | ClaudeEvent::Prompt => {
                self.expire_queued(&s.id);
                if event == "UserPromptSubmit" && s.title.is_none() {
                    s.title = p.get("prompt").and_then(Value::as_str).map(|t| claude::cut(t, 60));
                }
                s.state = AgentState::Running;
                s.last_ask = None;
                self.save(&s);
                None
            }
            ClaudeEvent::Permission(ask) => {
                let (_, d) = self.decide(&s, ask, None).await;
                (d == AskDecision::Auto).then(claude::allow_output)
            }
            ClaudeEvent::Notify { idle } => {
                if self.d.store.queued_asks(&s.id).unwrap_or_default().is_empty() {
                    let q = if idle { "is waiting for you?" } else { "needs your permission?" };
                    self.queue_question(&mut s, q);
                } else {
                    s.state = AgentState::Waiting;
                    self.save(&s);
                }
                None
            }
            ClaudeEvent::Stop { active } => self.stop(s, p, active).await,
            ClaudeEvent::End => {
                self.expire_queued(&s.id);
                self.save(&s);
                let _ = self.d.store.end_agent_session(&s.id);
                (self.d.emit)();
                None
            }
        }
    }

    async fn stop(&self, mut s: AgentSession, p: &Value, active: bool) -> Option<Value> {
        // The turn ended: any permission it waited on was answered in the terminal.
        self.expire_queued(&s.id);
        let mut tail = s.transcript_path.as_deref().map(|t| read_tail(std::path::Path::new(t), TAIL_BYTES)).unwrap_or_default();
        if let Some(last) = p.get("last_assistant_message").and_then(Value::as_str) {
            let last = last.trim();
            tail.last_question = last.ends_with('?').then(|| last.to_string());
        }
        let Some(question) = tail.last_question.clone() else {
            s.state = AgentState::Done;
            s.last_ask = None;
            self.save(&s);
            return None;
        };
        let cfg = (self.d.cfg)();
        let can_act = cfg.auto_approve && cfg.screenshot_check && !self.d.paused.load(Ordering::SeqCst);
        if can_act && wants_look(&tail) {
            let now = (self.d.now)();
            let recent = self.d.store.go_ons_since(&s.id, now - chrono::Duration::hours(1)).unwrap_or(usize::MAX);
            let verdict = match (&tail.image, active) {
                // Never spend a model call on a check that cannot go on anyway.
                (Some(img), false) if recent < super::screenshot::MAX_GO_ONS_PER_HOUR => {
                    self.d.checker.check(tail.original_ask.as_deref().unwrap_or(""), &question, img).await
                }
                _ => None,
            };
            if may_go_on(active, recent, verdict.as_ref()) {
                let v = verdict?;
                let summary = format!("screenshot matched: {}", claude::question_summary(&question));
                let new = NewAsk { session_id: s.id.clone(), kind: AskKind::Question, summary, payload: json!({"question": question, "reason": v.reason}), checks: vec![], decision: AskDecision::Auto, rule: Some(SCREENSHOT_RULE.into()), rpc_id: None };
                let _ = self.d.store.insert_agent_ask(&new);
                s.state = AgentState::Running;
                s.last_ask = None;
                self.save(&s);
                // After the save: the session row must exist to hold its go-on history.
                let _ = self.d.store.push_go_on(&s.id, now);
                return Some(claude::go_on_output());
            }
        }
        self.queue_question(&mut s, &question);
        None
    }

    fn codex_notify(&self, p: &Value) {
        let Some(thread) = p.get("thread-id").and_then(Value::as_str) else { return };
        let cwd = p.get("cwd").and_then(Value::as_str).unwrap_or("");
        let mut s = self.session_for(format!("codex:{thread}"), AgentTool::Codex, cwd, None);
        if s.title.is_none() {
            s.title = p.get("input-messages").and_then(Value::as_array).and_then(|m| m.first()).and_then(Value::as_str).map(|t| claude::cut(t, 60));
        }
        self.expire_queued(&s.id);
        match p.get("last-assistant-message").and_then(Value::as_str).map(str::trim).filter(|m| m.ends_with('?')) {
            Some(q) => self.queue_question(&mut s, q),
            None => {
                s.state = AgentState::Done;
                s.last_ask = None;
                self.save(&s);
            }
        }
    }
}

#[async_trait]
impl HookHandler for AgentWatch {
    async fn handle(&self, msg: HookMsg) -> Option<Value> {
        match msg.source.as_str() {
            "claude" => self.claude(&msg.event, &msg.payload).await,
            "codex-notify" => {
                self.codex_notify(&msg.payload);
                None
            }
            _ => None,
        }
    }
}

/// A hub over a temp store, for other modules' tests.
#[cfg(test)]
pub mod testing {
    use super::*;
    use crate::agentwatch::screenshot::LookVerdict;
    use crate::runtime::ImageIn;

    pub struct NoPing;
    impl AgentPing for NoPing {
        fn waiting(&self, _p: &str, _s: &str, _id: i64, _n: usize) {}
    }
    pub struct NoLook;
    #[async_trait]
    impl ScreenshotChecker for NoLook {
        async fn check(&self, _a: &str, _q: &str, _i: &ImageIn) -> Option<LookVerdict> {
            None
        }
    }

    pub fn hub(auto: bool) -> (tempfile::TempDir, Arc<Store>, Arc<AgentWatch>) {
        let d = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&d.path().join("b.db")).unwrap());
        let w = AgentWatch::new(WatchDeps {
            store: store.clone(),
            rules: Arc::new(|| toml::from_str(crate::agentwatch::rules::DEFAULT_RULES).unwrap()),
            cfg: Arc::new(move || AgentsConfig { auto_approve: auto, screenshot_check: true, codex_watch: true }),
            paused: Arc::new(AtomicBool::new(false)),
            checker: Arc::new(NoLook),
            ping: Arc::new(NoPing),
            emit: Arc::new(|| {}),
            now: Arc::new(chrono::Utc::now),
        });
        (d, store, w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agentwatch::screenshot::LookVerdict;
    use crate::runtime::ImageIn;
    use crate::store::tests::utc;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Pings(Mutex<Vec<(String, String, usize)>>);
    impl AgentPing for Pings {
        fn waiting(&self, project: &str, summary: &str, _ask_id: i64, waiting_now: usize) {
            self.0.lock().unwrap().push((project.into(), summary.into(), waiting_now));
        }
    }

    struct Checker {
        answer: Option<LookVerdict>,
        asked: Mutex<Vec<(String, String)>>,
    }
    #[async_trait]
    impl ScreenshotChecker for Checker {
        async fn check(&self, original_ask: &str, question: &str, _image: &ImageIn) -> Option<LookVerdict> {
            self.asked.lock().unwrap().push((original_ask.into(), question.into()));
            self.answer.clone()
        }
    }

    struct Rig {
        _d: tempfile::TempDir,
        store: Arc<Store>,
        pings: Arc<Pings>,
        checker: Arc<Checker>,
        paused: Arc<AtomicBool>,
        w: Arc<AgentWatch>,
    }

    fn rig(auto: bool, look: Option<LookVerdict>) -> Rig {
        let d = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&d.path().join("b.db")).unwrap());
        let pings = Arc::new(Pings::default());
        let checker = Arc::new(Checker { answer: look, asked: Default::default() });
        let paused = Arc::new(AtomicBool::new(false));
        let w = AgentWatch::new(WatchDeps {
            store: store.clone(),
            rules: Arc::new(|| toml::from_str(crate::agentwatch::rules::DEFAULT_RULES).unwrap()),
            cfg: Arc::new(move || AgentsConfig { auto_approve: auto, screenshot_check: true, codex_watch: false }),
            paused: paused.clone(),
            checker: checker.clone(),
            ping: pings.clone(),
            emit: Arc::new(|| {}),
            now: Arc::new(|| utc("2026-11-13T10:00:00Z")),
        });
        Rig { _d: d, store, pings, checker, paused, w }
    }

    fn fx(name: &str) -> Value {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agents/claude").join(name);
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    fn transcript(name: &str) -> String {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agents").join(name).display().to_string()
    }

    fn msg(event: &str, payload: Value) -> HookMsg {
        HookMsg { source: "claude".into(), event: event.into(), wait: true, payload }
    }

    fn bash(cmd: &str) -> HookMsg {
        let mut p = fx("permission_request_bash.json");
        p["tool_input"]["command"] = json!(cmd);
        msg("PermissionRequest", p)
    }

    const SID: &str = "claude:60adba83-07cb-42b3-8c37-d834b9fbe879";

    #[tokio::test]
    async fn auto_off_queues_even_safe_asks() {
        let r = rig(false, None);
        assert_eq!(r.w.handle(bash("npm test")).await, None);
        let q = r.store.queued_asks(SID).unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].checks, vec!["auto-approve is off".to_string()]);
        let s = r.store.agent_session(SID).unwrap().unwrap();
        assert_eq!((s.state, s.last_ask.as_deref(), s.project.as_str()), (AgentState::Waiting, Some("wants to run npm test"), "/Users/me/code/shop"));
        assert_eq!(r.pings.0.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn auto_on_allows_rule_match_and_logs_rule() {
        let r = rig(true, None);
        assert_eq!(r.w.handle(bash("npm test")).await, Some(claude::allow_output()));
        let a = r.store.agent_asks_recent(1).unwrap().remove(0);
        assert_eq!((a.decision, a.rule.as_deref(), a.summary.as_str()), (AskDecision::Auto, Some("npm test"), "wants to run npm test"));
        assert_eq!(a.payload["command"], "npm test");
        assert!(r.pings.0.lock().unwrap().is_empty());
        assert_eq!(r.store.agent_session(SID).unwrap().unwrap().state, AgentState::Running);
    }

    #[tokio::test]
    async fn paused_allows_nothing() {
        let r = rig(true, None);
        r.paused.store(true, Ordering::SeqCst);
        assert_eq!(r.w.handle(bash("npm test")).await, None);
        assert_eq!(r.store.queued_asks(SID).unwrap()[0].checks, vec!["bleep is paused".to_string()]);
    }

    #[tokio::test]
    async fn unsure_ask_queues_pings_and_returns_none() {
        let r = rig(true, None);
        assert_eq!(r.w.handle(bash("git push --force origin main")).await, None);
        let q = r.store.queued_asks(SID).unwrap();
        assert_eq!(q[0].checks, vec!["matches \"git push\"".to_string()]);
        assert_eq!(r.pings.0.lock().unwrap()[0], ("/Users/me/code/shop".to_string(), "wants to run git push --force origin main".to_string(), 1));
    }

    fn stop_with(transcript_name: &str, last: &str) -> HookMsg {
        let mut p = fx("stop.json");
        p["transcript_path"] = json!(transcript(transcript_name));
        p["last_assistant_message"] = json!(last);
        msg("Stop", p)
    }

    fn high() -> Option<LookVerdict> {
        Some(LookVerdict { matches: true, confidence: "high".into(), reason: "The button is blue and full width.".into() })
    }

    #[tokio::test]
    async fn stop_with_matching_screenshot_goes_on() {
        let r = rig(true, high());
        let out = r.w.handle(stop_with("transcript_screenshot.jsonl", "I made the button blue and full width. Does this look right?")).await;
        assert_eq!(out, Some(claude::go_on_output()));
        assert!(!out.unwrap().to_string().contains("full width."), "model text stays out of the agent");
        assert_eq!(r.checker.asked.lock().unwrap()[0].0, "Make the checkout button blue and full width, then show me a screenshot.");
        let a = r.store.agent_asks_recent(1).unwrap().remove(0);
        assert_eq!((a.kind, a.decision, a.rule.as_deref()), (AskKind::Question, AskDecision::Auto, Some(SCREENSHOT_RULE)));
        assert_eq!(a.payload["reason"], "The button is blue and full width.");
        assert_eq!(r.store.go_ons_since(SID, utc("2026-11-13T09:00:00Z")).unwrap(), 1);
        assert_eq!(r.store.agent_session(SID).unwrap().unwrap().state, AgentState::Running);
    }

    #[tokio::test]
    async fn low_confidence_screenshot_waits_for_me() {
        let r = rig(true, Some(LookVerdict { matches: true, confidence: "low".into(), reason: "Hard to tell.".into() }));
        let out = r.w.handle(stop_with("transcript_screenshot.jsonl", "I made the button blue and full width. Does this look right?")).await;
        assert_eq!(out, None);
        let q = r.store.queued_asks(SID).unwrap();
        assert_eq!((q[0].kind, q[0].summary.as_str()), (AskKind::Question, "Does this look right?"));
        assert_eq!(r.store.agent_session(SID).unwrap().unwrap().state, AgentState::Waiting);
    }

    #[tokio::test]
    async fn screenshot_go_on_needs_auto_approve() {
        let r = rig(false, high());
        assert_eq!(r.w.handle(stop_with("transcript_screenshot.jsonl", "Does this look right?")).await, None);
        assert!(r.checker.asked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn stop_without_look_question_marks_done() {
        let r = rig(true, high());
        assert_eq!(r.w.handle(stop_with("transcript_plain.jsonl", "All tests pass now.")).await, None);
        assert_eq!(r.store.agent_session(SID).unwrap().unwrap().state, AgentState::Done);
        assert!(r.checker.asked.lock().unwrap().is_empty());
        assert!(r.pings.0.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn next_event_expires_queued_asks() {
        let r = rig(true, None);
        r.w.handle(bash("git push --force")).await;
        let id = r.store.queued_asks(SID).unwrap()[0].id;
        r.w.handle(msg("PostToolUse", fx("post_tool_use.json"))).await;
        assert_eq!(r.store.agent_ask(id).unwrap().unwrap().decision, AskDecision::Expired);
        assert_eq!(r.store.agent_session(SID).unwrap().unwrap().state, AgentState::Running);
    }

    #[tokio::test]
    async fn two_waiting_say_n_agents() {
        let r = rig(true, None);
        r.w.handle(bash("git push --force")).await;
        let mut other = bash("rm -rf dist");
        other.payload["session_id"] = json!("other");
        r.w.handle(other).await;
        assert_eq!(r.pings.0.lock().unwrap()[1].2, 2);
    }

    #[tokio::test]
    async fn prompt_sets_title_and_end_hides_session() {
        let r = rig(true, None);
        r.w.handle(msg("UserPromptSubmit", fx("user_prompt_submit.json"))).await;
        assert!(r.store.agent_session(SID).unwrap().unwrap().title.unwrap().starts_with("Run the shell command"));
        r.w.handle(msg("SessionEnd", fx("session_end.json"))).await;
        assert!(r.store.agent_sessions(utc("2026-11-13T10:00:00Z")).unwrap().is_empty());
    }

    #[tokio::test]
    async fn codex_notify_marks_turn_done() {
        let r = rig(true, None);
        let p: Value = serde_json::from_str(&std::fs::read_to_string(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agents/codex/notify_payload.json")).unwrap()).unwrap();
        r.w.handle(HookMsg { source: "codex-notify".into(), event: "agent-turn-complete".into(), wait: false, payload: p }).await;
        let s = r.store.agent_session("codex:thr_01a1").unwrap().unwrap();
        assert_eq!((s.tool, s.state, s.project.as_str()), (AgentTool::Codex, AgentState::Done, "/Users/me/code/shop"));
    }
}
