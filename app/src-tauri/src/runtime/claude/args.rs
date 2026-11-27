//! How bleep starts the `claude` CLI: its own config dir, a fixed working dir and a clean env.
//! What one model call passes is in `adapter.rs`.
use crate::paths::BleepPaths;
use std::path::Path;

pub const MIN_VERSION: (u64, u64, u64) = (2, 1, 0);
/// Env vars that must never reach bleep's Claude: an API key would bill per token
/// instead of using the subscription, and the CLAUDECODE markers come from a parent session.
pub const SCRUBBED_ENV: &[&str] = &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT"];

/// `claude auth status --json` -> (signed in, plan).
pub fn parse_auth_status(json: &str) -> (bool, Option<String>) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else { return (false, None) };
    let signed_in = v.get("loggedIn").and_then(|x| x.as_bool()).unwrap_or(false);
    (signed_in, v.get("subscriptionType").and_then(|x| x.as_str()).map(String::from))
}

/// Leading semver of `claude --version` output, for example "2.1.319 (Claude Code)".
pub fn parse_version(out: &str) -> Option<String> {
    let v = out.split_whitespace().next()?;
    (v.split('.').count() == 3 && v.split('.').all(|p| p.parse::<u64>().is_ok())).then(|| v.to_string())
}

pub fn version_ok(v: &str) -> bool {
    let p: Vec<u64> = v.split('.').filter_map(|x| x.parse().ok()).collect();
    p.len() == 3 && (p[0], p[1], p[2]) >= MIN_VERSION
}

/// A `claude` command with bleep's own config dir, fixed working dir and a clean env.
pub fn base_command(bin: &Path, paths: &BleepPaths) -> tokio::process::Command {
    let mut c = tokio::process::Command::new(bin);
    c.env("CLAUDE_CONFIG_DIR", &paths.claude_home).current_dir(&paths.claude_work);
    for k in SCRUBBED_ENV {
        c.env_remove(k);
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn auth_status_signed_out() {
        assert_eq!(parse_auth_status(r#"{"loggedIn":false,"authMethod":"none"}"#), (false, None));
    }

    #[test]
    fn auth_status_signed_in() {
        assert_eq!(parse_auth_status(r#"{"loggedIn":true,"subscriptionType":"pro"}"#), (true, Some("pro".into())));
    }

    #[test]
    fn auth_status_garbage_is_signed_out() {
        assert_eq!(parse_auth_status("oops"), (false, None));
    }

    #[test]
    fn version_parse_and_floor() {
        assert_eq!(parse_version("2.1.319 (Claude Code)").as_deref(), Some("2.1.319"));
        assert!(version_ok("2.1.319"));
        assert!(version_ok("3.0.0"));
        assert!(!version_ok("2.0.99"));
        assert!(!version_ok("junk"));
    }

    #[test]
    fn command_env_isolates_claude() {
        let paths = BleepPaths::from_root(PathBuf::from("/x"));
        let cmd = base_command(&PathBuf::from("claude"), &paths);
        let envs: Vec<_> = cmd.as_std().get_envs().map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned()))).collect();
        assert!(envs.contains(&("CLAUDE_CONFIG_DIR".into(), Some("/x/claude-home".into()))));
        assert!(envs.contains(&("ANTHROPIC_API_KEY".into(), None)), "API key must never reach bleep's Claude");
        assert_eq!(cmd.as_std().get_current_dir(), Some(std::path::Path::new("/x/claude-work")));
    }
}
