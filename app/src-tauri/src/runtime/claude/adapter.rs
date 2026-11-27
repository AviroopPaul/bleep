//! Claude Code as a plain model: one stateless `claude -p` call per model step.
//!
//! bleep owns the loop, the tools and the history (`runtime::api::agent::Core`). Each call
//! gets the whole conversation from bleep, Claude Code's own tools, MCP, settings and
//! session files are off, and the reply comes back as JSON (`--json-schema`): the text for
//! the user and the tool calls bleep should run. Claude Code never runs a tool and never
//! keeps a session.
use super::args::base_command;
use crate::paths::BleepPaths;
use crate::runtime::api::adapter::ModelAdapter;
use crate::runtime::api::types::*;
use crate::runtime::preview;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub const STALLED: &str = "Claude Code stopped responding. Send again to continue.";
pub const EXITED: &str = "Claude Code stopped before it answered. Send again to continue.";
pub const SIGNED_OUT: &str = "Claude Code is signed out. Open Settings, then Model, to sign in.";
/// No output for this long ends the call.
const IDLE: Duration = Duration::from_secs(120);

/// How bleep works with the model. Sent after bleep's own system prompt when tools are on.
pub const PROTOCOL: &str = "# How you work inside bleep
You are the language model inside bleep's agent loop. Each time, bleep sends you the whole conversation as JSON lines. You reply once, through your structured output, with two fields: text and tool_calls.
bleep's tools are listed below. They are not in your native tool list. You use them by putting {name, args} items in tool_calls. bleep runs them on the user's Mac, shows the user any approval card, and sends you each result as a tool_result line. Then you reply again.
So every tool below is available to you now. When a tool can get what the user needs, call it through tool_calls. Never say a tool is unavailable, and never say you cannot check something a tool can check.
Put what you say to the user in text. While you call tools, text can be empty or one short line. When you need no more tools, give your full answer in text and leave tool_calls empty.
Call only the tools listed below, with args that match their input schemas.";

const NO_TOOLS: &str = "# Tools\nNo tools are available for this message. Answer in text.";
const ONE_SHOT_SYSTEM: &str = "Give your answer in the requested JSON shape.";

pub struct ClaudeCodeAdapter {
    pub bin: PathBuf,
    pub paths: BleepPaths,
    pub idle: Duration,
}

impl ClaudeCodeAdapter {
    pub fn new(bin: PathBuf, paths: BleepPaths) -> Self {
        Self { bin, paths, idle: IDLE }
    }
}

/// What the call should return, from the request's tool choice.
#[derive(Debug, PartialEq)]
enum Shape {
    /// Plain text, no schema.
    Text,
    /// `{text, tool_calls}`.
    Step,
    /// One forced tool: the reply is its args.
    Forced(String),
}

fn shape(req: &ModelRequest) -> Shape {
    match &req.choice {
        ToolChoice::Force(name) => Shape::Forced(name.clone()),
        ToolChoice::Auto if !req.tools.is_empty() => Shape::Step,
        _ => Shape::Text,
    }
}

pub fn step_schema(tools: &[ToolDef]) -> Value {
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    json!({
        "type": "object",
        "properties": {
            "text": {"type": "string"},
            "tool_calls": {"type": "array", "items": {
                "type": "object",
                "properties": {"name": {"type": "string", "enum": names}, "args": {"type": "object"}},
                "required": ["name", "args"]
            }}
        },
        "required": ["text", "tool_calls"]
    })
}

pub fn system_prompt(req: &ModelRequest) -> String {
    match shape(req) {
        Shape::Forced(_) if req.system.trim().is_empty() => ONE_SHOT_SYSTEM.into(),
        Shape::Forced(_) => req.system.clone(),
        Shape::Text if req.choice == ToolChoice::None && !req.tools.is_empty() => format!("{}\n\n{NO_TOOLS}", req.system),
        Shape::Text => req.system.clone(),
        Shape::Step => {
            let tools: Vec<Value> = req.tools.iter().map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.schema})).collect();
            format!("{}\n\n{PROTOCOL}\n\n# Tools\n{}", req.system, serde_json::to_string_pretty(&tools).unwrap_or_default())
        }
    }
}

/// One JSON line per message part, in order. Images are noted, not sent, inside a transcript.
pub fn transcript(msgs: &[Msg]) -> String {
    let mut lines = vec![];
    for m in msgs {
        let (mut text, mut calls) = (vec![], vec![]);
        let flush = |text: &mut Vec<String>, calls: &mut Vec<Value>, lines: &mut Vec<String>| {
            if text.is_empty() && calls.is_empty() {
                return;
            }
            let mut v = json!({"role": if m.role == Role::User { "user" } else { "assistant" }, "text": text.join("\n")});
            if !calls.is_empty() {
                v["tool_calls"] = Value::Array(std::mem::take(calls));
            }
            text.clear();
            lines.push(v.to_string());
        };
        for p in &m.parts {
            match p {
                Part::Text { text: t } => text.push(t.clone()),
                Part::Image { .. } => text.push("[image]".into()),
                Part::ToolCall { id, name, args } => calls.push(json!({"id": id, "name": name, "args": args})),
                Part::ToolResult { id, ok, text: t } => {
                    flush(&mut text, &mut calls, &mut lines);
                    lines.push(json!({"role": "tool_result", "id": id, "ok": ok, "result": t}).to_string());
                }
            }
        }
        flush(&mut text, &mut calls, &mut lines);
    }
    format!("<conversation>\n{}\n</conversation>\nWrite the assistant's next message.", lines.join("\n"))
}

/// The one stdin line. A single plain user message (a one-shot) goes as it is, images too.
pub fn input_line(msgs: &[Msg]) -> String {
    let single = msgs.len() == 1 && msgs[0].role == Role::User && msgs[0].parts.iter().all(|p| matches!(p, Part::Text { .. } | Part::Image { .. }));
    let content: Vec<Value> = if single {
        msgs[0]
            .parts
            .iter()
            .map(|p| match p {
                Part::Image { media_type, data_b64 } => json!({"type": "image", "source": {"type": "base64", "media_type": media_type, "data": data_b64}}),
                Part::Text { text } => json!({"type": "text", "text": text}),
                _ => unreachable!(),
            })
            .collect()
    } else {
        vec![json!({"type": "text", "text": transcript(msgs)})]
    };
    json!({"type": "user", "message": {"role": "user", "content": content}}).to_string() + "\n"
}

pub fn call_args(model: &str, system_file: &str, schema: Option<&Value>) -> Vec<String> {
    let mut a: Vec<String> = [
        "-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose", "--include-partial-messages",
        "--tools", "", "--setting-sources", "", "--strict-mcp-config", "--no-session-persistence", "--disable-slash-commands",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    a.extend(["--model".into(), model.into(), "--system-prompt-file".into(), system_file.into()]);
    if let Some(s) = schema {
        a.extend(["--json-schema".into(), s.to_string()]);
    }
    a
}

/// The decoded value of a top-level string field in JSON that may be cut off anywhere.
/// Returns what is complete so far, or None while the field has not started.
pub fn partial_field(buf: &str, field: &str) -> Option<String> {
    let b = buf.as_bytes();
    let (mut i, mut depth) = (0, 0i32);
    while i < b.len() {
        match b[i] {
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth -= 1,
            b'"' => {
                let (s, end) = read_string(buf, i + 1);
                let end = end?;
                i = end + 1;
                // A key at depth 1 is followed by ':'.
                let rest = buf[i..].trim_start();
                if depth == 1 && rest.starts_with(':') && s == field {
                    let after = rest[1..].trim_start();
                    if !after.starts_with('"') {
                        return None;
                    }
                    let start = buf.len() - after.len() + 1;
                    return Some(read_string(buf, start).0);
                }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Decodes a JSON string from byte `start` (after the opening quote). Returns the text and
/// the index of the closing quote, if it has arrived. A cut escape is left out.
fn read_string(buf: &str, start: usize) -> (String, Option<usize>) {
    let mut out = String::new();
    let mut chars = buf[start..].char_indices();
    while let Some((k, c)) = chars.next() {
        match c {
            '"' => return (out, Some(start + k)),
            '\\' => match chars.next() {
                Some((_, 'n')) => out.push('\n'),
                Some((_, 't')) => out.push('\t'),
                Some((_, 'r')) => out.push('\r'),
                Some((_, 'b')) => out.push('\u{8}'),
                Some((_, 'f')) => out.push('\u{c}'),
                Some((_, 'u')) => {
                    let hex: String = chars.by_ref().take(4).map(|(_, c)| c).collect();
                    if hex.len() < 4 {
                        return (out, None);
                    }
                    let Ok(n) = u32::from_str_radix(&hex, 16) else { return (out, None) };
                    // A surrogate pair needs the next \uXXXX too.
                    if (0xD800..0xDC00).contains(&n) {
                        let rest: String = chars.by_ref().take(6).map(|(_, c)| c).collect();
                        let lo = rest.strip_prefix("\\u").and_then(|h| u32::from_str_radix(h, 16).ok());
                        match lo {
                            Some(lo) if rest.len() == 6 => out.extend(char::from_u32(0x10000 + ((n - 0xD800) << 10) + (lo.wrapping_sub(0xDC00)))),
                            _ => return (out, None),
                        }
                    } else {
                        out.extend(char::from_u32(n));
                    }
                }
                Some((_, c)) => out.push(c),
                None => return (out, None),
            },
            c => out.push(c),
        }
    }
    (out, None)
}

/// Turns Claude Code's stream-json lines into ModelEvents for one call.
pub struct CallParser {
    shape: Shape,
    json: String,
    sent: usize,
    rate_limited: Option<i64>,
    ids: String,
}

fn u(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(Value::as_u64).unwrap_or(0)
}

impl CallParser {
    fn new(shape: Shape) -> Self {
        Self { shape, json: String::new(), sent: 0, rate_limited: None, ids: crate::mcp::new_token()[..10].to_string() }
    }

    fn text_so_far(&self) -> Option<String> {
        partial_field(&self.json, "text")
    }

    fn more_text(&mut self, full: &str) -> Vec<Result<ModelEvent, ApiError>> {
        let chars = full.chars().count();
        if chars <= self.sent {
            return vec![];
        }
        let t: String = full.chars().skip(self.sent).collect();
        self.sent = chars;
        vec![Ok(ModelEvent::TextDelta(t))]
    }

    /// Events for one line. `Err` ends the call.
    pub fn feed(&mut self, line: &str) -> Vec<Result<ModelEvent, ApiError>> {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else { return vec![] };
        match v.get("type").and_then(Value::as_str) {
            Some("stream_event") => {
                let d = v.pointer("/event/delta");
                match d.and_then(|d| d.get("type")).and_then(Value::as_str) {
                    Some("text_delta") if self.shape == Shape::Text => {
                        let t = d.and_then(|d| d.get("text")).and_then(Value::as_str).unwrap_or("").to_string();
                        self.sent += t.chars().count();
                        vec![Ok(ModelEvent::TextDelta(t))]
                    }
                    Some("input_json_delta") if self.shape == Shape::Step => {
                        self.json.push_str(d.and_then(|d| d.get("partial_json")).and_then(Value::as_str).unwrap_or(""));
                        match self.text_so_far() {
                            Some(t) => self.more_text(&t),
                            None => vec![],
                        }
                    }
                    _ => vec![],
                }
            }
            Some("rate_limit_event") => {
                let info = &v["rate_limit_info"];
                if info.get("status").and_then(Value::as_str) == Some("rejected") {
                    self.rate_limited = Some(info.get("resetsAt").and_then(Value::as_i64).unwrap_or(0));
                }
                vec![]
            }
            Some("result") => self.result(&v),
            _ => vec![],
        }
    }

    fn result(&mut self, v: &Value) -> Vec<Result<ModelEvent, ApiError>> {
        if v.get("is_error").and_then(Value::as_bool).unwrap_or(false) {
            let detail = v.pointer("/errors/0").or_else(|| v.get("result")).and_then(Value::as_str).unwrap_or("").to_string();
            return vec![Err(self.classify(&detail))];
        }
        let us = &v["usage"];
        let tokens = Tokens { input: u(us, "input_tokens"), cache_read: u(us, "cache_read_input_tokens"), cache_write: u(us, "cache_creation_input_tokens"), output: u(us, "output_tokens") };
        let mut out = vec![];
        let mut stop = StopReason::EndTurn;
        match &self.shape {
            Shape::Text => {
                let full = v.get("result").and_then(Value::as_str).unwrap_or("").to_string();
                out.extend(self.more_text(&full));
            }
            Shape::Step => {
                let Some(so) = v.get("structured_output").filter(|s| s.is_object()) else {
                    return vec![Err(ApiError::Retryable("Claude Code gave no answer in the expected shape. Send again to continue.".into()))];
                };
                out.extend(self.more_text(so.get("text").and_then(Value::as_str).unwrap_or("")));
                for (i, c) in so.get("tool_calls").and_then(Value::as_array).into_iter().flatten().enumerate() {
                    let Some(name) = c.get("name").and_then(Value::as_str) else { continue };
                    let args = c.get("args").cloned().filter(Value::is_object).unwrap_or_else(|| json!({}));
                    out.push(Ok(ModelEvent::ToolCall { id: format!("cc_{}_{i}", self.ids), name: name.into(), args }));
                    stop = StopReason::ToolUse;
                }
            }
            Shape::Forced(name) => match v.get("structured_output") {
                Some(so) if !so.is_null() => out.push(Ok(ModelEvent::ToolCall { id: format!("cc_{}_0", self.ids), name: name.clone(), args: so.clone() })),
                _ => return vec![Err(ApiError::Retryable("Claude Code gave no answer in the expected shape.".into()))],
            },
        }
        out.push(Ok(ModelEvent::Usage(tokens)));
        // Compaction needs the window; Claude Code gives it per model with each reply.
        if let Some(w) = v.get("modelUsage").and_then(Value::as_object).and_then(|m| m.values().filter_map(|x| x.get("contextWindow").and_then(Value::as_u64)).max()) {
            out.push(Ok(ModelEvent::Window(w)));
        }
        out.push(Ok(ModelEvent::Stop(stop)));
        out
    }

    fn classify(&self, detail: &str) -> ApiError {
        if let Some(until) = self.rate_limited {
            return ApiError::RateLimited { until };
        }
        let d = detail.to_lowercase();
        if d.contains("limit") && (d.contains("usage") || d.contains("rate")) {
            return ApiError::RateLimited { until: 0 };
        }
        if d.contains("login") || d.contains("log in") || d.contains("not logged") || d.contains("authenticat") || d.contains("oauth") {
            return ApiError::Auth(SIGNED_OUT.into());
        }
        eprintln!("bleep: claude call failed: {}", preview(detail, 300));
        ApiError::Retryable("Claude Code could not answer. Send again to continue.".into())
    }
}

/// Removes the call's system prompt file when the call ends.
struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[async_trait]
impl ModelAdapter for ClaudeCodeAdapter {
    async fn stream(&self, req: &ModelRequest) -> Result<ModelStream, ApiError> {
        let shape = shape(req);
        let schema = match &shape {
            Shape::Text => None,
            Shape::Step => Some(step_schema(&req.tools)),
            Shape::Forced(name) => Some(req.tools.iter().find(|t| &t.name == name).map(|t| t.schema.clone()).unwrap_or_else(|| json!({"type": "object"}))),
        };
        let file = TempFile(self.paths.run.join(format!("prompt-{}.md", &crate::mcp::new_token()[..16])));
        std::fs::write(&file.0, system_prompt(req)).map_err(|e| ApiError::Fatal(format!("bleep could not write a file in its data folder: {e}")))?;
        let log = std::fs::OpenOptions::new().create(true).append(true).open(self.paths.logs.join("claude.log")).map(Stdio::from).unwrap_or_else(|_| Stdio::null());
        let mut child = base_command(&self.bin, &self.paths)
            .args(call_args(&req.model, &file.0.display().to_string(), schema.as_ref()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(log)
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| ApiError::Fatal(format!("bleep could not start Claude Code: {e}")))?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let line = input_line(&req.msgs);
        if stdin.write_all(line.as_bytes()).await.is_err() {
            return Err(ApiError::Retryable(EXITED.into()));
        }
        // Closing stdin tells Claude Code this is the only message.
        drop(stdin);
        let idle = self.idle;
        let state = (child, file, BufReader::new(stdout).lines(), CallParser::new(shape), std::collections::VecDeque::new(), false);
        Ok(Box::pin(futures::stream::unfold(state, move |(child, file, mut lines, mut p, mut pending, mut done)| async move {
            loop {
                if let Some(e) = pending.pop_front() {
                    return Some((e, (child, file, lines, p, pending, done)));
                }
                if done {
                    return None;
                }
                match tokio::time::timeout(idle, lines.next_line()).await {
                    Err(_) => {
                        pending.push_back(Err(ApiError::Retryable(STALLED.into())));
                        done = true;
                    }
                    Ok(Ok(Some(l))) => {
                        let evs = p.feed(&l);
                        done = evs.iter().any(|e| matches!(e, Err(_) | Ok(ModelEvent::Stop(_))));
                        pending.extend(evs);
                    }
                    Ok(_) => {
                        pending.push_back(Err(ApiError::Retryable(EXITED.into())));
                        done = true;
                    }
                }
            }
        })))
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ApiError> {
        Ok(vec![])
    }
}

#[cfg(test)]
#[path = "adapter_tests.rs"]
mod tests;
