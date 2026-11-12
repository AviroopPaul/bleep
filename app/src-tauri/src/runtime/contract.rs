//! Contract checks every AgentRuntime backend must pass on its recorded fixtures (build plan section 10).
#![cfg(test)]
use super::AgentEvent;

/// Deltas join to `expected`, one Usage with output > 0, last event is Done with a session id.
pub fn assert_text_turn(ev: &[AgentEvent], expected: &str) {
    let text: String = ev.iter().filter_map(|e| match e { AgentEvent::TextDelta { text } => Some(text.as_str()), _ => None }).collect();
    assert_eq!(text, expected, "joined text deltas");
    let usages: Vec<_> = ev.iter().filter(|e| matches!(e, AgentEvent::Usage { .. })).collect();
    assert_eq!(usages.len(), 1, "exactly one Usage");
    assert!(matches!(usages[0], AgentEvent::Usage { output, .. } if *output > 0), "Usage.output > 0");
    assert!(matches!(ev.last(), Some(AgentEvent::Done { runtime_session_id: Some(_) })), "ends with Done(Some)");
}

/// A ToolStart for `tool` and an ok ToolResult with the same id, both before the last TextDelta.
pub fn assert_tool_turn(ev: &[AgentEvent], tool: &str) {
    let start = ev.iter().position(|e| matches!(e, AgentEvent::ToolStart { name, .. } if name == tool)).expect("ToolStart");
    let AgentEvent::ToolStart { id, .. } = &ev[start] else { unreachable!() };
    let result = ev.iter().position(|e| matches!(e, AgentEvent::ToolResult { id: rid, ok: true, .. } if rid == id)).expect("ok ToolResult with same id");
    let last_text = ev.iter().rposition(|e| matches!(e, AgentEvent::TextDelta { .. })).expect("some text");
    assert!(start < result && result < last_text, "tool step happens before the final text");
    assert!(matches!(ev.last(), Some(AgentEvent::Done { .. })));
}

pub fn assert_rate_limited(ev: &[AgentEvent]) {
    assert!(ev.iter().any(|e| matches!(e, AgentEvent::RateLimited { until } if *until > 0)), "RateLimited with a reset time");
}
