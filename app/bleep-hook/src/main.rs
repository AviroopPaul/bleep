//! bleep-hook: the tiny program that Claude Code hooks and Codex `notify` run.
//!
//! It forwards the hook JSON to bleep's socket (`$BLEEP_HOME/run/bleep.sock`, default
//! `~/.bleep/run/bleep.sock`) and, for the events that wait, prints bleep's reply.
//! It must never block or break a coding agent: on any problem it prints nothing and
//! exits 0, so the agent shows its normal prompt.
//!
//!   bleep-hook claude <Event>               payload on stdin
//!   bleep-hook claude-reply Stop            payload on stdin; an async-rewake hook
//!   bleep-hook codex-notify -- [prog args...] <json>
//!
//! `claude-reply` waits in the background for my reply from bleep. With a reply it
//! prints it on stderr and exits 2, which wakes the Claude Code session with it.
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT_MS: u64 = 20_000;
/// A permission ask or a reply can wait for me this long. Under the hook's own 3600 s.
const LONG_TIMEOUT_MS: u64 = 3_590_000;
const NOTIFY_WAIT: Duration = Duration::from_secs(5);
/// The Claude Code events whose answer changes what the agent does.
const WAITING_EVENTS: &[&str] = &["PermissionRequest", "Stop"];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("claude-reply") {
        if let Some(text) = reply(&args) {
            let _ = std::io::stderr().write_all(text.as_bytes());
            std::process::exit(2);
        }
        return;
    }
    if let Some(out) = run(&args) {
        let mut o = std::io::stdout();
        let _ = o.write_all(out.as_bytes());
        let _ = o.write_all(b"\n");
    }
}

fn run(args: &[String]) -> Option<String> {
    match args.first().map(String::as_str) {
        Some("claude") => {
            let event = args.get(1)?;
            let mut payload = String::new();
            std::io::stdin().read_to_string(&mut payload).ok()?;
            let wait = WAITING_EVENTS.contains(&event.as_str());
            let long = event == "PermissionRequest";
            send("claude", event, wait, &payload, timeout(long))
        }
        Some("codex-notify") => {
            let rest = match args.get(1).map(String::as_str) {
                Some("--") => &args[2..],
                _ => &args[1..],
            };
            let (payload, original) = rest.split_last()?;
            if let Some((prog, prog_args)) = original.split_first() {
                run_original(prog, prog_args, payload);
            }
            let event = json_type(payload).unwrap_or_else(|| "notify".into());
            send("codex-notify", &event, false, payload, timeout(false))
        }
        _ => None,
    }
}

/// `claude-reply <Event>`: my reply text, or `None` when I answered somewhere else.
fn reply(args: &[String]) -> Option<String> {
    let event = args.get(1)?;
    let mut payload = String::new();
    std::io::stdin().read_to_string(&mut payload).ok()?;
    let out = send("claude-reply", event, true, &payload, timeout(true))?;
    let text = json_string(out.strip_prefix("{\"rewake\":")?.strip_suffix('}')?)?;
    (!text.trim().is_empty()).then_some(text)
}

/// Decodes one JSON string literal (the whole input), without a JSON library.
fn json_string(s: &str) -> Option<String> {
    let body = s.trim().strip_prefix('"')?.strip_suffix('"')?;
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return None,
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{c}'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    let code = u32::from_str_radix(&hex, 16).ok()?;
                    let code = if (0xD800..0xDC00).contains(&code) {
                        // A surrogate pair: the low half must follow.
                        let rest: String = chars.by_ref().take(6).collect();
                        let low = u32::from_str_radix(rest.strip_prefix("\\u")?, 16).ok()?;
                        0x10000 + ((code - 0xD800) << 10) + (low.checked_sub(0xDC00)?)
                    } else {
                        code
                    };
                    out.push(char::from_u32(code)?);
                }
                _ => return None,
            },
            c => out.push(c),
        }
    }
    Some(out)
}

/// Runs the notify program Codex had before bleep, with the same arguments. Its result
/// does not matter; it gets a few seconds, then keeps running on its own.
fn run_original(prog: &str, args: &[String], payload: &str) {
    let Ok(mut child) = Command::new(prog).args(args).arg(payload).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else {
        return;
    };
    let t0 = Instant::now();
    while t0.elapsed() < NOTIFY_WAIT {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The value of the first `"type"` key, read without a JSON parser.
fn json_type(s: &str) -> Option<String> {
    let at = s.find("\"type\"")? + 6;
    let rest = s[at..].trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    let end = rest.find('"')?;
    let v = &rest[..end];
    v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_').then(|| v.to_string())
}

fn sock_path() -> Option<PathBuf> {
    let root = match std::env::var("BLEEP_HOME").ok().filter(|s| !s.is_empty()) {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from(std::env::var("HOME").ok()?).join(".bleep"),
    };
    Some(root.join("run").join("bleep.sock"))
}

fn timeout(long: bool) -> Duration {
    let ms = std::env::var("BLEEP_HOOK_TIMEOUT_MS").ok().and_then(|s| s.parse().ok()).unwrap_or(if long { LONG_TIMEOUT_MS } else { DEFAULT_TIMEOUT_MS });
    Duration::from_millis(ms)
}

/// Sends one line and, when `wait`, reads one line back. `None` means "no decision".
fn send(source: &str, event: &str, wait: bool, payload: &str, budget: Duration) -> Option<String> {
    let payload = payload.trim();
    // Only a JSON object goes on the wire. Raw newlines in JSON are whitespace only.
    if !(payload.starts_with('{') && payload.ends_with('}')) || !event.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return None;
    }
    let payload = payload.replace(['\n', '\r'], " ");
    let line = format!("{{\"v\":1,\"source\":\"{source}\",\"event\":\"{event}\",\"wait\":{wait},\"payload\":{payload}}}\n");
    let t0 = Instant::now();
    let mut s = UnixStream::connect(sock_path()?).ok()?;
    s.set_write_timeout(Some(budget)).ok()?;
    s.write_all(line.as_bytes()).ok()?;
    if !wait {
        return None;
    }
    let left = budget.checked_sub(t0.elapsed()).filter(|d| !d.is_zero())?;
    s.set_read_timeout(Some(left)).ok()?;
    let mut reply = String::new();
    BufReader::new(s).read_line(&mut reply).ok()?;
    let reply = reply.trim();
    (!reply.is_empty() && reply.starts_with('{')).then(|| reply.to_string())
}
