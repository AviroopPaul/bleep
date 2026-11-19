//! Claude Code hooks: what each event means for a session, and the exact JSON bleep
//! prints back through `bleep-hook`.
use super::rules::Ask;
use super::AskKind;
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq)]
pub enum ClaudeEvent {
    Start,
    Prompt,
    Permission(Ask),
    ToolDone,
    Notify { idle: bool },
    Stop { active: bool },
    End,
}

pub fn classify(event: &str, payload: &Value) -> Option<ClaudeEvent> {
    Some(match event {
        "SessionStart" => ClaudeEvent::Start,
        "UserPromptSubmit" => ClaudeEvent::Prompt,
        "PermissionRequest" => ClaudeEvent::Permission(ask_from_permission(payload)?),
        "PostToolUse" => ClaudeEvent::ToolDone,
        "Notification" => match payload.get("notification_type").and_then(Value::as_str) {
            Some("idle_prompt") => ClaudeEvent::Notify { idle: true },
            Some("permission_prompt") => ClaudeEvent::Notify { idle: false },
            _ => return None,
        },
        "Stop" => ClaudeEvent::Stop { active: payload.get("stop_hook_active").and_then(Value::as_bool).unwrap_or(false) },
        "SessionEnd" => ClaudeEvent::End,
        _ => return None,
    })
}

pub fn ask_from_permission(payload: &Value) -> Option<Ask> {
    let tool_name = payload.get("tool_name")?.as_str()?.to_string();
    let cwd = payload.get("cwd")?.as_str()?.to_string();
    let input = payload.get("tool_input").cloned().unwrap_or(Value::Null);
    let text = |k: &str| input.get(k).and_then(Value::as_str).map(str::to_string);
    let command = text("command");
    let mut paths: Vec<String> = ["file_path", "path", "notebook_path"].iter().filter_map(|k| text(k)).collect();
    // A Glob pattern is a path too when it is absolute or climbs out.
    if let Some(p) = text("pattern").filter(|p| tool_name == "Glob" && (p.starts_with('/') || p.contains(".."))) {
        paths.push(p);
    }
    Some(Ask { kind: AskKind::Permission, tool_name, command, paths, cwd, sandbox_off: input.get("dangerouslyDisableSandbox").and_then(Value::as_bool).unwrap_or(false) })
}

fn file_name(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
}

pub fn summary(ask: &Ask) -> String {
    let first = ask.paths.first().map(|p| file_name(p)).unwrap_or("a file");
    match ask.tool_name.as_str() {
        "Bash" | "commandExecution" => format!("wants to run {}", cut(ask.command.as_deref().unwrap_or(""), 60)),
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => format!("wants to edit {}", cut(first, 60)),
        "Read" | "Glob" | "Grep" | "LS" => format!("wants to read {}", cut(first, 60)),
        "fileChange" => "wants to change files".into(),
        t => format!("wants to use {}", cut(t, 60)),
    }
}

/// One short line from an agent's question: its last sentence.
pub fn question_summary(text: &str) -> String {
    let t = text.trim();
    let body = &t[..t.len().saturating_sub(1)];
    let start = body.rfind(['.', '!', '?', '\n']).map(|i| i + 1).unwrap_or(0);
    cut(t[start..].trim(), 80)
}

pub fn allow_output() -> Value {
    json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": {"behavior": "allow"}}})
}

/// My "no" from bleep. The agent goes on without the tool.
pub fn deny_output() -> Value {
    json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": {"behavior": "deny", "message": "The user said no in bleep."}}})
}

/// Claude Code's tool for questions with options. Never approved by a rule.
pub const ASK_USER_QUESTION: &str = "AskUserQuestion";

/// My answers to an AskUserQuestion call: question text to option label (several labels
/// joined with ", "). Claude Code takes them as the tool's `answers` input.
pub fn answers_output(questions: &Value, answers: &serde_json::Map<String, Value>) -> Value {
    json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": {"behavior": "allow", "updatedInput": {"questions": questions, "answers": answers}}}})
}

/// What `bleep-hook claude-reply` prints to wake the session. Only my own words go in.
pub fn reply_output(text: &str) -> Value {
    json!({"rewake": format!("The user replied from bleep:\n\n{}", text.trim())})
}

/// bleep's auto-answer: the agent's own first suggested reply, with a fixed note. No model
/// text goes in.
pub fn auto_reply_output(label: &str) -> Value {
    json!({"rewake": format!("bleep answered for the user with your first suggested reply, as their auto-answer rules allow:\n\n{}", label.trim())})
}

/// The question texts of an AskUserQuestion input.
pub fn question_texts(questions: &Value) -> Vec<String> {
    questions.as_array().into_iter().flatten().filter_map(|q| q.get("question").and_then(Value::as_str)).map(str::to_string).collect()
}

/// A fixed line: the small model read agent-made pixels, so its words never reach the agent.
pub fn go_on_output() -> Value {
    json!({"decision": "block", "reason": "bleep checked the screenshot against your ask and it matches. Go on."})
}

/// Cuts to `n` characters, ending with "…" when cut.
pub fn cut(s: &str, n: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > n { s.chars().take(n.saturating_sub(1)).collect::<String>() + "…" } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx(name: &str) -> Value {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agents/claude").join(name);
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn classifies_each_fixture() {
        assert_eq!(classify("SessionStart", &fx("session_start.json")), Some(ClaudeEvent::Start));
        assert_eq!(classify("UserPromptSubmit", &fx("user_prompt_submit.json")), Some(ClaudeEvent::Prompt));
        assert!(matches!(classify("PermissionRequest", &fx("permission_request_bash.json")), Some(ClaudeEvent::Permission(_))));
        assert_eq!(classify("PostToolUse", &fx("post_tool_use.json")), Some(ClaudeEvent::ToolDone));
        assert_eq!(classify("Notification", &fx("notification_permission.json")), Some(ClaudeEvent::Notify { idle: false }));
        assert_eq!(classify("Notification", &fx("notification_idle.json")), Some(ClaudeEvent::Notify { idle: true }));
        assert_eq!(classify("Notification", &json!({"notification_type": "auth_success"})), None);
        assert_eq!(classify("Stop", &fx("stop.json")), Some(ClaudeEvent::Stop { active: false }));
        assert_eq!(classify("Stop", &fx("stop_active.json")), Some(ClaudeEvent::Stop { active: true }));
        assert_eq!(classify("SessionEnd", &fx("session_end.json")), Some(ClaudeEvent::End));
        assert_eq!(classify("Mystery", &json!({})), None);
    }

    #[test]
    fn ask_from_bash_and_read_permissions() {
        let a = ask_from_permission(&fx("permission_request_bash.json")).unwrap();
        assert_eq!((a.kind, a.tool_name.as_str(), a.command.as_deref(), a.cwd.as_str()), (AskKind::Permission, "Bash", Some("npm test"), "/Users/me/code/shop"));
        assert!(a.paths.is_empty());
        let r = ask_from_permission(&fx("permission_request_read.json")).unwrap();
        assert_eq!((r.tool_name.as_str(), r.command, r.paths), ("Read", None, vec!["/etc/hosts".to_string()]));
        assert!(ask_from_permission(&json!({"tool_name": "Bash"})).is_none(), "no cwd");
        let mut p = fx("permission_request_bash.json");
        p["tool_input"]["dangerouslyDisableSandbox"] = json!(true);
        assert!(ask_from_permission(&p).unwrap().sandbox_off);
        assert!(!a.sandbox_off);
    }

    #[test]
    fn summaries_are_short_and_plain() {
        let mut a = ask_from_permission(&fx("permission_request_bash.json")).unwrap();
        assert_eq!(summary(&a), "wants to run npm test");
        a.command = Some(format!("npm test -- {}\nrm x", "a".repeat(80)));
        let s = summary(&a);
        assert!(s.chars().count() <= "wants to run ".len() + 60 && s.ends_with('…') && !s.contains('\n'), "{s}");
        let e = Ask { tool_name: "Edit".into(), command: None, paths: vec!["/Users/me/code/shop/src/cart.ts".into()], ..a.clone() };
        assert_eq!(summary(&e), "wants to edit cart.ts");
        assert_eq!(summary(&Ask { tool_name: "WebFetch".into(), command: None, paths: vec![], ..a.clone() }), "wants to use WebFetch");
        assert_eq!(summary(&Ask { tool_name: "fileChange".into(), command: None, paths: vec![], ..a }), "wants to change files");
        assert_eq!(question_summary("I fixed it. Tests pass. Does this look right?"), "Does this look right?");
        assert!(question_summary(&format!("{}?", "word ".repeat(40))).chars().count() <= 80);
        for s in [summary(&e), question_summary("A. B?")] {
            assert!(!s.contains('\u{2014}'));
        }
    }

    #[test]
    fn answers_and_replies_match_claude_code() {
        let qs = json!([{"question": "Which DB?", "header": "DB", "options": [{"label": "Neon"}, {"label": "SQLite"}], "multiSelect": false}]);
        let mut a = serde_json::Map::new();
        a.insert("Which DB?".into(), json!("Neon"));
        assert_eq!(answers_output(&qs, &a)["hookSpecificOutput"]["decision"], json!({"behavior": "allow", "updatedInput": {"questions": qs, "answers": {"Which DB?": "Neon"}}}));
        assert_eq!(question_texts(&qs), vec!["Which DB?".to_string()]);
        assert_eq!(deny_output()["hookSpecificOutput"]["decision"]["behavior"], "deny");
        assert_eq!(reply_output("  Yes, go.\n"), json!({"rewake": "The user replied from bleep:\n\nYes, go."}));
        let auto = auto_reply_output(" Merge it ");
        assert!(auto["rewake"].as_str().unwrap().ends_with("\n\nMerge it") && !auto.to_string().contains('\u{2014}'));
    }

    #[test]
    fn outputs_match_claude_code() {
        assert_eq!(allow_output(), json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": {"behavior": "allow"}}}));
        // Fixed text: the model's own words never go back into the agent.
        assert_eq!(go_on_output(), json!({"decision": "block", "reason": "bleep checked the screenshot against your ask and it matches. Go on."}));
    }
}
