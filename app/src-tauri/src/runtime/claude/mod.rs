//! Claude Code backend: one warm `claude` child for the main chat, stream-json over stdio.
pub mod args;
pub mod parse;

use crate::config::Tiers;
use crate::paths::BleepPaths;
use crate::runtime::*;
use args::*;
use parse::ClaudeParser;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::mpsc;

pub const HINT_MISSING: &str = "Install Claude Code, then reopen bleep.";
pub const HINT_SIGNED_OUT: &str = "Sign in to Claude Code for bleep. It uses its own login, separate from your terminal.";
pub const HINT_OLD: &str = "Update Claude Code, then reopen bleep.";
pub const STALLED: &str = "Claude Code stopped responding. Send again to continue.";
pub const LOST: &str = "bleep lost the connection to Claude Code. Send again to continue.";
/// How long a resumed child gets to report a bad session id before we call it healthy.
const RESUME_CHECK: Duration = Duration::from_millis(1500);
const IDLE_TIMEOUT: Duration = Duration::from_secs(120);
const STALE_MARK: &str = "No conversation found";

struct Warm {
    _child: Child,
    stdin: ChildStdin,
    lines: mpsc::UnboundedReceiver<String>,
    /// Lines read while checking the resume, replayed on the next turn.
    early: VecDeque<String>,
}

pub struct ClaudeRuntime {
    paths: BleepPaths,
    tiers: Tiers,
    bin: PathBuf,
    warm: Arc<tokio::sync::Mutex<Option<Warm>>>,
    cfg: Mutex<Option<SessionConfig>>,
    last_sid: Arc<Mutex<Option<String>>>,
    idle: Duration,
    /// Above zero while an approval card waits on the user; the idle timer pauses.
    hold: Option<Arc<AtomicUsize>>,
}

/// Find `claude` the way a login shell does; a GUI app does not get the terminal PATH.
pub fn find_claude() -> Option<PathBuf> {
    let out = std::process::Command::new("/bin/zsh").args(["-lc", "command -v claude"]).output();
    let stdout = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    pick_claude(&stdout, &claude_fallbacks())
}

/// The last non-empty line of the shell's output if it is a real path (profiles may print
/// banners first), else the first install location that exists.
pub fn pick_claude(shell_out: &str, fallbacks: &[PathBuf]) -> Option<PathBuf> {
    shell_out
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute() && p.exists())
        .or_else(|| fallbacks.iter().find(|p| p.exists()).cloned())
}

fn claude_fallbacks() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    vec![
        home.join(".local/bin/claude"),
        home.join(".claude/local/claude"),
        "/opt/homebrew/bin/claude".into(),
        "/usr/local/bin/claude".into(),
    ]
}

impl ClaudeRuntime {
    /// A turn with no output for this long is ended and the child replaced (default 120 s).
    pub fn with_idle_timeout(mut self, d: Duration) -> Self {
        self.idle = d;
        self
    }

    pub fn with_hold(mut self, hold: Arc<AtomicUsize>) -> Self {
        self.hold = Some(hold);
        self
    }

    pub fn new(paths: BleepPaths, tiers: Tiers, bin: PathBuf) -> Self {
        Self { paths, tiers, bin, warm: Default::default(), cfg: Mutex::new(None), last_sid: Default::default(), idle: IDLE_TIMEOUT, hold: None }
    }

    fn spawn(&self, cfg: &SessionConfig) -> anyhow::Result<Warm> {
        let files = write_run_files(&self.paths, cfg)?;
        let log = std::fs::OpenOptions::new().create(true).append(true).open(self.paths.logs.join("claude.log"))?;
        let mut child = base_command(&self.bin, &self.paths)
            .args(chat_args(cfg, &files, &self.tiers))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log))
            .kill_on_drop(true)
            .spawn()?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut r = BufReader::new(stdout).lines();
            while let Ok(Some(l)) = r.next_line().await {
                if tx.send(l).is_err() {
                    break;
                }
            }
        });
        Ok(Warm { _child: child, stdin, lines: rx, early: VecDeque::new() })
    }

    /// Spawn for `cfg`. When resuming, wait briefly: a stale id fails within about 0.5 s.
    async fn spawn_checked(&self, cfg: &SessionConfig) -> anyhow::Result<Warm> {
        let mut w = self.spawn(cfg)?;
        if cfg.resume_id.is_some() {
            if let Ok(first) = tokio::time::timeout(RESUME_CHECK, w.lines.recv()).await {
                let Some(line) = first else { anyhow::bail!("Claude Code exited at start") };
                if let Some(AgentEvent::Error { message, .. }) = ClaudeParser::default().feed(&line).into_iter().next() {
                    if message.contains(STALE_MARK) {
                        return Err(StaleResume(message).into());
                    }
                    anyhow::bail!(message);
                }
                w.early.push_back(line);
            }
        }
        Ok(w)
    }

    async fn run_cli(&self, args: &[&str]) -> Option<String> {
        let out = base_command(&self.bin, &self.paths).args(args).output().await.ok()?;
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

fn finish_with(message: &str) -> Vec<AgentEvent> {
    vec![AgentEvent::Error { message: message.into(), retryable: true }, AgentEvent::Done { runtime_session_id: None }]
}

fn finish_lost() -> Vec<AgentEvent> {
    finish_with(LOST)
}

/// One turn's read state. Dropped before Done (abort, panic, stall), it drops the child too,
/// so the old turn's remaining output can never reach the next turn.
struct Turn {
    guard: tokio::sync::OwnedMutexGuard<Option<Warm>>,
    parser: ClaudeParser,
    pending: VecDeque<AgentEvent>,
    done: bool,
}

impl Drop for Turn {
    fn drop(&mut self) {
        if !self.done {
            *self.guard = None;
        }
    }
}

async fn write_line(w: &mut Warm, line: &str) -> std::io::Result<()> {
    w.stdin.write_all(line.as_bytes()).await?;
    w.stdin.flush().await
}

#[async_trait]
impl AgentRuntime for ClaudeRuntime {
    fn id(&self) -> &'static str {
        "claude"
    }

    async fn health(&self) -> RuntimeHealth {
        let mut h = RuntimeHealth { runtime: "claude".into(), installed: false, version: None, version_ok: false, signed_in: false, plan: None, hint: Some(HINT_MISSING.into()) };
        let Some(v) = self.run_cli(&["--version"]).await else { return h };
        h.installed = true;
        h.version = parse_version(&v);
        h.version_ok = h.version.as_deref().is_some_and(version_ok);
        if !h.version_ok {
            h.hint = Some(HINT_OLD.into());
            return h;
        }
        let (signed_in, plan) = parse_auth_status(&self.run_cli(&["auth", "status", "--json"]).await.unwrap_or_default());
        h.signed_in = signed_in;
        h.plan = plan;
        h.hint = (!signed_in).then(|| HINT_SIGNED_OUT.into());
        h
    }

    async fn start_session(&self, cfg: SessionConfig) -> anyhow::Result<SessionHandle> {
        let w = self.spawn_checked(&cfg).await?;
        *self.warm.lock().await = Some(w);
        *self.last_sid.lock().unwrap() = cfg.resume_id.clone();
        *self.cfg.lock().unwrap() = Some(cfg.clone());
        Ok(SessionHandle { resume_id: cfg.resume_id })
    }

    async fn send(&self, _s: &SessionHandle, msg: UserMessage) -> anyhow::Result<EventStream> {
        let mut guard = self.warm.clone().lock_owned().await;
        if let Some(w) = guard.as_mut() {
            // Lines after the previous turn's result are leftovers, never part of this turn.
            while w.lines.try_recv().is_ok() {}
            // A child that died while idle is replaced quietly, so the message is not lost.
            if w.lines.is_closed() {
                *guard = None;
            }
        }
        let line = serde_json::json!({"type": "user", "message": {"role": "user", "content": msg.text}}).to_string() + "\n";
        let mut attempts = 0;
        loop {
            if guard.is_none() {
                let mut cfg = self.cfg.lock().unwrap().clone().ok_or_else(|| anyhow::anyhow!("no session started"))?;
                cfg.resume_id = self.last_sid.lock().unwrap().clone();
                *guard = Some(self.spawn_checked(&cfg).await?);
            }
            attempts += 1;
            match write_line(guard.as_mut().unwrap(), &line).await {
                Ok(()) => break,
                Err(_) if attempts < 2 => *guard = None,
                Err(_) => {
                    *guard = None;
                    return Ok(Box::pin(futures::stream::iter(finish_lost())));
                }
            }
        }
        let last_sid = self.last_sid.clone();
        let idle = self.idle;
        let hold = self.hold.clone();
        let turn = Turn { guard, parser: ClaudeParser::default(), pending: VecDeque::new(), done: false };
        Ok(Box::pin(futures::stream::unfold(turn, move |mut st| {
            let last_sid = last_sid.clone();
            let hold = hold.clone();
            async move {
                loop {
                    if let Some(e) = st.pending.pop_front() {
                        return Some((e, st));
                    }
                    if st.done {
                        return None;
                    }
                    let Some(w) = st.guard.as_mut() else { return None };
                    let next = match w.early.pop_front() {
                        Some(l) => Ok(Some(l)),
                        // Silence while an approval card waits on the user is not a stall.
                        None => loop {
                            match tokio::time::timeout(idle, w.lines.recv()).await {
                                Err(_) if hold.as_ref().is_some_and(|h| h.load(std::sync::atomic::Ordering::SeqCst) > 0) => continue,
                                r => break r,
                            }
                        },
                    };
                    match next {
                        Err(_) => {
                            *st.guard = None;
                            st.pending.extend(finish_with(STALLED));
                            st.done = true;
                        }
                        Ok(None) => {
                            *st.guard = None;
                            st.pending.extend(finish_lost());
                            st.done = true;
                        }
                        Ok(Some(l)) => {
                            for e in st.parser.feed(&l) {
                                if let AgentEvent::Done { runtime_session_id: Some(id) } = &e {
                                    *last_sid.lock().unwrap() = Some(id.clone());
                                }
                                st.done |= matches!(e, AgentEvent::Done { .. });
                                st.pending.push_back(e);
                            }
                        }
                    }
                }
            }
        })))
    }

    async fn one_shot(&self, req: OneShot) -> anyhow::Result<serde_json::Value> {
        let model = match req.tier {
            Tier::Main => &self.tiers.main,
            Tier::Small => &self.tiers.small,
        };
        let out = base_command(&self.bin, &self.paths).args(one_shot_args(&req.prompt, &req.schema, model)).output().await?;
        let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
        if v.get("is_error").and_then(|x| x.as_bool()).unwrap_or(true) {
            anyhow::bail!("one-shot failed: {}", v.pointer("/errors/0").or_else(|| v.get("result")).unwrap_or(&v));
        }
        v.get("structured_output").cloned().ok_or_else(|| anyhow::anyhow!("no structured_output"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Tiers;
    use crate::paths::BleepPaths;
    use crate::runtime::StaleResume;
    use futures::StreamExt;
    use std::path::PathBuf;

    fn cfg() -> SessionConfig {
        SessionConfig {
            system_prompt: "p".into(),
            mcp: McpEndpoint { url: "http://127.0.0.1:1/mcp".into(), token: "t".into() },
            tier: Tier::Main,
            resume_id: None,
        }
    }

    fn rt(bin: PathBuf) -> (tempfile::TempDir, ClaudeRuntime) {
        let dir = tempfile::tempdir().unwrap();
        let paths = BleepPaths::from_root(dir.path().into());
        paths.ensure().unwrap();
        (dir, ClaudeRuntime::new(paths, Tiers::default(), bin))
    }

    #[tokio::test]
    async fn child_death_mid_turn() {
        let bin = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude/die_mid_turn.sh");
        let (_d, rt) = rt(bin);
        let h = rt.start_session(cfg()).await.unwrap();
        let ev: Vec<_> = rt.send(&h, UserMessage { text: "hi".into() }).await.unwrap().collect().await;
        let n = ev.len();
        assert!(n >= 2, "{ev:?}");
        assert!(matches!(&ev[n - 2], AgentEvent::Error { retryable: true, .. }), "{ev:?}");
        assert_eq!(ev[n - 1], AgentEvent::Done { runtime_session_id: None });
    }

    #[tokio::test]
    async fn stale_resume_fails_start() {
        let bin = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude/bad_resume.sh");
        let (_d, rt) = rt(bin);
        let mut c = cfg();
        c.resume_id = Some("00000000-0000-0000-0000-000000000000".into());
        let err = rt.start_session(c).await.err().expect("stale resume must fail");
        assert!(err.to_string().contains("No conversation found"), "{err}");
        assert!(rt.start_session(cfg()).await.is_ok(), "fresh start works");
    }

    fn fixture_bin(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude").join(name)
    }

    async fn collect(s: EventStream) -> Vec<AgentEvent> {
        tokio::time::timeout(std::time::Duration::from_secs(5), s.collect::<Vec<_>>()).await.expect("turn did not end")
    }

    #[tokio::test]
    async fn stale_resume_is_typed() {
        let (_d, rt) = rt(fixture_bin("bad_resume.sh"));
        let mut c = cfg();
        c.resume_id = Some("00000000-0000-0000-0000-000000000000".into());
        let err = rt.start_session(c).await.err().unwrap();
        assert!(err.downcast_ref::<StaleResume>().is_some(), "{err}");
    }

    #[tokio::test]
    async fn stray_line_after_done_does_not_leak() {
        let (_d, rt) = rt(fixture_bin("turn_then_stray.sh"));
        let h = rt.start_session(cfg()).await.unwrap();
        let one = collect(rt.send(&h, UserMessage { text: "a".into() }).await.unwrap()).await;
        crate::runtime::contract::assert_text_turn(&one, "hi there");
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let two = collect(rt.send(&h, UserMessage { text: "b".into() }).await.unwrap()).await;
        crate::runtime::contract::assert_text_turn(&two, "hi there");
    }

    #[tokio::test]
    async fn dropped_stream_restarts_child() {
        let (d, rt) = rt(fixture_bin("hang_mid_turn.sh"));
        let h = rt.start_session(cfg()).await.unwrap();
        {
            let mut s = rt.send(&h, UserMessage { text: "a".into() }).await.unwrap();
            assert!(s.next().await.is_some());
        }
        let mut s = rt.send(&h, UserMessage { text: "b".into() }).await.unwrap();
        assert!(s.next().await.is_some());
        let starts = std::fs::read_to_string(d.path().join("claude-home/starts")).unwrap();
        assert_eq!(starts.lines().count(), 2, "the half-finished child was replaced");
    }

    #[tokio::test]
    async fn idle_timeout_ends_turn() {
        let (_d, rt) = rt(fixture_bin("hang_mid_turn.sh"));
        let rt = rt.with_idle_timeout(std::time::Duration::from_millis(300));
        let h = rt.start_session(cfg()).await.unwrap();
        let ev = collect(rt.send(&h, UserMessage { text: "a".into() }).await.unwrap()).await;
        let n = ev.len();
        assert!(matches!(&ev[n - 2], AgentEvent::Error { retryable: true, message } if message == STALLED), "{ev:?}");
        assert_eq!(ev[n - 1], AgentEvent::Done { runtime_session_id: None });
    }

    #[tokio::test]
    async fn held_turn_does_not_stall() {
        use std::sync::atomic::Ordering;
        let hold = Arc::new(AtomicUsize::new(1));
        let (_d, rt) = rt(fixture_bin("hang_mid_turn.sh"));
        let rt = rt.with_idle_timeout(std::time::Duration::from_millis(300)).with_hold(hold.clone());
        let h = rt.start_session(cfg()).await.unwrap();
        let mut s = rt.send(&h, UserMessage { text: "a".into() }).await.unwrap();
        let mut seen = vec![];
        let waited = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while let Some(e) = s.next().await {
                seen.push(e);
            }
        })
        .await;
        assert!(waited.is_err(), "the turn ended while an approval was waiting: {seen:?}");
        hold.store(0, Ordering::SeqCst);
        let rest = collect(s).await;
        assert!(rest.iter().any(|e| matches!(e, AgentEvent::Error { message, .. } if message.contains(STALLED))), "{rest:?}");
    }

    #[tokio::test]
    async fn dead_idle_child_is_respawned() {
        let (_d, rt) = rt(fixture_bin("die_once.sh"));
        let h = rt.start_session(cfg()).await.unwrap();
        // Wait for the real exit, not a fixed time: a busy machine starts `sh` slowly.
        for _ in 0..500 {
            if rt.warm.lock().await.as_ref().is_some_and(|w| w.lines.is_closed()) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let ev = collect(rt.send(&h, UserMessage { text: "a".into() }).await.unwrap()).await;
        crate::runtime::contract::assert_text_turn(&ev, "hi there");
    }

    #[test]
    fn pick_claude_takes_last_path_line_then_fallbacks() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("claude");
        std::fs::write(&real, "").unwrap();
        let out = format!("Welcome to zsh!\n{}\n", real.display());
        assert_eq!(pick_claude(&out, &[]), Some(real.clone()));
        assert_eq!(pick_claude("noise\n", &[PathBuf::from("/nope/claude"), real.clone()]), Some(real.clone()));
        assert_eq!(pick_claude("", &[PathBuf::from("/nope/claude")]), None);
    }

    #[tokio::test]
    async fn missing_binary_health() {
        let (_d, rt) = rt(PathBuf::from("/nonexistent/claude"));
        let h = rt.health().await;
        assert!(!h.installed);
        assert!(!h.ok());
        assert_eq!(h.hint.as_deref(), Some(HINT_MISSING));
    }
}
