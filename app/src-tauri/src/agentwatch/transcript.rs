//! Reads the end of a Claude Code transcript (JSONL). Read-only. Everything in it is
//! agent or tool text: untrusted.
use crate::runtime::ImageIn;
use serde_json::Value;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tail {
    /// The last thing I typed.
    pub original_ask: Option<String>,
    /// The agent's last words, when they end with a question mark.
    pub last_question: Option<String>,
    /// The newest image since my last prompt (a screenshot).
    pub image: Option<ImageIn>,
}

pub const TAIL_BYTES: u64 = 512 * 1024;

/// The last `max_bytes` of a file as whole JSON lines, skipping meta and sidechain rows.
fn last_rows(path: &Path, max_bytes: u64) -> Vec<Value> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else { return vec![] };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max_bytes);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return vec![];
    }
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return vec![];
    }
    let text = String::from_utf8_lossy(&buf);
    let mut lines = text.lines();
    if start > 0 {
        lines.next(); // cut off mid-line
    }
    lines
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|v| v.get("isMeta").and_then(Value::as_bool) != Some(true) && v.get("isSidechain").and_then(Value::as_bool) != Some(true))
        .collect()
}

pub fn read_tail(path: &Path, max_bytes: u64) -> Tail {
    let mut t = Tail::default();
    for v in last_rows(path, max_bytes) {
        let content = v.pointer("/message/content");
        match v.get("type").and_then(Value::as_str) {
            Some("user") => {
                if let Some(prompt) = user_prompt(content) {
                    // A new prompt starts a new turn.
                    t = Tail { original_ask: Some(prompt), ..Tail::default() };
                }
                if let Some(img) = content.and_then(newest_image) {
                    t.image = Some(img);
                }
            }
            Some("assistant") => {
                for b in content.and_then(Value::as_array).into_iter().flatten() {
                    if b.get("type").and_then(Value::as_str) == Some("text") {
                        let said = b.get("text").and_then(Value::as_str).unwrap_or("").trim();
                        t.last_question = said.ends_with('?').then(|| said.to_string());
                    }
                }
            }
            _ => {}
        }
    }
    t
}

/// One line of a session's chat, for the Agents view. All of it is untrusted text.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ChatLine {
    /// "user", "assistant" or "tool".
    pub role: &'static str,
    pub text: String,
}

pub const CHAT_BYTES: u64 = 2 * 1024 * 1024;
pub const CHAT_LINES: usize = 80;
const CHAT_TEXT_MAX: usize = 6000;

/// The newest `max_lines` lines of a Claude Code chat: what I typed, what the agent
/// said, and one short line per tool call. Tool results are left out.
pub fn read_chat(path: &Path, max_bytes: u64, max_lines: usize) -> Vec<ChatLine> {
    let mut out = Vec::new();
    for v in last_rows(path, max_bytes) {
        let content = v.pointer("/message/content");
        match v.get("type").and_then(Value::as_str) {
            Some("user") => {
                if let Some(p) = user_prompt(content).filter(|p| !is_harness_text(p)) {
                    out.push(ChatLine { role: "user", text: clip(&p) });
                }
            }
            Some("assistant") => {
                for b in content.and_then(Value::as_array).into_iter().flatten() {
                    match b.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            let t = b.get("text").and_then(Value::as_str).unwrap_or("").trim();
                            if !t.is_empty() {
                                out.push(ChatLine { role: "assistant", text: clip(t) });
                            }
                        }
                        Some("tool_use") => out.push(ChatLine { role: "tool", text: tool_line(b) }),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    let skip = out.len().saturating_sub(max_lines);
    out.split_off(skip)
}

/// Text Claude Code puts in the user turn itself: slash command echoes and reminders.
fn is_harness_text(t: &str) -> bool {
    ["<command-", "<local-command-", "<system-reminder>", "<task-notification>", "<bash-"].iter().any(|p| t.starts_with(p))
}

fn clip(t: &str) -> String {
    if t.chars().count() > CHAT_TEXT_MAX { t.chars().take(CHAT_TEXT_MAX - 1).collect::<String>() + "…" } else { t.to_string() }
}

/// "Bash: npm test", "Edit: cart.ts", "AskUserQuestion: Which DB?".
fn tool_line(b: &Value) -> String {
    let name = b.get("name").and_then(Value::as_str).unwrap_or("tool");
    let input = b.get("input");
    let text = |k: &str| input.and_then(|i| i.get(k)).and_then(Value::as_str);
    let detail = text("command")
        .map(str::to_string)
        .or_else(|| ["file_path", "notebook_path", "path"].iter().find_map(|k| text(k)).map(|p| p.rsplit('/').next().unwrap_or(p).to_string()))
        .or_else(|| input.and_then(|i| i.pointer("/questions/0/question")).and_then(Value::as_str).map(str::to_string))
        .or_else(|| ["pattern", "url", "description", "prompt"].iter().find_map(|k| text(k)).map(str::to_string));
    match detail {
        Some(d) => super::claude::cut(&format!("{name}: {d}"), 140),
        None => name.to_string(),
    }
}

/// The text I typed: string content, or text blocks that are not tool results.
fn user_prompt(content: Option<&Value>) -> Option<String> {
    match content? {
        Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
        Value::Array(blocks) => {
            if blocks.iter().any(|b| b.get("type").and_then(Value::as_str) == Some("tool_result")) {
                return None;
            }
            let text: Vec<&str> = blocks.iter().filter(|b| b.get("type").and_then(Value::as_str) == Some("text")).filter_map(|b| b.get("text").and_then(Value::as_str)).collect();
            Some(text.join("\n").trim().to_string()).filter(|s| !s.is_empty())
        }
        _ => None,
    }
}

/// The last base64 image anywhere in these blocks (including inside tool results).
fn newest_image(v: &Value) -> Option<ImageIn> {
    match v {
        Value::Array(items) => items.iter().rev().find_map(newest_image),
        Value::Object(o) => {
            if o.get("type").and_then(Value::as_str) == Some("image") {
                let src = o.get("source")?;
                if src.get("type").and_then(Value::as_str) != Some("base64") {
                    return None;
                }
                return Some(ImageIn { media_type: src.get("media_type")?.as_str()?.to_string(), data_b64: src.get("data")?.as_str()?.to_string() });
            }
            o.get("content").and_then(newest_image)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn fixture(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agents").join(name)
    }

    #[test]
    fn tail_finds_ask_question_and_image() {
        let t = read_tail(&fixture("transcript_screenshot.jsonl"), TAIL_BYTES);
        assert_eq!(t.original_ask.as_deref(), Some("Make the checkout button blue and full width, then show me a screenshot."));
        assert_eq!(t.last_question.as_deref(), Some("I made the button blue and full width. Does this look right?"));
        let img = t.image.unwrap();
        assert_eq!(img.media_type, "image/png");
        assert!(img.data_b64.starts_with("iVBORw0KGgo"));
    }

    #[test]
    fn plain_turn_has_no_question_or_image() {
        let t = read_tail(&fixture("transcript_plain.jsonl"), TAIL_BYTES);
        assert_eq!(t.original_ask.as_deref(), Some("Fix the failing test in cart.ts."));
        assert_eq!(t.last_question, None);
        assert_eq!(t.image, None);
    }

    #[test]
    fn small_window_skips_the_cut_line() {
        // Only the last few hundred bytes: the first, cut-off line must not break parsing.
        let t = read_tail(&fixture("transcript_screenshot.jsonl"), 330);
        assert_eq!(t.last_question.as_deref(), Some("I made the button blue and full width. Does this look right?"));
    }

    #[test]
    fn chat_has_prompts_words_and_tool_lines() {
        let c = read_chat(&fixture("transcript_screenshot.jsonl"), CHAT_BYTES, CHAT_LINES);
        assert_eq!(c.first(), Some(&ChatLine { role: "user", text: "old ask that is not the latest".into() }));
        assert!(c.contains(&ChatLine { role: "user", text: "Make the checkout button blue and full width, then show me a screenshot.".into() }));
        assert_eq!(c.last(), Some(&ChatLine { role: "assistant", text: "I made the button blue and full width. Does this look right?".into() }));
        assert!(c.iter().any(|l| l.role == "tool"), "{c:?}");
        assert!(c.iter().all(|l| !l.text.contains("iVBORw0KGgo")), "no image data");
        assert_eq!(read_chat(&fixture("transcript_screenshot.jsonl"), CHAT_BYTES, 1).len(), 1);
    }

    #[test]
    fn chat_skips_harness_text_and_names_tools() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("t.jsonl");
        let rows = [
            serde_json::json!({"type": "user", "message": {"content": "<command-name>/clear</command-name>"}}),
            serde_json::json!({"type": "user", "message": {"content": "Add a DB"}}),
            serde_json::json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "Edit", "input": {"file_path": "/a/b/cart.ts"}}, {"type": "tool_use", "name": "AskUserQuestion", "input": {"questions": [{"question": "Which DB?"}]}}]}}),
            serde_json::json!({"type": "user", "isMeta": true, "message": {"content": "meta"}}),
        ];
        std::fs::write(&p, rows.iter().map(|r| r.to_string()).collect::<Vec<_>>().join("\n")).unwrap();
        let c = read_chat(&p, CHAT_BYTES, CHAT_LINES);
        let got: Vec<(&str, &str)> = c.iter().map(|l| (l.role, l.text.as_str())).collect();
        assert_eq!(got, vec![("user", "Add a DB"), ("tool", "Edit: cart.ts"), ("tool", "AskUserQuestion: Which DB?")]);
    }

    #[test]
    fn tail_of_missing_file_is_empty() {
        assert_eq!(read_tail(Path::new("/nope/x.jsonl"), TAIL_BYTES), Tail::default());
    }
}
