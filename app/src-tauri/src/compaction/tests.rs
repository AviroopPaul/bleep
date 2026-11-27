use super::*;
use crate::config::MemoryConfig;
use crate::store::{ApprovalStatus, Kind, Message, MsgStatus, Role};
use serde_json::json;

fn m(id: i64, role: Role, kind: Kind, content: &str) -> Message {
    Message { id, role, kind, content: content.into(), tool_steps: json!([]), tokens: None, status: MsgStatus::Done, session_id: Some(1), created_at: String::new(), approvals: vec![], error: None }
}

fn cfg(compact_at: u64, window: u64) -> MemoryConfig {
    MemoryConfig { compact_at, nightly: "22:00".into(), context_window: Some(window) }
}

#[test]
fn trigger_levels() {
    let c = cfg(250_000, 1_000_000);
    assert_eq!(trigger(249_999, &c), Trigger::None);
    assert_eq!(trigger(250_000, &c), Trigger::Compact);
    assert_eq!(trigger(950_000, &c), Trigger::HardStop);
    assert_eq!(trigger(30_000, &cfg(20_000, 1_000_000)), Trigger::Compact, "low test threshold");
}

#[test]
fn transcript_lines_seed_first_and_tools_named() {
    let mut a = m(2, Role::Assistant, Kind::Chat, "It is noon.");
    a.tool_steps = json!([{"name": "bleep_now"}, {"name": "calendar_next"}]);
    let msgs = vec![
        m(1, Role::User, Kind::Chat, "what time is it"),
        a,
        m(3, Role::System, Kind::System, "Started a fresh Claude session."),
        m(4, Role::User, Kind::Scheduled, "Prep me: Design review"),
        m(5, Role::System, Kind::Divider, "Compacted"),
    ];
    let t = transcript(Some("OLD SUMMARY"), &msgs);
    assert_eq!(t, "Earlier summary:\nOLD SUMMARY\n\nuser: what time is it\nbleep [used bleep_now, calendar_next]: It is noon.\nuser (scheduled): Prep me: Design review");
}

#[test]
fn transcript_keeps_the_newest_when_long() {
    let msgs: Vec<Message> = (0..400).map(|i| m(i, Role::User, Kind::Chat, &format!("line {i} {}", "x".repeat(1000)))).collect();
    let t = transcript(Some("SEED"), &msgs);
    assert!(t.len() <= TRANSCRIPT_BYTES + 200, "{}", t.len());
    assert!(t.starts_with("Earlier summary:\nSEED") && t.contains("line 399 ") && !t.contains("line 0 "));
}

#[test]
fn seed_has_approvals_summary_and_last_turns() {
    let msgs: Vec<Message> = (0..30)
        .map(|i| m(i, if i % 2 == 0 { Role::User } else { Role::Assistant }, Kind::Chat, &format!("turn {i} {}", if i == 29 { "y".repeat(3000) } else { String::new() })))
        .collect();
    let pending = vec![crate::store::Approval {
        id: 7,
        tool: "calendar_create".into(),
        payload: json!({"title": "Pay electricity bill", "start": "2026-11-14T19:00:00+05:30"}),
        status: ApprovalStatus::Pending,
        turn_message_id: None,
        result: None,
        created_at: String::new(),
        decided_at: None,
    }];
    let s = seed(Some("SUMMARY TEXT"), &pending, &msgs);
    let (a, b, c) = (s.find("## Waiting for the user").unwrap(), s.find("## Summary").unwrap(), s.find("## Last turns").unwrap());
    assert!(a < b && b < c, "{s}");
    assert!(s.contains("calendar_create") && s.contains("Pay electricity bill"));
    assert!(s.contains("SUMMARY TEXT"));
    assert!(s.contains("user: turn 10") && !s.contains("user: turn 8 "), "last 20 rows = 10 turns");
    assert!(s.contains(&format!("bleep: turn 29 {}…", "y".repeat(TURN_CHARS - 8))), "long turns are cut");
    let hard = seed(None, &[], &msgs);
    assert!(!hard.contains("## Summary") && !hard.contains("## Waiting") && hard.contains("## Last turns"));
}

#[test]
fn divider_texts() {
    assert_eq!(divider_text(160_000, 12_000), "Compacted 160K into a 3K summary. Facts saved to memory.");
    assert_eq!(divider_text(21_400, 100), "Compacted 21K into a 1K summary. Facts saved to memory.");
    assert_eq!(hard_stop_text(950_000), "Started fresh at 950K. The summary failed, so only the last turns carried over.");
    for t in [divider_text(1, 1), hard_stop_text(1), FACTS_PROMPT.into(), SUMMARY_PROMPT.into()] {
        assert!(!t.contains('\u{2014}'), "{t}");
    }
}
