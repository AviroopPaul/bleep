//! Claude Code backend. Claude Code is only the model: bleep's own loop (`api::agent::Core`)
//! sends it the conversation one step at a time through `ClaudeCodeAdapter`, runs the tools
//! itself and keeps the history in SQLite. No Claude Code session is ever resumed.
pub mod adapter;
pub mod args;

use crate::config::Tiers;
use crate::paths::BleepPaths;
use crate::runtime::api::agent::{ApiChoice, ApiRuntime, Core};
use crate::runtime::*;
use adapter::ClaudeCodeAdapter;
use args::*;
use std::path::PathBuf;
use std::sync::Arc;

pub const HINT_MISSING: &str = "Install Claude Code, then reopen bleep.";
pub const HINT_SIGNED_OUT: &str = "Sign in to Claude Code for bleep. It uses its own login, separate from your terminal.";
pub const HINT_OLD: &str = "Update Claude Code, then reopen bleep.";
pub const LABEL: &str = "Claude Code";

pub struct ClaudeRuntime {
    paths: BleepPaths,
    bin: PathBuf,
    core: ApiRuntime,
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
    pub fn new(paths: BleepPaths, tiers: Tiers, bin: PathBuf, store: Arc<crate::store::Store>) -> Self {
        Self::with_adapter(paths.clone(), tiers, bin.clone(), store, ClaudeCodeAdapter::new(bin, paths))
    }

    pub fn with_adapter(paths: BleepPaths, tiers: Tiers, bin: PathBuf, store: Arc<crate::store::Store>, adapter: ClaudeCodeAdapter) -> Self {
        let choice = ApiChoice { provider: "claude".into(), label: LABEL.into(), main: tiers.main, small: tiers.small, has_key: true, free: true };
        let core = Core::new(choice, Arc::new(adapter), store).local_agent();
        Self { paths, bin, core: ApiRuntime::new(core) }
    }

    async fn run_cli(&self, args: &[&str]) -> Option<String> {
        let out = base_command(&self.bin, &self.paths).args(args).output().await.ok()?;
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    }
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
        self.core.start_session(cfg).await
    }

    async fn send(&self, s: &SessionHandle, msg: UserMessage) -> anyhow::Result<EventStream> {
        self.core.send(s, msg).await
    }

    async fn one_shot(&self, req: OneShot) -> anyhow::Result<serde_json::Value> {
        self.core.one_shot(req).await
    }

    async fn one_shot_with_usage(&self, req: OneShot) -> anyhow::Result<(serde_json::Value, Option<ShotUsage>)> {
        self.core.one_shot_with_usage(req).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let dir = tempfile::tempdir().unwrap();
        let paths = BleepPaths::from_root(dir.path().into());
        let store = Arc::new(crate::store::Store::open(&dir.path().join("b.db")).unwrap());
        let rt = ClaudeRuntime::new(paths, Tiers::default(), PathBuf::from("/nonexistent/claude"), store);
        let h = rt.health().await;
        assert!(!h.installed);
        assert!(!h.ok());
        assert_eq!(h.hint.as_deref(), Some(HINT_MISSING));
    }
}
