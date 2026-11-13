//! Runs the built `bleep-hook` against a test socket. bleep-hook must never block or fail
//! a coding agent: on any problem it prints nothing and exits 0.
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_bleep-hook");

fn home() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("run")).unwrap();
    let sock = d.path().join("run/bleep.sock");
    (d, sock)
}

fn run(home: &Path, args: &[&str], stdin: &str, timeout_ms: Option<u64>) -> (Output, Duration) {
    let mut c = Command::new(BIN);
    c.args(args).env("BLEEP_HOME", home).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(t) = timeout_ms {
        c.env("BLEEP_HOOK_TIMEOUT_MS", t.to_string());
    }
    let t0 = Instant::now();
    let mut ch = c.spawn().unwrap();
    ch.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let out = ch.wait_with_output().unwrap();
    (out, t0.elapsed())
}

/// Accepts one connection, returns the request line, and answers with `reply` after `delay`.
fn serve_once(sock: &Path, reply: Option<&'static str>, delay: Duration) -> std::thread::JoinHandle<String> {
    let l = UnixListener::bind(sock).unwrap();
    l.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        let t0 = Instant::now();
        let s = loop {
            match l.accept() {
                Ok((s, _)) => break s,
                Err(_) if t0.elapsed() < Duration::from_secs(5) => std::thread::sleep(Duration::from_millis(10)),
                Err(_) => return String::new(),
            }
        };
        s.set_nonblocking(false).unwrap();
        let mut r = BufReader::new(s.try_clone().unwrap());
        let mut line = String::new();
        r.read_line(&mut line).unwrap();
        std::thread::sleep(delay);
        if let Some(rep) = reply {
            let mut w = s;
            let _ = w.write_all(format!("{rep}\n").as_bytes());
        }
        line
    })
}

#[test]
fn no_socket_prints_nothing_exits_zero() {
    let (d, _) = home();
    let (out, _) = run(d.path(), &["claude", "PermissionRequest"], r#"{"tool_name":"Bash"}"#, None);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
}

#[test]
fn stale_socket_file_prints_nothing() {
    let (d, sock) = home();
    drop(UnixListener::bind(&sock).unwrap()); // the file stays, nobody listens
    assert!(sock.exists());
    let (out, _) = run(d.path(), &["claude", "Stop"], "{}", None);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
}

#[test]
fn slow_server_gives_up_at_timeout() {
    let (d, sock) = home();
    let _srv = serve_once(&sock, Some(r#"{"late":true}"#), Duration::from_millis(1500));
    let (out, took) = run(d.path(), &["claude", "PermissionRequest"], "{}", Some(300));
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert!(took < Duration::from_millis(1200), "took {took:?}");
}

#[test]
fn prints_reply_verbatim() {
    let (d, sock) = home();
    let reply = r#"{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}}"#;
    let srv = serve_once(&sock, Some(reply), Duration::ZERO);
    let (out, _) = run(d.path(), &["claude", "PermissionRequest"], "{\"tool_name\":\"Bash\"}\n", None);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim_end(), reply);
    let req = srv.join().unwrap();
    assert_eq!(req.trim_end(), r#"{"v":1,"source":"claude","event":"PermissionRequest","wait":true,"payload":{"tool_name":"Bash"}}"#);
}

#[test]
fn empty_reply_prints_nothing() {
    let (d, sock) = home();
    let _srv = serve_once(&sock, Some(""), Duration::ZERO);
    let (out, _) = run(d.path(), &["claude", "Stop"], "{}", None);
    assert!(out.stdout.is_empty());
}

#[test]
fn fire_and_forget_does_not_wait() {
    let (d, sock) = home();
    let srv = serve_once(&sock, None, Duration::from_millis(3000));
    let (out, took) = run(d.path(), &["claude", "PostToolUse"], "{\"a\":1}", Some(5000));
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert!(took < Duration::from_millis(1500), "took {took:?}");
    assert!(srv.join().unwrap().contains(r#""wait":false,"payload":{"a":1}"#));
}

#[test]
fn bad_payload_is_not_sent() {
    let (d, sock) = home();
    let l = UnixListener::bind(&sock).unwrap();
    l.set_nonblocking(true).unwrap();
    let (out, _) = run(d.path(), &["claude", "Stop"], "not json at all", None);
    assert!(out.stdout.is_empty());
    assert_eq!(out.status.code(), Some(0));
    assert!(l.accept().is_err(), "nothing should connect");
}

#[test]
fn codex_notify_runs_original_then_forwards() {
    let (d, sock) = home();
    let marker = d.path().join("orig.txt");
    let script = d.path().join("orig.sh");
    std::fs::write(&script, format!("#!/bin/sh\necho \"$@\" > '{}'\n", marker.display())).unwrap();
    std::os::unix::fs::PermissionsExt::set_mode(&mut std::fs::metadata(&script).unwrap().permissions(), 0o755);
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let srv = serve_once(&sock, None, Duration::ZERO);
    let payload = r#"{"type":"agent-turn-complete","thread-id":"t1"}"#;
    let (out, _) = run(d.path(), &["codex-notify", "--", script.to_str().unwrap(), "turn-ended", payload], "", None);
    assert_eq!(out.status.code(), Some(0));
    let mut got = String::new();
    std::fs::File::open(&marker).unwrap().read_to_string(&mut got).unwrap();
    assert_eq!(got.trim_end(), format!("turn-ended {payload}"));
    let req = srv.join().unwrap();
    assert_eq!(req.trim_end(), format!(r#"{{"v":1,"source":"codex-notify","event":"agent-turn-complete","wait":false,"payload":{payload}}}"#));
}

#[test]
fn codex_notify_without_original_still_forwards() {
    let (d, sock) = home();
    let srv = serve_once(&sock, None, Duration::ZERO);
    let (out, _) = run(d.path(), &["codex-notify", "--", r#"{"type":"agent-turn-complete"}"#], "", None);
    assert_eq!(out.status.code(), Some(0));
    assert!(srv.join().unwrap().contains(r#""source":"codex-notify""#));
}

#[test]
fn unknown_mode_exits_zero() {
    let (d, _) = home();
    let (out, _) = run(d.path(), &["what"], "", None);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
}
