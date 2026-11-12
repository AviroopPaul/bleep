//! Claude Code `--output-format stream-json` lines to AgentEvents.
//! Shapes recorded from Claude Code 2.1.319 on 12 Nov 2026. Uses Value lookups, not a typed
//! model, because the CLI adds fields often.
use crate::runtime::{preview, AgentEvent};
use serde_json::Value;

pub const TOOL_PREFIX: &str = "mcp__bleep__";

#[derive(Default)]
pub struct ClaudeParser;

fn tool_result_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

fn u(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(Value::as_u64).unwrap_or(0)
}

impl ClaudeParser {
    pub fn feed(&mut self, line: &str) -> Vec<AgentEvent> {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return vec![];
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            eprintln!("bleep: unparsed claude line: {}", preview(line, 120));
            return vec![];
        };
        let content = || v.pointer("/message/content").and_then(Value::as_array).cloned().unwrap_or_default();
        match v.get("type").and_then(Value::as_str) {
            Some("stream_event") => {
                let delta = v.pointer("/event/delta");
                match (v.pointer("/event/type").and_then(Value::as_str), delta.and_then(|d| d.get("type")).and_then(Value::as_str)) {
                    (Some("content_block_delta"), Some("text_delta")) => {
                        let text = delta.and_then(|d| d.get("text")).and_then(Value::as_str).unwrap_or("").to_string();
                        vec![AgentEvent::TextDelta { text }]
                    }
                    _ => vec![],
                }
            }
            Some("assistant") => content()
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
                .map(|b| {
                    let name = b.get("name").and_then(Value::as_str).unwrap_or("");
                    AgentEvent::ToolStart {
                        id: b.get("id").and_then(Value::as_str).unwrap_or("").into(),
                        name: name.strip_prefix(TOOL_PREFIX).unwrap_or(name).into(),
                        args: b.get("input").cloned().unwrap_or(Value::Null),
                    }
                })
                .collect(),
            Some("user") => content()
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
                .map(|b| AgentEvent::ToolResult {
                    id: b.get("tool_use_id").and_then(Value::as_str).unwrap_or("").into(),
                    ok: !b.get("is_error").and_then(Value::as_bool).unwrap_or(false),
                    preview: preview(&tool_result_text(b.get("content").unwrap_or(&Value::Null)), 200),
                })
                .collect(),
            Some("rate_limit_event") => {
                let info = &v["rate_limit_info"];
                match info.get("status").and_then(Value::as_str) {
                    Some("allowed") | None => vec![],
                    Some(_) => vec![AgentEvent::RateLimited { until: info.get("resetsAt").and_then(Value::as_i64).unwrap_or(0) }],
                }
            }
            Some("result") => {
                let sid = v.get("session_id").and_then(Value::as_str).map(String::from);
                if v.get("is_error").and_then(Value::as_bool).unwrap_or(false) {
                    let message = v
                        .pointer("/errors/0")
                        .or_else(|| v.get("result"))
                        .or_else(|| v.get("api_error_status"))
                        .and_then(Value::as_str)
                        .unwrap_or("Claude Code reported an error.")
                        .to_string();
                    vec![AgentEvent::Error { message, retryable: false }, AgentEvent::Done { runtime_session_id: sid }]
                } else {
                    let us = &v["usage"];
                    let input = u(us, "input_tokens") + u(us, "cache_creation_input_tokens") + u(us, "cache_read_input_tokens");
                    vec![AgentEvent::Usage { input, output: u(us, "output_tokens") }, AgentEvent::Done { runtime_session_id: sid }]
                }
            }
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::contract::*;
    use crate::runtime::AgentEvent;

    fn fixture(name: &str) -> Vec<AgentEvent> {
        let path = format!("{}/tests/fixtures/claude/{name}", env!("CARGO_MANIFEST_DIR"));
        let mut p = ClaudeParser::default();
        std::fs::read_to_string(path).unwrap().lines().flat_map(|l| p.feed(l)).collect()
    }

    #[test]
    fn text_turn_yields_text_usage_done() {
        let ev = fixture("text_turn.jsonl");
        assert_text_turn(&ev, "hi there");
        assert!(ev.contains(&AgentEvent::Usage { input: 3557, output: 5 }));
        assert_eq!(ev.last(), Some(&AgentEvent::Done { runtime_session_id: Some("b5b2cc90-ab10-43ed-b1af-333853209ad8".into()) }));
    }

    #[test]
    fn tool_turn_yields_tool_steps() {
        let ev = fixture("tool_turn.jsonl");
        assert_tool_turn(&ev, "bleep_now");
        assert!(ev.iter().any(|e| matches!(e, AgentEvent::ToolResult { preview, .. } if preview.contains("utc_offset"))));
    }

    #[test]
    fn rate_limit_rejected_emits_rate_limited() {
        let ev = fixture("rate_limited.jsonl");
        assert_rate_limited(&ev);
        assert!(ev.contains(&AgentEvent::RateLimited { until: 1794493800 }));
    }

    #[test]
    fn allowed_rate_limit_is_silent() {
        assert!(!fixture("text_turn.jsonl").iter().any(|e| matches!(e, AgentEvent::RateLimited { .. })));
    }

    #[test]
    fn bad_resume_yields_error_then_done() {
        let ev = fixture("init_error.jsonl");
        match &ev[0] {
            AgentEvent::Error { message, .. } => assert!(message.contains("No conversation found")),
            e => panic!("expected error, got {e:?}"),
        }
        assert!(matches!(ev.last(), Some(AgentEvent::Done { .. })));
    }

    #[test]
    fn tool_use_name_is_unprefixed() {
        let mut p = ClaudeParser::default();
        let ev = p.feed(r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"mcp__bleep__bleep_now","input":{}}]}}"#);
        assert_eq!(ev, vec![AgentEvent::ToolStart { id: "t1".into(), name: "bleep_now".into(), args: serde_json::json!({}) }]);
    }

    #[test]
    fn tool_result_maps_ok_and_preview() {
        let mut p = ClaudeParser::default();
        let long = "x".repeat(500);
        let line = format!(r#"{{"type":"user","message":{{"content":[{{"type":"tool_result","tool_use_id":"t1","content":[{{"type":"text","text":"{long}"}}]}}]}}}}"#);
        match &p.feed(&line)[0] {
            AgentEvent::ToolResult { id, ok, preview } => {
                assert_eq!((id.as_str(), *ok, preview.chars().count()), ("t1", true, 200));
            }
            e => panic!("{e:?}"),
        }
    }

    #[test]
    fn assistant_text_blocks_are_not_repeated() {
        let mut p = ClaudeParser::default();
        assert!(p.feed(r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hi there"}]}}"#).is_empty());
    }

    #[test]
    fn garbage_and_comment_lines_are_ignored() {
        let mut p = ClaudeParser::default();
        assert!(p.feed("not json").is_empty());
        assert!(p.feed("# exit=1").is_empty());
        assert!(p.feed("").is_empty());
    }
}
