use super::*;
use crate::connectors::bleep::BleepConnector;
use crate::mcp::host::tests::FakeGate;
use crate::mcp::host::{Decision, ToolHost, TurnTracker};
use crate::mcp::McpServer;
use crate::runtime::contract;
use crate::runtime::McpEndpoint;
use std::collections::VecDeque;

/// Plays back one scripted reply per model call and keeps every request.
#[derive(Default)]
pub struct MockAdapter {
    pub replies: Mutex<VecDeque<Result<Vec<Result<ModelEvent, ApiError>>, ApiError>>>,
    pub requests: Mutex<Vec<ModelRequest>>,
    /// When the script runs out, answer every call with a tool call (a model that never stops).
    pub loop_tools: AtomicBool,
}

#[async_trait]
impl ModelAdapter for MockAdapter {
    async fn stream(&self, req: &ModelRequest) -> Result<ModelStream, ApiError> {
        self.requests.lock().unwrap().push(req.clone());
        let next = self.replies.lock().unwrap().pop_front();
        let events = match next {
            Some(r) => r?,
            None if self.loop_tools.load(Ordering::Relaxed) => tool_reply(&format!("c{}", self.requests.lock().unwrap().len())),
            None => text_reply("(no script)"),
        };
        Ok(Box::pin(futures::stream::iter(events)))
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ApiError> {
        Ok(vec![ModelInfo { id: "main-m".into(), name: None, window: Some(100_000), price: Some(Price { input: 1.0, output: 2.0, ..Default::default() }) }])
    }
}

fn usage(i: u64, o: u64) -> Result<ModelEvent, ApiError> {
    Ok(ModelEvent::Usage(Tokens { input: i, output: o, ..Default::default() }))
}

pub fn text_reply(t: &str) -> Vec<Result<ModelEvent, ApiError>> {
    vec![Ok(ModelEvent::TextDelta(t.into())), usage(100, 10), Ok(ModelEvent::Stop(StopReason::EndTurn))]
}

fn tool_reply(id: &str) -> Vec<Result<ModelEvent, ApiError>> {
    vec![Ok(ModelEvent::ToolCall { id: id.into(), name: "bleep_now".into(), args: json!({}) }), usage(50, 5), Ok(ModelEvent::Stop(StopReason::ToolUse))]
}

struct Rig {
    rt: ApiRuntime,
    mock: Arc<MockAdapter>,
    store: Arc<Store>,
    mcp: McpEndpoint,
    _srv: McpServer,
    _d: tempfile::TempDir,
}

fn choice() -> ApiChoice {
    ApiChoice { provider: "openrouter".into(), label: "OpenRouter".into(), main: "main-m".into(), small: "small-m".into(), has_key: true, free: false }
}

async fn rig(replies: Vec<Result<Vec<Result<ModelEvent, ApiError>>, ApiError>>) -> Rig {
    let d = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(&d.path().join("b.db")).unwrap());
    let host = ToolHost::new(vec![Arc::new(BleepConnector)], FakeGate::new(Decision::Approved), TurnTracker::default());
    let srv = McpServer::start("tok".into(), host).await.unwrap();
    let mock = Arc::new(MockAdapter::default());
    mock.replies.lock().unwrap().extend(replies);
    let rt = ApiRuntime::new(Core::new(choice(), mock.clone(), store.clone()).with_retry_wait(Duration::from_millis(1)));
    let mcp = McpEndpoint { url: srv.url(), token: "tok".into() };
    Rig { rt, mock, store, mcp, _srv: srv, _d: d }
}

fn cfg(r: &Rig, resume: Option<String>) -> SessionConfig {
    SessionConfig { system_prompt: "Be bleep.".into(), mcp: r.mcp.clone(), tier: Tier::Main, resume_id: resume }
}

async fn turn(r: &Rig, resume: Option<String>, text: &str) -> Vec<AgentEvent> {
    let h = r.rt.start_session(cfg(r, resume)).await.unwrap();
    r.rt.send(&h, UserMessage { text: text.into() }).await.unwrap().collect().await
}

fn conv_of(ev: &[AgentEvent]) -> String {
    match ev.last() {
        Some(AgentEvent::Done { runtime_session_id: Some(id) }) => id.clone(),
        e => panic!("{e:?}"),
    }
}

#[tokio::test]
async fn a_text_turn_meets_the_contract_and_is_saved() {
    let r = rig(vec![Ok(text_reply("Hi there."))]).await;
    r.rt.load_models().await.unwrap();
    let ev = turn(&r, None, "hello").await;
    contract::assert_text_turn(&ev, "Hi there.");
    assert!(ev.contains(&AgentEvent::Context { used: 100, window: Some(100_000) }));
    let saved = r.store.api_messages(&conv_of(&ev)).unwrap();
    assert_eq!(saved.len(), 2, "user and assistant");
    let req = &r.mock.requests.lock().unwrap()[0];
    assert_eq!((req.model.as_str(), req.system.as_str(), req.choice.clone()), ("main-m", "Be bleep.", ToolChoice::Auto));
    assert!(req.tools.iter().any(|t| t.name == "bleep_now"), "bleep's own tools are offered");
    let spend = r.store.spend_on(&chrono::Local::now().date_naive().to_string()).unwrap();
    assert!((spend.cost - (100.0 * 1.0 + 10.0 * 2.0) / 1e6).abs() < 1e-12, "{spend:?}");
}

#[tokio::test]
async fn a_tool_turn_runs_the_tool_through_bleeps_server() {
    let r = rig(vec![Ok(tool_reply("t1")), Ok(text_reply("It is noon."))]).await;
    let ev = turn(&r, None, "time?").await;
    contract::assert_tool_turn(&ev, "bleep_now");
    let reqs = r.mock.requests.lock().unwrap();
    let last = reqs[1].msgs.last().unwrap();
    assert!(matches!(&last.parts[0], Part::ToolResult { id, ok: true, text } if id == "t1" && !text.is_empty()), "{last:?}");
    assert!(ev.contains(&AgentEvent::Usage { input: 150, output: 15 }), "usage of both calls");
}

#[tokio::test]
async fn the_next_turn_resumes_the_saved_history() {
    let r = rig(vec![Ok(text_reply("One.")), Ok(text_reply("Two."))]).await;
    let conv = conv_of(&turn(&r, None, "first").await);
    let ev = turn(&r, Some(conv.clone()), "second").await;
    assert_eq!(conv_of(&ev), conv);
    let msgs = &r.mock.requests.lock().unwrap()[1].msgs;
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[2], Msg::user_text("second"));
}

#[tokio::test]
async fn an_unknown_resume_id_is_stale() {
    let r = rig(vec![]).await;
    let err = r.rt.start_session(cfg(&r, Some("api-gone".into()))).await.unwrap_err();
    assert!(err.downcast_ref::<StaleResume>().is_some());
}

#[tokio::test]
async fn a_rate_limit_ends_the_turn_with_a_reset_time() {
    let r = rig(vec![Err(ApiError::RateLimited { until: 1_900_000_000 })]).await;
    let ev = turn(&r, None, "hi").await;
    contract::assert_rate_limited(&ev);
    assert!(ev.iter().any(|e| matches!(e, AgentEvent::Error { message, retryable: true } if message.contains("rate limit"))));
    assert!(matches!(ev.last(), Some(AgentEvent::Done { runtime_session_id: Some(_) })));
}

#[tokio::test]
async fn a_rejected_key_shows_in_health() {
    let r = rig(vec![Err(ApiError::Auth("bad".into()))]).await;
    assert!(r.rt.health().await.ok());
    let ev = turn(&r, None, "hi").await;
    assert!(ev.iter().any(|e| matches!(e, AgentEvent::Error { message, retryable: false } if message.starts_with(KEY_REJECTED))));
    let h = r.rt.health().await;
    assert!(!h.ok() && h.hint.as_deref() == Some(KEY_REJECTED));
}

#[tokio::test]
async fn a_failed_start_is_tried_once_more() {
    let r = rig(vec![Err(ApiError::Retryable("503".into())), Ok(text_reply("Back."))]).await;
    contract::assert_text_turn(&turn(&r, None, "hi").await, "Back.");
}

#[tokio::test]
async fn a_model_that_never_stops_calling_tools_is_cut_off() {
    let r = rig(vec![]).await;
    r.mock.loop_tools.store(true, Ordering::Relaxed);
    let ev = turn(&r, None, "loop").await;
    let reqs = r.mock.requests.lock().unwrap();
    assert_eq!(reqs.len(), MAX_STEPS + 1);
    let starts = ev.iter().filter(|e| matches!(e, AgentEvent::ToolStart { .. })).count();
    assert_eq!(starts, MAX_STEPS, "tools the model asks for on the last call are not run");
    assert_eq!(reqs[MAX_STEPS].choice, ToolChoice::None, "the last call may not use tools");
    assert!(matches!(ev.last(), Some(AgentEvent::Done { .. })));
}

#[tokio::test]
async fn an_error_mid_reply_keeps_the_text_and_the_history_stays_valid() {
    let cut = vec![Ok(ModelEvent::TextDelta("Half".into())), Ok(ModelEvent::ToolCall { id: "x".into(), name: "bleep_now".into(), args: json!({}) }), Err(ApiError::Retryable("broke".into()))];
    let r = rig(vec![Ok(cut), Ok(text_reply("Fine."))]).await;
    let ev = turn(&r, None, "hi").await;
    assert!(ev.iter().any(|e| matches!(e, AgentEvent::Error { .. })));
    turn(&r, Some(conv_of(&ev)), "again").await;
    let msgs = &r.mock.requests.lock().unwrap()[1].msgs;
    assert_eq!(msgs[1], Msg { role: Role::Assistant, parts: vec![Part::Text { text: "Half".into() }] }, "no half-made tool call");
    assert_eq!(msgs.len(), 3);
}

#[tokio::test]
async fn a_stopped_turn_keeps_the_results_of_tools_that_ran() {
    let two = vec![
        Ok(ModelEvent::ToolCall { id: "t1".into(), name: "bleep_now".into(), args: json!({}) }),
        Ok(ModelEvent::ToolCall { id: "t2".into(), name: "bleep_now".into(), args: json!({}) }),
        usage(10, 2),
        Ok(ModelEvent::Stop(StopReason::ToolUse)),
    ];
    let r = rig(vec![Ok(two)]).await;
    let h = r.rt.start_session(cfg(&r, None)).await.unwrap();
    let conv = h.resume_id.clone().unwrap();
    let mut s = r.rt.send(&h, UserMessage { text: "go".into() }).await.unwrap();
    while let Some(e) = s.next().await {
        if matches!(e, AgentEvent::ToolResult { .. }) {
            break;
        }
    }
    drop(s);
    let mut last = None;
    for _ in 0..200 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        let msgs = r.store.api_messages(&conv).unwrap();
        last = msgs.last().and_then(|m| serde_json::from_str::<Msg>(m).ok());
        if last.as_ref().is_some_and(|m| m.role == Role::User && m.parts.len() == 2) {
            break;
        }
    }
    let last = last.unwrap();
    assert!(matches!(&last.parts[0], Part::ToolResult { id, ok: true, .. } if id == "t1"), "the tool that ran keeps its real result: {last:?}");
    assert!(matches!(&last.parts[1], Part::ToolResult { id, .. } if id == "t2"), "the other call has a result too (real, or not run if the stop came first)");
}

#[tokio::test]
async fn one_shot_forces_the_answer_tool_and_returns_its_input() {
    let reply = vec![Ok(ModelEvent::ToolCall { id: "a".into(), name: ANSWER_TOOL.into(), args: json!({"urgent": true}) }), usage(20, 4), Ok(ModelEvent::Stop(StopReason::ToolUse))];
    let r = rig(vec![Ok(reply)]).await;
    let img = crate::runtime::ImageIn { media_type: "image/png".into(), data_b64: "AAAA".into() };
    let v = r.rt.one_shot(OneShot { prompt: "Urgent?".into(), schema: json!({"type": "object", "properties": {"urgent": {"type": "boolean"}}}), tier: Tier::Small, images: vec![img] }).await.unwrap();
    assert_eq!(v, json!({"urgent": true}));
    let req = &r.mock.requests.lock().unwrap()[0];
    assert_eq!((req.model.as_str(), req.choice.clone()), ("small-m", ToolChoice::Force(ANSWER_TOOL.into())));
    assert!(matches!(req.msgs[0].parts[1], Part::Image { .. }));
    assert_eq!(r.store.spend_on(&chrono::Local::now().date_naive().to_string()).unwrap().calls, 1);
}

#[tokio::test]
async fn one_shot_reads_plain_json_text_from_a_model_that_ignores_tools() {
    let r = rig(vec![Ok(text_reply("```json\n{\"a\": 1}\n```"))]).await;
    let v = r.rt.one_shot(OneShot { prompt: "p".into(), schema: json!({}), tier: Tier::Small, images: vec![] }).await.unwrap();
    assert_eq!(v, json!({"a": 1}));
}

#[tokio::test]
async fn health_says_what_is_missing() {
    let r = rig(vec![]).await;
    let mut c = choice();
    c.has_key = false;
    let rt = ApiRuntime::new(Core::new(c.clone(), r.mock.clone(), r.store.clone()));
    assert!(rt.health().await.hint.unwrap().contains("Add your OpenRouter key"));
    c.has_key = true;
    c.small = String::new();
    let rt = ApiRuntime::new(Core::new(c, r.mock.clone(), r.store.clone()));
    assert!(rt.health().await.hint.unwrap().contains("Pick a main and a small model"));
}

#[test]
fn sanitize_fixes_roles_and_unanswered_calls() {
    let call = |id: &str| Part::ToolCall { id: id.into(), name: "n".into(), args: json!({}) };
    let res = |id: &str| Part::ToolResult { id: id.into(), ok: true, text: "r".into() };
    let msgs = vec![
        Msg { role: Role::Assistant, parts: vec![Part::Text { text: "orphan".into() }] },
        Msg::user_text("a"),
        Msg::user_text("b"),
        Msg { role: Role::Assistant, parts: vec![call("1"), call("2")] },
        Msg { role: Role::User, parts: vec![res("1")] },
        Msg::user_text("c"),
        Msg { role: Role::Assistant, parts: vec![call("3")] },
    ];
    let out = sanitize(msgs);
    let roles: Vec<_> = out.iter().map(|m| m.role).collect();
    assert_eq!(roles, [Role::User, Role::Assistant, Role::User, Role::Assistant, Role::User]);
    assert_eq!(out[0].parts.len(), 2, "two user messages merge");
    let ids: Vec<_> = out[2].parts.iter().filter_map(|p| if let Part::ToolResult { id, .. } = p { Some(id.as_str()) } else { None }).collect();
    assert_eq!(ids, ["2", "1"], "the missing result is added");
    assert!(matches!(out[2].parts.last(), Some(Part::Text { text }) if text == "c"), "results come before text");
    assert!(matches!(&out[4].parts[0], Part::ToolResult { id, ok: false, .. } if id == "3"));
}
