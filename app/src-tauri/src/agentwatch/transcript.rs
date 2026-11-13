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

pub fn read_tail(path: &Path, max_bytes: u64) -> Tail {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else { return Tail::default() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max_bytes);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return Tail::default();
    }
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return Tail::default();
    }
    let text = String::from_utf8_lossy(&buf);
    let mut lines = text.lines();
    if start > 0 {
        lines.next(); // cut off mid-line
    }
    let mut t = Tail::default();
    for line in lines {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v.get("isMeta").and_then(Value::as_bool) == Some(true) || v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            continue;
        }
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
    fn tail_of_missing_file_is_empty() {
        assert_eq!(read_tail(Path::new("/nope/x.jsonl"), TAIL_BYTES), Tail::default());
    }
}
