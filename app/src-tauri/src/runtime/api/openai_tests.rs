use super::*;
use crate::runtime::api::anthropic::tests::replay;
use serde_json::json;

fn fixture(name: &str) -> Vec<Result<ModelEvent, ApiError>> {
    let bytes = std::fs::read(format!("{}/tests/fixtures/api/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    replay(&bytes, Parser::default())
}

fn ok(v: Vec<Result<ModelEvent, ApiError>>) -> Vec<ModelEvent> {
    v.into_iter().map(|r| r.unwrap()).collect()
}

#[test]
fn a_text_turn_counts_cached_tokens_apart() {
    assert_eq!(
        ok(fixture("openai_text.sse")),
        [
            ModelEvent::TextDelta("Hello".into()),
            ModelEvent::TextDelta(" there.".into()),
            ModelEvent::Usage(Tokens { input: 12, cache_read: 2000, cache_write: 0, output: 9 }),
            ModelEvent::Stop(StopReason::EndTurn),
        ]
    );
}

#[test]
fn interleaved_tool_call_deltas_join_by_index() {
    let ev = ok(fixture("openai_tool.sse"));
    assert_eq!(ev[0], ModelEvent::TextDelta("Checking.".into()));
    assert_eq!(ev[1], ModelEvent::ToolCall { id: "call_a".into(), name: "bleep_now".into(), args: json!({"tz": "Asia/Kolkata"}) });
    assert_eq!(ev[2], ModelEvent::ToolCall { id: "call_b".into(), name: "calendar_list".into(), args: json!({}) });
    assert_eq!(ev[3], ModelEvent::Usage(Tokens { input: 40, output: 22, ..Default::default() }));
    assert_eq!(ev[4], ModelEvent::Stop(StopReason::ToolUse));
}

#[test]
fn ollama_tool_calls_without_ids_get_one() {
    let ev = ok(fixture("ollama_tool.sse"));
    assert_eq!(ev[0], ModelEvent::ToolCall { id: "call_0".into(), name: "bleep_now".into(), args: json!({}) });
    assert_eq!(ev.last(), Some(&ModelEvent::Stop(StopReason::ToolUse)));
}

#[test]
fn an_error_chunk_ends_the_stream() {
    let ev = fixture("openai_error.sse");
    assert_eq!(ev[0], Ok(ModelEvent::TextDelta("Hi".into())));
    assert!(matches!(&ev[1], Err(ApiError::Retryable(m)) if m.contains("Provider returned error")), "{ev:?}");
    assert_eq!(ev.len(), 2);
}

#[test]
fn stop_with_tool_calls_is_a_tool_stop() {
    let raw = b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"x\",\"function\":{\"name\":\"a\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
    assert_eq!(ok(replay(raw, Parser::default())).last(), Some(&ModelEvent::Stop(StopReason::ToolUse)));
}

fn request() -> ModelRequest {
    ModelRequest {
        model: "m".into(),
        system: "Be brief.".into(),
        msgs: vec![
            Msg::user_text("Time?"),
            Msg { role: Role::Assistant, parts: vec![Part::Text { text: "Let me look.".into() }, Part::ToolCall { id: "t1".into(), name: "bleep_now".into(), args: json!({"a": 1}) }, Part::ToolCall { id: "t2".into(), name: "b".into(), args: json!({}) }] },
            Msg { role: Role::User, parts: vec![Part::ToolResult { id: "t1".into(), ok: true, text: "noon".into() }, Part::ToolResult { id: "t2".into(), ok: false, text: "boom".into() }] },
            Msg { role: Role::User, parts: vec![Part::Text { text: "Look".into() }, Part::Image { media_type: "image/png".into(), data_b64: "AAAA".into() }] },
        ],
        tools: vec![ToolDef { name: "bleep_now".into(), description: "Now.".into(), schema: json!({"type": "object"}) }],
        choice: ToolChoice::Force("bleep_now".into()),
        max_tokens: 500,
    }
}

#[test]
fn the_body_puts_tool_results_in_tool_messages() {
    let b = body(&request(), false);
    let m = b["messages"].as_array().unwrap();
    assert_eq!(m[0], json!({"role": "system", "content": "Be brief."}));
    assert_eq!(m[1], json!({"role": "user", "content": "Time?"}));
    assert_eq!(m[2]["content"], "Let me look.");
    assert_eq!(m[2]["tool_calls"][0]["function"]["arguments"], r#"{"a":1}"#, "arguments are a JSON string");
    assert_eq!(m[3], json!({"role": "tool", "tool_call_id": "t1", "content": "noon"}));
    assert_eq!(m[4], json!({"role": "tool", "tool_call_id": "t2", "content": "Error: boom"}));
    assert_eq!(m[5]["content"][1], json!({"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}));
    assert_eq!(m.len(), 6, "a results-only user message adds no empty user message");
    assert_eq!(b["tool_choice"], json!({"type": "function", "function": {"name": "bleep_now"}}));
    assert_eq!(b["tools"][0]["function"]["parameters"], json!({"type": "object"}));
    assert_eq!((b["max_tokens"].as_u64(), b.get("max_completion_tokens")), (Some(500), None));
    assert_eq!(body(&request(), true)["max_completion_tokens"], 500);
    assert_eq!(b["stream_options"]["include_usage"], true);
}

#[test]
fn models_list_reads_openrouter_prices_and_windows() {
    let v = json!({"data": [
        {"id": "z/free", "name": "Z", "context_length": 8000, "pricing": {"prompt": "0", "completion": "0"}},
        {"id": "anthropic/claude-sonnet-4.5", "name": "Claude Sonnet 4.5", "context_length": 1000000, "pricing": {"prompt": "0.000003", "completion": "0.000015", "input_cache_read": "0.0000003"}},
        {"id": "models/gemini-2.5-flash"},
        {"id": "llama3.1:8b"}
    ]});
    let m = parse_models(&v);
    let ids: Vec<_> = m.iter().map(|x| x.id.as_str()).collect();
    assert_eq!(ids, ["anthropic/claude-sonnet-4.5", "gemini-2.5-flash", "llama3.1:8b", "z/free"]);
    let p = m[0].price.unwrap();
    assert!((p.input - 3.0).abs() < 1e-9 && (p.output - 15.0).abs() < 1e-9 && (p.cache_read - 0.3).abs() < 1e-9);
    assert_eq!(m[0].window, Some(1_000_000));
    assert_eq!(m[1].price.unwrap().output, 2.5, "falls back to the built-in table");
    assert!(m[2].price.is_none());
    assert_eq!(m[3].price.unwrap().input, 0.0, "a free model costs zero, not unknown");
}
