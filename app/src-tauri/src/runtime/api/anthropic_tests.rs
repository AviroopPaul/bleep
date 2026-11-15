use super::*;
use serde_json::json;

pub fn fixture(name: &str) -> Vec<Result<ModelEvent, ApiError>> {
    let bytes = std::fs::read(format!("{}/tests/fixtures/api/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    replay(&bytes, Parser::default())
}

/// Feeds a recorded body through the framer and a parser in 7-byte chunks, the way
/// `drive` does, stopping after the first error.
pub fn replay(bytes: &[u8], mut p: impl WireParser) -> Vec<Result<ModelEvent, ApiError>> {
    let mut f = SseFramer::default();
    let mut out = vec![];
    let mut evs = vec![];
    for c in bytes.chunks(7) {
        evs.extend(f.feed(c));
    }
    evs.extend(f.rest());
    for e in evs {
        match p.on(&e) {
            Ok(v) => out.extend(v.into_iter().map(Ok)),
            Err(e) => {
                out.push(Err(e));
                return out;
            }
        }
    }
    out.extend(p.finish().into_iter().map(Ok));
    out
}

fn ok(v: Vec<Result<ModelEvent, ApiError>>) -> Vec<ModelEvent> {
    v.into_iter().map(|r| r.unwrap()).collect()
}

#[test]
fn a_text_turn_streams_text_then_usage_then_stop() {
    let ev = ok(fixture("anthropic_text.sse"));
    assert_eq!(
        ev,
        [
            ModelEvent::TextDelta("Hello".into()),
            ModelEvent::TextDelta(" there.".into()),
            ModelEvent::Usage(Tokens { input: 12, cache_write: 2000, cache_read: 0, output: 9 }),
            ModelEvent::Stop(StopReason::EndTurn),
        ]
    );
}

#[test]
fn a_tool_turn_joins_streamed_arguments() {
    let ev = ok(fixture("anthropic_tool.sse"));
    assert_eq!(ev[0], ModelEvent::TextDelta("Checking.".into()));
    assert_eq!(ev[1], ModelEvent::ToolCall { id: "toolu_01".into(), name: "bleep_now".into(), args: json!({"tz": "Asia/Kolkata"}) });
    assert_eq!(ev[2], ModelEvent::ToolCall { id: "toolu_02".into(), name: "calendar_list".into(), args: json!({}) }, "no deltas means no arguments");
    assert_eq!(ev[3], ModelEvent::Usage(Tokens { input: 30, cache_read: 1800, cache_write: 0, output: 40 }));
    assert_eq!(ev[4], ModelEvent::Stop(StopReason::ToolUse));
}

#[test]
fn an_overload_mid_stream_is_retryable_and_ends_the_stream() {
    let ev = fixture("anthropic_error.sse");
    assert_eq!(ev.len(), 1);
    assert!(matches!(&ev[0], Err(ApiError::Retryable(m)) if m.contains("Overloaded") && m.contains("not instructions")));
}

#[test]
fn a_stream_cut_short_still_ends_with_usage_and_stop() {
    let ev = ok(replay(b"event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":3}}}\n\n", Parser::default()));
    assert!(matches!(ev.last(), Some(ModelEvent::Stop(StopReason::Other(_)))));
}

fn request(choice: ToolChoice) -> ModelRequest {
    ModelRequest {
        model: "claude-sonnet-5-5".into(),
        system: "Be brief.".into(),
        msgs: vec![
            Msg { role: Role::User, parts: vec![Part::Text { text: "Time?".into() }, Part::Image { media_type: "image/png".into(), data_b64: "AAAA".into() }] },
            Msg { role: Role::Assistant, parts: vec![Part::ToolCall { id: "t1".into(), name: "bleep_now".into(), args: json!({}) }] },
            Msg { role: Role::User, parts: vec![Part::ToolResult { id: "t1".into(), ok: false, text: "boom".into() }] },
        ],
        tools: vec![ToolDef { name: "a".into(), description: "A.".into(), schema: json!({"type": "object"}) }, ToolDef { name: "b".into(), description: "B.".into(), schema: json!({"type": "object"}) }],
        choice,
        max_tokens: 1000,
    }
}

#[test]
fn the_body_uses_three_cache_breakpoints_and_native_parts() {
    let b = body(&request(ToolChoice::Auto));
    assert_eq!(b["system"][0]["cache_control"]["type"], "ephemeral");
    assert!(b["tools"][0].get("cache_control").is_none() && b["tools"][1]["cache_control"]["type"] == "ephemeral", "only the last tool");
    assert_eq!(b["tools"][0]["input_schema"], json!({"type": "object"}));
    assert_eq!(b["messages"][0]["content"][1], json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AAAA"}}));
    assert_eq!(b["messages"][1]["content"][0]["type"], "tool_use");
    let last = &b["messages"][2]["content"][0];
    assert_eq!((last["type"].as_str(), last["is_error"].as_bool(), last["cache_control"]["type"].as_str()), (Some("tool_result"), Some(true), Some("ephemeral")));
    assert_eq!(b["tool_choice"], json!({"type": "auto"}));
    assert_eq!(b["stream"], true);
}

#[test]
fn a_forced_tool_is_named() {
    assert_eq!(body(&request(ToolChoice::Force("answer".into())))["tool_choice"], json!({"type": "tool", "name": "answer"}));
    assert_eq!(body(&request(ToolChoice::None))["tool_choice"], json!({"type": "none"}));
}

#[test]
fn models_list_reads_ids_names_and_windows() {
    let v = json!({"data": [{"id": "claude-haiku-4-5-20251001", "display_name": "Claude Haiku 4.5", "max_input_tokens": 200000}, {"id": "x"}]});
    let m = parse_models(&v);
    assert_eq!((m[0].id.as_str(), m[0].name.as_deref(), m[0].window), ("claude-haiku-4-5-20251001", Some("Claude Haiku 4.5"), Some(200000)));
    assert!(m[0].price.is_some() && m[1].price.is_none());
}
