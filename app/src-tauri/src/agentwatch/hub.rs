//! The agent watch hub: every hook event and Codex ask comes here. It keeps session
//! state, runs the rules, records each ask, and pings me when an agent needs me.
use super::answer::{check_answer, vet_passes, AnswerMode, AnswerVerdict, AnswerVetter, MAX_ANSWERS_PER_HOUR, MAX_SAME_ASK_PER_HOUR};
use super::claude::{self, ClaudeEvent};
use super::rules::{check, AgentRules, Ask, RuleVerdict};
use super::screenshot::{may_go_on, wants_look, ScreenshotChecker};
use super::socket::HookHandler;
use super::transcript::{as_question, read_tail, NextStep, TAIL_BYTES};
use super::{AgentSession, AgentState, AgentTool, AskDecision, AskKind, HookMsg, NewAsk};
use crate::config::AgentsConfig;
use crate::proactive::scheduler::Clock;
use crate::store::Store;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::sync::oneshot;

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
    /// The small model that may stop an auto-answer.
    pub vetter: Arc<dyn AnswerVetter>,
    /// Tells the UI the agent list changed.
    pub emit: Arc<dyn Fn() + Send + Sync>,
    pub now: Clock,
}

pub struct AgentWatch {
    d: WatchDeps,
    /// This hub, for the auto-answer task that outlives a hook call.
    me: Weak<AgentWatch>,
    /// Auto-answers I stopped, by ask id.
    kept: Mutex<HashSet<i64>>,
    /// Claude Code permission hooks held open until I answer in bleep, by ask id. The
    /// terminal shows its own prompt meanwhile; the first answer wins.
    held: Mutex<HashMap<i64, oneshot::Sender<Option<Value>>>>,
    /// Background Stop hooks (`claude-reply`) waiting for my reply, by session id. They
    /// get the hook output to print.
    replies: Mutex<HashMap<String, oneshot::Sender<Option<Value>>>>,
}

/// What bleep does with a question in the next-step box.
#[derive(Clone, Debug, PartialEq)]
enum Plan {
    Send { label: String, rule: String },
    Shadow { label: String, rule: String },
    Keep { why: String },
}

impl Plan {
    fn json(&self) -> Value {
        match self {
            Plan::Send { label, rule } => json!({"mode": "on", "label": label, "rule": rule}),
            Plan::Shadow { label, rule } => json!({"mode": "shadow", "label": label, "rule": rule}),
            Plan::Keep { why } => json!({"mode": "keep", "why": why}),
        }
    }
}

/// What I can answer from bleep right now.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct Live {
    /// Ask ids whose hook waits for my answer.
    pub held: Vec<i64>,
    /// Session ids that take a reply.
    pub replyable: Vec<String>,
}

impl AgentWatch {
    pub fn new(d: WatchDeps) -> Arc<AgentWatch> {
        Arc::new_cyclic(|me| AgentWatch { d, me: me.clone(), kept: Mutex::default(), held: Mutex::default(), replies: Mutex::default() })
    }

    /// A hook that gave up (its own time ran out) leaves a closed slot: not live.
    pub fn live(&self) -> Live {
        let mut held: Vec<i64> = {
            let mut h = self.held.lock().unwrap();
            h.retain(|_, t| !t.is_closed());
            h.keys().copied().collect()
        };
        held.sort_unstable();
        let mut replyable: Vec<String> = {
            let mut r = self.replies.lock().unwrap();
            r.retain(|_, t| !t.is_closed());
            r.keys().cloned().collect()
        };
        replyable.sort();
        Live { held, replyable }
    }

    /// Holds a Claude Code hook until I answer in bleep. `None`: answered elsewhere.
    async fn hold(&self, ask_id: i64) -> Option<Value> {
        let (tx, rx) = oneshot::channel();
        self.held.lock().unwrap().insert(ask_id, tx);
        (self.d.emit)();
        let out = rx.await.ok().flatten();
        self.held.lock().unwrap().remove(&ask_id);
        out
    }

    /// Sends my answer to a held hook and records it.
    fn release(&self, ask_id: i64, out: Value, decision: AskDecision) -> Result<(), String> {
        let gone = || "This ask is no longer waiting. It was answered somewhere else.".to_string();
        let tx = self.held.lock().unwrap().remove(&ask_id).ok_or_else(gone)?;
        tx.send(Some(out)).map_err(|_| gone())?;
        let _ = self.d.store.set_ask_decision(ask_id, decision, None);
        if let Some(a) = self.d.store.agent_ask(ask_id).ok().flatten() {
            self.mark_running(&a.session_id);
        }
        Ok(())
    }

    fn mark_running(&self, session_id: &str) {
        if let Some(mut s) = self.d.store.agent_session(session_id).ok().flatten() {
            s.state = AgentState::Running;
            s.last_ask = None;
            s.updated = (self.d.now)();
            self.save(&s);
        }
    }

    /// My Approve or Deny on a held Claude Code permission ask.
    pub fn decide_held(&self, ask_id: i64, approve: bool) -> Result<(), String> {
        let out = if approve { claude::allow_output() } else { claude::deny_output() };
        self.release(ask_id, out, if approve { AskDecision::Approved } else { AskDecision::Denied })
    }

    /// My picks for an AskUserQuestion call: question text to label(s).
    pub fn choose(&self, ask_id: i64, answers: serde_json::Map<String, Value>) -> Result<(), String> {
        let ask = self.d.store.agent_ask(ask_id).map_err(|e| e.to_string())?.ok_or("No such ask.")?;
        let questions = ask.payload.get("questions").cloned().unwrap_or(Value::Null);
        let known = claude::question_texts(&questions);
        let mut clean = serde_json::Map::new();
        for (q, a) in answers {
            let Some(a) = a.as_str().map(str::trim).filter(|a| !a.is_empty()) else { continue };
            if !known.contains(&q) {
                return Err("That answer is for a question this ask does not have.".into());
            }
            clean.insert(q, json!(a));
        }
        if clean.is_empty() {
            return Err("Pick an answer first.".into());
        }
        self.release(ask_id, claude::answers_output(&questions, &clean), AskDecision::Answered)
    }

    /// My reply to a session that stopped, sent through its waiting `claude-reply` hook.
    pub fn reply(&self, session_id: &str, text: &str) -> Result<(), String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("Type a reply first.".into());
        }
        let tx = self.replies.lock().unwrap().remove(session_id).ok_or("This session does not take a reply now. Answer in the terminal.")?;
        tx.send(Some(claude::reply_output(text))).map_err(|_| "This session does not take a reply now. Answer in the terminal.".to_string())?;
        for a in self.d.store.queued_asks(session_id).unwrap_or_default() {
            let _ = self.d.store.merge_ask_payload(a.id, &json!({"user_reply": claude::cut(text, 300)}));
            let _ = self.d.store.set_ask_decision(a.id, AskDecision::Answered, None);
        }
        self.mark_running(session_id);
        Ok(())
    }

    /// A `claude-reply` hook: waits until I reply, or until the session moves on.
    async fn wait_reply(&self, p: &Value) -> Option<Value> {
        let sid = format!("claude:{}", p.get("session_id")?.as_str()?);
        let (tx, rx) = oneshot::channel();
        if let Some(old) = self.replies.lock().unwrap().insert(sid.clone(), tx) {
            let _ = old.send(None);
        }
        (self.d.emit)();
        let out = rx.await.ok().flatten();
        if out.is_none() {
            // Dropped or replaced: clear only our own (closed) slot.
            let mut r = self.replies.lock().unwrap();
            if r.get(&sid).is_some_and(|t| t.is_closed()) {
                r.remove(&sid);
            }
        }
        out
    }

    /// My "keep this one for me" on a pending auto-answer.
    pub fn keep(&self, ask_id: i64) -> Result<(), String> {
        let a = self.d.store.agent_ask(ask_id).map_err(|e| e.to_string())?.ok_or("No such ask.")?;
        if a.decision != AskDecision::Queued {
            return Err("This ask is no longer waiting.".into());
        }
        self.kept.lock().unwrap().insert(ask_id);
        let _ = self.d.store.merge_ask_payload(ask_id, &json!({"kept": true}));
        (self.d.emit)();
        Ok(())
    }

    /// The session moved on without bleep: its reply hook gives up.
    fn drop_reply(&self, session_id: &str) {
        if let Some(tx) = self.replies.lock().unwrap().remove(session_id) {
            let _ = tx.send(None);
        }
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
            if let Some(tx) = self.held.lock().unwrap().remove(&a.id) {
                let _ = tx.send(None);
            }
        }
    }

    /// Queues an agent's question for me. `extra` adds payload keys; `ping` replaces the
    /// ping text.
    fn queue_question(&self, s: &mut AgentSession, question: &str, extra: Value, ping: Option<&str>) -> i64 {
        let summary = claude::question_summary(question);
        let mut payload = json!({"question": question});
        if let (Some(o), Some(e)) = (payload.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        let new = NewAsk { session_id: s.id.clone(), kind: AskKind::Question, summary: summary.clone(), payload, checks: vec![], decision: AskDecision::Queued, rule: None, rpc_id: None };
        let id = self.d.store.insert_agent_ask(&new).unwrap_or(0);
        s.state = AgentState::Waiting;
        s.last_ask = Some(summary.clone());
        self.save(s);
        self.d.ping.waiting(&s.project, ping.unwrap_or(&summary), id, self.waiting_now());
        id
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
                // What I typed instead: the record that shows whether auto-answer would match me.
                if let Some(typed) = p.get("prompt").and_then(Value::as_str).filter(|_| event == "UserPromptSubmit") {
                    for a in self.d.store.queued_asks(&s.id).unwrap_or_default() {
                        let _ = self.d.store.merge_ask_payload(a.id, &json!({"user_reply": claude::cut(typed, 300)}));
                    }
                }
                self.expire_queued(&s.id);
                self.drop_reply(&s.id);
                if event == "UserPromptSubmit" && s.title.is_none() {
                    s.title = p.get("prompt").and_then(Value::as_str).map(|t| claude::cut(t, 60));
                }
                s.state = AgentState::Running;
                s.last_ask = None;
                self.save(&s);
                None
            }
            ClaudeEvent::Permission(ask) if ask.tool_name == claude::ASK_USER_QUESTION => self.ask_user(s, p).await,
            ClaudeEvent::Permission(ask) => {
                let (id, d) = self.decide(&s, ask, None).await;
                if d == AskDecision::Auto {
                    return Some(claude::allow_output());
                }
                // The terminal asks too. Whichever of us answers first wins.
                self.hold(id).await
            }
            ClaudeEvent::Notify { idle } => {
                if self.d.store.queued_asks(&s.id).unwrap_or_default().is_empty() {
                    let q = if idle { "is waiting for you?" } else { "needs your permission?" };
                    self.queue_question(&mut s, q, json!({}), None);
                } else {
                    s.state = AgentState::Waiting;
                    self.save(&s);
                }
                None
            }
            ClaudeEvent::Stop { active } => self.stop(s, p, active).await,
            ClaudeEvent::End => {
                self.expire_queued(&s.id);
                self.drop_reply(&s.id);
                self.save(&s);
                let _ = self.d.store.end_agent_session(&s.id);
                (self.d.emit)();
                None
            }
        }
    }

    /// Claude Code asks me questions with options. Rules never answer these: only I do.
    async fn ask_user(&self, mut s: AgentSession, p: &Value) -> Option<Value> {
        let questions = p.pointer("/tool_input/questions").cloned().unwrap_or(json!([]));
        let first = claude::question_texts(&questions).into_iter().next().unwrap_or_else(|| "has a question for you".into());
        let summary = claude::cut(&first, 80);
        let cwd = p.get("cwd").and_then(Value::as_str).unwrap_or("");
        let new = NewAsk { session_id: s.id.clone(), kind: AskKind::Question, summary: summary.clone(), payload: json!({"tool_name": claude::ASK_USER_QUESTION, "questions": questions, "cwd": cwd}), checks: vec![], decision: AskDecision::Queued, rule: None, rpc_id: None };
        let id = self.d.store.insert_agent_ask(&new).unwrap_or(0);
        s.state = AgentState::Waiting;
        s.last_ask = Some(summary.clone());
        self.save(&s);
        self.d.ping.waiting(&s.project, &summary, id, self.waiting_now());
        self.hold(id).await
    }

    async fn stop(&self, mut s: AgentSession, p: &Value, active: bool) -> Option<Value> {
        // The turn ended: any permission it waited on was answered in the terminal.
        self.expire_queued(&s.id);
        let mut tail = s.transcript_path.as_deref().map(|t| read_tail(std::path::Path::new(t), TAIL_BYTES)).unwrap_or_default();
        if let Some(last) = p.get("last_assistant_message").and_then(Value::as_str) {
            tail.last_question = as_question(last);
        }
        // In the desktop app the question sits in the next-step box, not in the last words.
        if let Some(step) = &tail.next_step {
            tail.last_question = Some(step.ask.clone());
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
        let step = tail.next_step.clone();
        let plan = step.as_ref().map(|st| self.plan_answer(&s, st, &question));
        let delay = (self.d.cfg)().answer_delay_secs;
        let mut answer = plan.as_ref().map(Plan::json);
        if let Some(a) = answer.as_mut().and_then(Value::as_object_mut) {
            a.insert("delay_secs".into(), json!(delay));
        }
        let extra = json!({"labels": step.as_ref().map(|x| x.labels.clone()).unwrap_or_default(), "answer": answer});
        let ping = match &plan {
            Some(Plan::Send { label, .. }) => {
                let q = claude::question_summary(&question);
                Some(claude::cut(&match delay {
                    0 => format!("answering \"{label}\" for you: {q}"),
                    n => format!("answers \"{label}\" in {n} s unless you stop it: {q}"),
                }, 120))
            }
            _ => None,
        };
        let id = self.queue_question(&mut s, &question, extra, ping.as_deref());
        let (send, label, rule) = match plan {
            Some(Plan::Send { label, rule }) => (true, label, rule),
            Some(Plan::Shadow { label, rule }) => (false, label, rule),
            _ => return None,
        };
        if let Some(me) = self.me.upgrade() {
            let original = tail.original_ask.unwrap_or_default();
            tokio::spawn(me.answer_later(id, s, original, question, label, rule, send, Duration::from_secs(delay)));
        }
        None
    }

    /// Rules decide whether bleep may answer for me (D12); the loop guard and the mode come after.
    fn plan_answer(&self, s: &AgentSession, step: &NextStep, question: &str) -> Plan {
        let keep = |why: &str| Plan::Keep { why: why.into() };
        let mode = (self.d.cfg)().auto_answer;
        if mode == AnswerMode::Off {
            return keep("auto-answer is off");
        }
        let (label, rule) = match check_answer(&(self.d.rules)(), step) {
            AnswerVerdict::Answer { label, rule } => (label, rule),
            AnswerVerdict::NeedsYou { why } => return Plan::Keep { why },
        };
        let hour_ago = (self.d.now)() - chrono::Duration::hours(1);
        if self.d.store.go_ons_since(&s.id, hour_ago).unwrap_or(usize::MAX) >= MAX_ANSWERS_PER_HOUR {
            return keep("bleep already answered this session 5 times this hour");
        }
        if self.d.store.asks_like_since(&s.id, &claude::question_summary(question), hour_ago).unwrap_or(usize::MAX) >= MAX_SAME_ASK_PER_HOUR {
            return keep("the agent asked this before: it may be in a loop");
        }
        if mode == AnswerMode::Shadow {
            return Plan::Shadow { label, rule };
        }
        if self.d.paused.load(Ordering::SeqCst) {
            return keep("bleep is paused");
        }
        Plan::Send { label, rule }
    }

    /// The model check, then (mode on) the grace window, then the agent's own first button
    /// goes through the session's waiting reply hook. Anything that changed meanwhile wins:
    /// my reply, my stop, or the session moving on.
    #[allow(clippy::too_many_arguments)]
    async fn answer_later(self: Arc<Self>, ask_id: i64, s: AgentSession, original: String, question: String, label: String, rule: String, send: bool, delay: Duration) {
        let vet = self.d.vetter.vet(&original, &question, &label);
        let v = if send { tokio::join!(vet, tokio::time::sleep(delay)).0 } else { vet.await };
        let ok = vet_passes(v.as_ref());
        let _ = self.d.store.merge_ask_payload(ask_id, &json!({"vet": v, "vet_ok": ok, "asked": claude::cut(&original, 200)}));
        let still_waiting = || self.d.store.agent_ask(ask_id).ok().flatten().is_some_and(|a| a.decision == AskDecision::Queued);
        let not_sent = |why: &str| {
            let _ = self.d.store.merge_ask_payload(ask_id, &json!({"not_sent": why}));
            if still_waiting() {
                let summary = claude::question_summary(&question);
                self.d.ping.waiting(&s.project, &claude::cut(&format!("needs you after all: {summary}"), 120), ask_id, self.waiting_now());
            }
            (self.d.emit)();
        };
        if !send {
            (self.d.emit)();
            return;
        }
        if self.kept.lock().unwrap().remove(&ask_id) {
            return (self.d.emit)();
        }
        if !still_waiting() {
            return;
        }
        if !ok {
            return not_sent("the model check did not say a confident yes");
        }
        let Some(tx) = self.replies.lock().unwrap().remove(&s.id) else { return not_sent("the session takes no reply") };
        if tx.send(Some(claude::auto_reply_output(&label))).is_err() {
            return not_sent("the session takes no reply");
        }
        let _ = self.d.store.set_ask_decision(ask_id, AskDecision::Auto, Some(&rule));
        let _ = self.d.store.push_go_on(&s.id, (self.d.now)());
        self.mark_running(&s.id);
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
            Some(q) => {
                self.queue_question(&mut s, q, json!({}), None);
            }
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
            "claude-reply" => self.wait_reply(&msg.payload).await,
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
    pub struct NoVet;
    #[async_trait]
    impl AnswerVetter for NoVet {
        async fn vet(&self, _a: &str, _q: &str, _l: &str) -> Option<crate::agentwatch::answer::Vet> {
            None
        }
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
            cfg: Arc::new(move || AgentsConfig { auto_approve: auto, screenshot_check: true, codex_watch: true, auto_answer: AnswerMode::Off, answer_delay_secs: 0 }),
            paused: Arc::new(AtomicBool::new(false)),
            checker: Arc::new(NoLook),
            ping: Arc::new(NoPing),
            vetter: Arc::new(NoVet),
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

    struct Vetter {
        answer: Option<crate::agentwatch::answer::Vet>,
        asked: Mutex<Vec<(String, String, String)>>,
    }
    #[async_trait]
    impl AnswerVetter for Vetter {
        async fn vet(&self, original_ask: &str, question: &str, label: &str) -> Option<crate::agentwatch::answer::Vet> {
            self.asked.lock().unwrap().push((original_ask.into(), question.into(), label.into()));
            self.answer.clone()
        }
    }

    struct Rig {
        _d: tempfile::TempDir,
        store: Arc<Store>,
        pings: Arc<Pings>,
        checker: Arc<Checker>,
        paused: Arc<AtomicBool>,
        vetter: Arc<Vetter>,
        w: Arc<AgentWatch>,
    }

    fn rig(auto: bool, look: Option<LookVerdict>) -> Rig {
        rig_with(auto, look, AnswerMode::Off, None)
    }

    fn rig_with(auto: bool, look: Option<LookVerdict>, mode: AnswerMode, vet: Option<crate::agentwatch::answer::Vet>) -> Rig {
        rig_delay(auto, look, mode, vet, 0)
    }

    fn rig_delay(auto: bool, look: Option<LookVerdict>, mode: AnswerMode, vet: Option<crate::agentwatch::answer::Vet>, delay: u64) -> Rig {
        let d = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&d.path().join("b.db")).unwrap());
        let pings = Arc::new(Pings::default());
        let checker = Arc::new(Checker { answer: look, asked: Default::default() });
        let paused = Arc::new(AtomicBool::new(false));
        let vetter = Arc::new(Vetter { answer: vet, asked: Default::default() });
        let w = AgentWatch::new(WatchDeps {
            store: store.clone(),
            rules: Arc::new(|| toml::from_str(crate::agentwatch::rules::DEFAULT_RULES).unwrap()),
            cfg: Arc::new(move || AgentsConfig { auto_approve: auto, screenshot_check: true, codex_watch: false, auto_answer: mode, answer_delay_secs: delay }),
            paused: paused.clone(),
            checker: checker.clone(),
            ping: pings.clone(),
            vetter: vetter.clone(),
            emit: Arc::new(|| {}),
            now: Arc::new(|| utc("2026-11-13T10:00:00Z")),
        });
        Rig { _d: d, store, pings, checker, paused, vetter, w }
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

    /// Sends a hook message that waits for me; returns once `n` hooks are held.
    async fn held(r: &Rig, m: HookMsg, n: usize) -> tokio::task::JoinHandle<Option<Value>> {
        let w = r.w.clone();
        let h = tokio::spawn(async move { w.handle(m).await });
        for _ in 0..500 {
            if r.w.live().held.len() >= n {
                return h;
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        panic!("hook was never held");
    }

    async fn reply_waiting(r: &Rig, m: HookMsg) -> tokio::task::JoinHandle<Option<Value>> {
        let w = r.w.clone();
        let h = tokio::spawn(async move { w.handle(m).await });
        for _ in 0..500 {
            if !r.w.live().replyable.is_empty() {
                return h;
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        panic!("reply hook never waited");
    }

    fn reply_msg() -> HookMsg {
        HookMsg { source: "claude-reply".into(), event: "Stop".into(), wait: true, payload: fx("stop.json") }
    }

    fn ask_user_msg() -> HookMsg {
        let mut p = fx("permission_request_bash.json");
        p["tool_name"] = json!("AskUserQuestion");
        p["tool_input"] = json!({"questions": [{"question": "Which DB?", "header": "DB", "options": [{"label": "Neon", "description": ""}, {"label": "SQLite", "description": ""}], "multiSelect": false}]});
        msg("PermissionRequest", p)
    }

    #[tokio::test]
    async fn auto_off_queues_even_safe_asks() {
        let r = rig(false, None);
        let _h = held(&r, bash("npm test"), 1).await;
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
        let _h = held(&r, bash("npm test"), 1).await;
        assert_eq!(r.store.queued_asks(SID).unwrap()[0].checks, vec!["bleep is paused".to_string()]);
    }

    #[tokio::test]
    async fn unsure_ask_queues_pings_and_waits_for_me() {
        let r = rig(true, None);
        let h = held(&r, bash("git push --force origin main"), 1).await;
        let q = r.store.queued_asks(SID).unwrap();
        assert_eq!(q[0].checks, vec!["matches \"git push\"".to_string()]);
        assert_eq!(r.pings.0.lock().unwrap()[0], ("/Users/me/code/shop".to_string(), "wants to run git push --force origin main".to_string(), 1));
        assert_eq!(r.w.live().held, vec![q[0].id]);
        assert!(!h.is_finished(), "the hook waits for my answer");
        h.abort();
    }

    #[tokio::test]
    async fn approve_in_bleep_releases_the_hook() {
        let r = rig(true, None);
        let h = held(&r, bash("git push --force"), 1).await;
        let id = r.store.queued_asks(SID).unwrap()[0].id;
        r.w.decide_held(id, true).unwrap();
        assert_eq!(h.await.unwrap(), Some(claude::allow_output()));
        assert_eq!(r.store.agent_ask(id).unwrap().unwrap().decision, AskDecision::Approved);
        assert_eq!(r.store.agent_session(SID).unwrap().unwrap().state, AgentState::Running);
        assert!(r.w.live().held.is_empty());
        assert!(r.w.decide_held(id, false).is_err(), "only once");
    }

    #[tokio::test]
    async fn deny_in_bleep_says_no() {
        let r = rig(true, None);
        let h = held(&r, bash("git push --force"), 1).await;
        let id = r.store.queued_asks(SID).unwrap()[0].id;
        r.w.decide_held(id, false).unwrap();
        assert_eq!(h.await.unwrap(), Some(claude::deny_output()));
        assert_eq!(r.store.agent_ask(id).unwrap().unwrap().decision, AskDecision::Denied);
    }

    #[tokio::test]
    async fn ask_user_question_is_never_auto_and_takes_my_pick() {
        let r = rig(true, None);
        let h = held(&r, ask_user_msg(), 1).await;
        let a = r.store.queued_asks(SID).unwrap().remove(0);
        assert_eq!((a.kind, a.summary.as_str()), (AskKind::Question, "Which DB?"));
        assert_eq!(a.payload["questions"][0]["options"][1]["label"], "SQLite");
        let mut wrong = serde_json::Map::new();
        wrong.insert("Something else?".into(), json!("Neon"));
        assert!(r.w.choose(a.id, wrong).is_err());
        assert!(r.w.choose(a.id, serde_json::Map::new()).is_err());
        let mut pick = serde_json::Map::new();
        pick.insert("Which DB?".into(), json!("SQLite"));
        r.w.choose(a.id, pick.clone()).unwrap();
        assert_eq!(h.await.unwrap(), Some(claude::answers_output(&a.payload["questions"], &pick)));
        assert_eq!(r.store.agent_ask(a.id).unwrap().unwrap().decision, AskDecision::Answered);
    }

    #[tokio::test]
    async fn reply_wakes_the_session_with_my_words() {
        let r = rig(true, None);
        r.w.handle(stop_with("transcript_plain.jsonl", "Should I also update the README?")).await;
        let q = r.store.queued_asks(SID).unwrap();
        assert_eq!(q.len(), 1);
        let h = reply_waiting(&r, reply_msg()).await;
        assert_eq!(r.w.live().replyable, vec![SID.to_string()]);
        assert!(r.w.reply(SID, "   ").is_err());
        r.w.reply(SID, "Yes, and the changelog.").unwrap();
        assert_eq!(h.await.unwrap(), Some(claude::reply_output("Yes, and the changelog.")));
        assert_eq!(r.store.agent_ask(q[0].id).unwrap().unwrap().decision, AskDecision::Answered);
        assert_eq!(r.store.agent_session(SID).unwrap().unwrap().state, AgentState::Running);
        assert!(r.w.reply(SID, "again").is_err(), "the hook is used up");
    }

    #[tokio::test]
    async fn typing_in_the_terminal_lets_the_reply_hook_go() {
        let r = rig(true, None);
        let h = reply_waiting(&r, reply_msg()).await;
        r.w.handle(msg("UserPromptSubmit", fx("user_prompt_submit.json"))).await;
        assert_eq!(h.await.unwrap(), None);
        assert!(r.w.live().replyable.is_empty());
    }

    #[tokio::test]
    async fn a_new_reply_hook_replaces_the_old_one() {
        let r = rig(true, None);
        let first = reply_waiting(&r, reply_msg()).await;
        let w = r.w.clone();
        let second = tokio::spawn(async move { w.handle(reply_msg()).await });
        assert_eq!(first.await.unwrap(), None);
        for _ in 0..500 {
            if r.w.reply(SID, "ok").is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        assert_eq!(second.await.unwrap(), Some(claude::reply_output("ok")));
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
        let h = held(&r, bash("git push --force"), 1).await;
        let id = r.store.queued_asks(SID).unwrap()[0].id;
        r.w.handle(msg("PostToolUse", fx("post_tool_use.json"))).await;
        assert_eq!(r.store.agent_ask(id).unwrap().unwrap().decision, AskDecision::Expired);
        assert_eq!(h.await.unwrap(), None, "answered in the terminal: the hook lets go");
        assert!(r.w.decide_held(id, true).is_err());
        assert_eq!(r.store.agent_session(SID).unwrap().unwrap().state, AgentState::Running);
    }

    #[tokio::test]
    async fn two_waiting_say_n_agents() {
        let r = rig(true, None);
        let _a = held(&r, bash("git push --force"), 1).await;
        let mut other = bash("rm -rf dist");
        other.payload["session_id"] = json!("other");
        let _b = held(&r, other, 2).await;
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

    /// A desktop-app turn: my prompt, the next-step box, then closing words with no "?".
    fn box_stop(dir: &std::path::Path, ask: &str, labels: &[&str]) -> HookMsg {
        let path = dir.join("box.jsonl");
        let actions: Vec<Value> = labels.iter().map(|l| json!({"label": l})).collect();
        let rows = [
            json!({"type": "user", "message": {"content": "Open the PR and merge it when CI is green."}}),
            json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": crate::agentwatch::transcript::WRAP_UP_TOOL, "input": {"recap": "r", "ask": ask, "actions": actions}}]}}),
            json!({"type": "user", "message": {"content": [{"type": "tool_result", "content": "Shown."}]}}),
            json!({"type": "assistant", "message": {"content": [{"type": "text", "text": "PR 19 is open."}]}}),
        ];
        std::fs::write(&path, rows.iter().map(|r| r.to_string()).collect::<Vec<_>>().join("\n")).unwrap();
        let mut p = fx("stop.json");
        p["transcript_path"] = json!(path.display().to_string());
        p["last_assistant_message"] = json!("PR 19 is open.");
        msg("Stop", p)
    }

    fn sure() -> Option<crate::agentwatch::answer::Vet> {
        Some(crate::agentwatch::answer::Vet { ok: true, confidence: "high".into(), reason: "A go-ahead on the PR the user asked for.".into() })
    }

    async fn settle(r: &Rig, id: i64, key: &str) -> crate::store::AgentAsk {
        for _ in 0..500 {
            let a = r.store.agent_ask(id).unwrap().unwrap();
            if a.payload.get(key).is_some() || a.decision != AskDecision::Queued {
                return a;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("auto-answer never settled");
    }

    #[tokio::test]
    async fn next_step_box_waits_instead_of_done() {
        let r = rig(true, None);
        let d = tempfile::tempdir().unwrap();
        assert_eq!(r.w.handle(box_stop(d.path(), "Merge PR 19 once CI is green?", &["Merge it", "I'll test first"])).await, None);
        let s = r.store.agent_session(SID).unwrap().unwrap();
        assert_eq!((s.state, s.last_ask.as_deref()), (AgentState::Waiting, Some("Merge PR 19 once CI is green?")));
        let a = r.store.queued_asks(SID).unwrap().remove(0);
        assert_eq!(a.payload["labels"], json!(["Merge it", "I'll test first"]));
        assert_eq!((a.payload["answer"]["mode"].as_str(), a.payload["answer"]["why"].as_str()), (Some("keep"), Some("auto-answer is off")));
        assert!(r.vetter.asked.lock().unwrap().is_empty(), "off spends no model call");
    }

    #[tokio::test]
    async fn auto_answer_sends_the_first_button_right_away() {
        let r = rig_with(false, None, AnswerMode::On, sure());
        let d = tempfile::tempdir().unwrap();
        let reply = reply_waiting(&r, reply_msg()).await;
        r.w.handle(box_stop(d.path(), "Merge PR 19 once CI is green?", &["Merge it", "I'll test first"])).await;
        assert!(r.pings.0.lock().unwrap()[0].1.starts_with("answering \"Merge it\" for you"), "{:?}", r.pings.0.lock().unwrap());
        assert_eq!(reply.await.unwrap(), Some(claude::auto_reply_output("Merge it")));
        let a = r.store.agent_asks_recent(1).unwrap().remove(0);
        assert_eq!((a.decision, a.rule.as_deref()), (AskDecision::Auto, Some("answer: merge")));
        assert_eq!(r.vetter.asked.lock().unwrap()[0], ("Open the PR and merge it when CI is green.".into(), "Merge PR 19 once CI is green?".into(), "Merge it".into()));
        assert_eq!(r.store.agent_session(SID).unwrap().unwrap().state, AgentState::Running);
    }

    #[tokio::test]
    async fn unsure_model_keeps_it_for_me() {
        let r = rig_with(false, None, AnswerMode::On, Some(crate::agentwatch::answer::Vet { ok: true, confidence: "low".into(), reason: "?".into() }));
        let d = tempfile::tempdir().unwrap();
        let _reply = reply_waiting(&r, reply_msg()).await;
        r.w.handle(box_stop(d.path(), "Merge PR 19?", &["Merge it"])).await;
        let id = r.store.queued_asks(SID).unwrap()[0].id;
        let a = settle(&r, id, "not_sent").await;
        assert_eq!((a.decision, a.payload["vet_ok"].as_bool()), (AskDecision::Queued, Some(false)));
        assert!(r.pings.0.lock().unwrap().last().unwrap().1.starts_with("needs you after all"));
        assert_eq!(r.w.live().replyable, vec![SID.to_string()], "the reply hook still waits for me");
    }

    #[tokio::test]
    async fn risky_box_is_never_answered() {
        let r = rig_with(false, None, AnswerMode::On, sure());
        let d = tempfile::tempdir().unwrap();
        r.w.handle(box_stop(d.path(), "Ship the landing update to Vercel?", &["Ship it", "Not yet"])).await;
        let a = r.store.queued_asks(SID).unwrap().remove(0);
        assert_eq!((a.payload["answer"]["mode"].as_str(), a.payload["answer"]["why"].as_str()), (Some("keep"), Some("matches \"ship\"")));
        assert!(r.vetter.asked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn shadow_logs_and_my_reply_is_recorded() {
        let r = rig_with(false, None, AnswerMode::Shadow, sure());
        let d = tempfile::tempdir().unwrap();
        let reply = reply_waiting(&r, reply_msg()).await;
        r.w.handle(box_stop(d.path(), "Merge PR 19?", &["Merge it"])).await;
        let id = r.store.queued_asks(SID).unwrap()[0].id;
        let a = settle(&r, id, "vet_ok").await;
        assert_eq!((a.decision, a.payload["answer"]["mode"].as_str(), a.payload["vet_ok"].as_bool()), (AskDecision::Queued, Some("shadow"), Some(true)));
        // I type in the terminal: shadow sent nothing, and my words are kept next to its pick.
        let mut p = fx("user_prompt_submit.json");
        p["prompt"] = json!("Merge it");
        r.w.handle(msg("UserPromptSubmit", p)).await;
        let a = r.store.agent_ask(id).unwrap().unwrap();
        assert_eq!((a.decision, a.payload["user_reply"].as_str()), (AskDecision::Expired, Some("Merge it")));
        assert_eq!(reply.await.unwrap(), None);
    }

    #[tokio::test]
    async fn keep_stops_a_pending_answer() {
        // With a delay set, I have time to stop it.
        let r = rig_delay(false, None, AnswerMode::On, sure(), 1);
        let d = tempfile::tempdir().unwrap();
        let _reply = reply_waiting(&r, reply_msg()).await;
        r.w.handle(box_stop(d.path(), "Merge PR 19?", &["Merge it"])).await;
        let id = r.store.queued_asks(SID).unwrap()[0].id;
        assert_eq!(r.store.agent_ask(id).unwrap().unwrap().payload["answer"]["delay_secs"], 1);
        r.w.keep(id).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
        let a = r.store.agent_ask(id).unwrap().unwrap();
        assert_eq!((a.decision, a.payload["kept"].as_bool()), (AskDecision::Queued, Some(true)));
        assert_eq!(r.w.live().replyable, vec![SID.to_string()]);
    }

    #[tokio::test]
    async fn a_repeated_ask_goes_to_me() {
        let r = rig_with(false, None, AnswerMode::Shadow, sure());
        let d = tempfile::tempdir().unwrap();
        for _ in 0..3 {
            r.w.handle(box_stop(d.path(), "Check #46 again and merge it?", &["Check and merge"])).await;
        }
        let a = r.store.agent_asks_recent(1).unwrap().remove(0);
        assert_eq!(a.payload["answer"]["why"], "the agent asked this before: it may be in a loop");
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
