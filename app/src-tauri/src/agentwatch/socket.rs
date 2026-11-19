//! The Unix socket `bleep-hook` talks to. One line in, one line out (only when the hook
//! waits). A slow or failing handler answers with an empty line: no decision.
use super::{parse_hook, HookMsg};
use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Under `bleep-hook`'s own 20 s, so the reply always arrives in time.
pub const HANDLER_BUDGET: Duration = Duration::from_secs(18);
/// A permission ask or a reply waits for me. Under `bleep-hook`'s own 3590 s.
pub const WAIT_FOR_ME: Duration = Duration::from_secs(3580);

/// The messages whose answer is mine to give, so they may wait long.
pub fn waits_for_me(msg: &HookMsg) -> bool {
    matches!((msg.source.as_str(), msg.event.as_str()), ("claude", "PermissionRequest") | ("claude-reply", _))
}

#[async_trait]
pub trait HookHandler: Send + Sync {
    async fn handle(&self, msg: HookMsg) -> Option<Value>;
}

pub async fn serve(path: &Path, handler: Arc<dyn HookHandler>) -> std::io::Result<tokio::task::JoinHandle<()>> {
    serve_with(path, handler, HANDLER_BUDGET).await
}

/// One hook payload is small; a huge line is cut off rather than read into memory.
const MAX_LINE: u64 = 8 * 1024 * 1024;

unsafe extern "C" {
    fn getuid() -> u32;
}

pub async fn serve_with(path: &Path, handler: Arc<dyn HookHandler>, budget: Duration) -> std::io::Result<tokio::task::JoinHandle<()>> {
    use std::os::unix::fs::PermissionsExt;
    if path.exists() {
        // A live socket belongs to another bleep: leave it. A dead one is left over.
        if tokio::net::UnixStream::connect(path).await.is_ok() {
            return Err(std::io::Error::new(std::io::ErrorKind::AddrInUse, "another bleep is listening"));
        }
        std::fs::remove_file(path)?;
    }
    let listener = tokio::net::UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    // SAFETY: getuid has no preconditions and cannot fail.
    let me = unsafe { getuid() };
    Ok(tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { continue };
            // The 0600 socket in the 0700 run dir keeps other users out. The uid check is a
            // second guard; a fire-and-forget hook may already be gone, so only a known
            // different uid is refused.
            if stream.peer_cred().is_ok_and(|c| c.uid() != me) {
                continue;
            }
            let handler = handler.clone();
            tokio::spawn(async move { one(stream, handler, budget).await });
        }
    }))
}

async fn one(stream: tokio::net::UnixStream, handler: Arc<dyn HookHandler>, budget: Duration) {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    let (r, mut w) = stream.into_split();
    let mut line = String::new();
    if BufReader::new(r.take(MAX_LINE)).read_line(&mut line).await.is_err() {
        return;
    }
    // An empty connection is another bleep build checking whether this one runs.
    if line.trim().is_empty() {
        return;
    }
    let reply = match parse_hook(&line) {
        Ok(msg) => {
            let wait = msg.wait;
            let budget = if waits_for_me(&msg) { budget.max(WAIT_FOR_ME) } else { budget };
            let answer = tokio::time::timeout(budget, handler.handle(msg)).await.ok().flatten();
            if !wait {
                return;
            }
            answer
        }
        Err(e) => {
            eprintln!("bleep: hook message ignored: {e}");
            None
        }
    };
    let text = reply.map(|v| v.to_string()).unwrap_or_default();
    let _ = w.write_all(format!("{text}\n").as_bytes()).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    struct Rec {
        got: Mutex<Vec<HookMsg>>,
        reply: Option<Value>,
        delay: Duration,
    }

    #[async_trait]
    impl HookHandler for Rec {
        async fn handle(&self, msg: HookMsg) -> Option<Value> {
            self.got.lock().unwrap().push(msg);
            tokio::time::sleep(self.delay).await;
            self.reply.clone()
        }
    }

    fn rec(reply: Option<Value>, delay_ms: u64) -> Arc<Rec> {
        Arc::new(Rec { got: Default::default(), reply, delay: Duration::from_millis(delay_ms) })
    }

    async fn ask(path: &Path, line: &str) -> String {
        let s = UnixStream::connect(path).await.unwrap();
        let (r, mut w) = s.into_split();
        w.write_all(format!("{line}\n").as_bytes()).await.unwrap();
        let mut out = String::new();
        BufReader::new(r).read_line(&mut out).await.unwrap();
        out
    }

    const STOP: &str = r#"{"v":1,"source":"claude","event":"Stop","wait":true,"payload":{"session_id":"s1"}}"#;

    #[tokio::test]
    async fn round_trips_one_message() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("bleep.sock");
        let h = rec(Some(json!({"decision":"block","reason":"go"})), 0);
        serve(&p, h.clone()).await.unwrap();
        let out = ask(&p, STOP).await;
        assert_eq!(serde_json::from_str::<Value>(out.trim()).unwrap(), json!({"decision":"block","reason":"go"}));
        assert_eq!(h.got.lock().unwrap()[0].payload["session_id"], "s1");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[tokio::test]
    async fn no_decision_is_an_empty_line() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("bleep.sock");
        serve(&p, rec(None, 0)).await.unwrap();
        assert_eq!(ask(&p, STOP).await, "\n");
    }

    #[tokio::test]
    async fn handler_over_budget_replies_empty() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("bleep.sock");
        serve_with(&p, rec(Some(json!({"late":true})), 500), Duration::from_millis(50)).await.unwrap();
        let t0 = std::time::Instant::now();
        assert_eq!(ask(&p, STOP).await, "\n");
        assert!(t0.elapsed() < Duration::from_millis(400));
    }

    #[test]
    fn only_my_answers_wait_long() {
        let m = |source: &str, event: &str| HookMsg { source: source.into(), event: event.into(), wait: true, payload: json!({}) };
        assert!(waits_for_me(&m("claude", "PermissionRequest")));
        assert!(waits_for_me(&m("claude-reply", "Stop")));
        assert!(!waits_for_me(&m("claude", "Stop")));
        assert!(!waits_for_me(&m("codex-notify", "PermissionRequest")));
    }

    #[tokio::test]
    async fn bad_json_replies_empty() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("bleep.sock");
        let h = rec(Some(json!({"x":1})), 0);
        serve(&p, h.clone()).await.unwrap();
        assert_eq!(ask(&p, "garbage").await, "\n");
        assert!(h.got.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn fire_and_forget_still_reaches_the_handler() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("bleep.sock");
        let h = rec(Some(json!({"x":1})), 0);
        serve(&p, h.clone()).await.unwrap();
        let mut s = UnixStream::connect(&p).await.unwrap();
        s.write_all(b"{\"v\":1,\"source\":\"claude\",\"event\":\"PostToolUse\",\"wait\":false,\"payload\":{}}\n").await.unwrap();
        drop(s);
        for _ in 0..50 {
            if !h.got.lock().unwrap().is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("handler never ran");
    }

    #[tokio::test]
    async fn stale_socket_is_replaced() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("bleep.sock");
        drop(std::os::unix::net::UnixListener::bind(&p).unwrap());
        assert!(p.exists());
        serve(&p, rec(None, 0)).await.unwrap();
        assert_eq!(ask(&p, STOP).await, "\n");
    }

    #[tokio::test]
    async fn a_live_socket_is_not_taken_over() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("bleep.sock");
        serve(&p, rec(None, 0)).await.unwrap();
        assert!(serve(&p, rec(None, 0)).await.is_err());
    }
}
