//! Everything about how bleep calls the `claude` CLI, in one place (build plan 8.1).
use crate::config::Tiers;
use crate::paths::BleepPaths;
use crate::runtime::{SessionConfig, Tier};
use std::path::{Path, PathBuf};

/// Server-level allow for bleep's MCP tools. Confirmed against a real turn in Task 7.
pub const ALLOWED_TOOLS: &str = "mcp__bleep";
pub const MIN_VERSION: (u64, u64, u64) = (2, 1, 0);
/// Env vars that must never reach bleep's Claude: an API key would bill per token
/// instead of using the subscription, and the CLAUDECODE markers come from a parent session.
pub const MCP_TOOL_TIMEOUT_MS: &str = "660000";
pub const SCRUBBED_ENV: &[&str] = &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT"];

pub struct RunFiles {
    pub mcp_config: PathBuf,
    pub system_prompt: PathBuf,
}

fn model<'a>(tier: Tier, tiers: &'a Tiers) -> &'a str {
    match tier {
        Tier::Main => &tiers.main,
        Tier::Small => &tiers.small,
    }
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

pub fn chat_args(cfg: &SessionConfig, files: &RunFiles, tiers: &Tiers) -> Vec<String> {
    let mut a = s(&[
        "-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose",
        "--include-partial-messages", "--tools", "", "--setting-sources", "", "--strict-mcp-config",
    ]);
    a.extend(s(&["--model", model(cfg.tier, tiers)]));
    a.extend(["--mcp-config".into(), files.mcp_config.display().to_string()]);
    a.extend(["--allowedTools".into(), ALLOWED_TOOLS.into()]);
    a.extend(["--append-system-prompt-file".into(), files.system_prompt.display().to_string()]);
    if let Some(id) = &cfg.resume_id {
        a.extend(["--resume".into(), id.clone()]);
    }
    a
}

pub fn one_shot_args(prompt: &str, schema: &serde_json::Value, model: &str) -> Vec<String> {
    let mut a = vec!["-p".to_string(), prompt.to_string()];
    a.extend(s(&["--output-format", "json", "--json-schema"]));
    a.push(schema.to_string());
    a.extend(s(&["--model", model, "--tools", "", "--setting-sources", "", "--strict-mcp-config", "--no-session-persistence"]));
    a
}

/// A one-shot that carries images: the prompt and images go in one stream-json line on
/// stdin (`--input-format stream-json` needs `--output-format stream-json`).
pub fn one_shot_image_args(schema: &serde_json::Value, model: &str) -> Vec<String> {
    let mut a = s(&["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose", "--json-schema"]);
    a.push(schema.to_string());
    a.extend(s(&["--model", model, "--tools", "", "--setting-sources", "", "--strict-mcp-config", "--no-session-persistence"]));
    a
}

pub fn one_shot_image_input(prompt: &str, images: &[crate::runtime::ImageIn]) -> String {
    let mut content = vec![serde_json::json!({"type": "text", "text": prompt})];
    content.extend(images.iter().map(|i| serde_json::json!({"type": "image", "source": {"type": "base64", "media_type": i.media_type, "data": i.data_b64}})));
    serde_json::json!({"type": "user", "message": {"role": "user", "content": content}}).to_string()
}

/// The `--mcp-config` body for bleep's server.
/// `timeout` lifts Claude Code's 300 s limit on a silent tool call for this server only:
/// a write tool waits up to 10 minutes on an approval card.
pub fn mcp_config_json(mcp: &crate::runtime::McpEndpoint) -> serde_json::Value {
    serde_json::json!({"mcpServers": {"bleep": {
        "type": "http",
        "url": mcp.url,
        "headers": {"Authorization": format!("Bearer {}", mcp.token)},
        "timeout": MCP_TOOL_TIMEOUT_MS.parse::<u64>().expect("number"),
    }}})
}

pub fn write_run_files(paths: &BleepPaths, cfg: &SessionConfig) -> anyhow::Result<RunFiles> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mcp_config = paths.run.join("mcp.json");
    let body = mcp_config_json(&cfg.mcp);
    let _ = std::fs::remove_file(&mcp_config);
    std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&mcp_config)?.write_all(body.to_string().as_bytes())?;
    let system_prompt = paths.run.join("system-prompt.md");
    std::fs::write(&system_prompt, &cfg.system_prompt)?;
    Ok(RunFiles { mcp_config, system_prompt })
}

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
    // An approval card may wait up to 10 minutes; the MCP call must outlive it.
    c.env("MCP_TOOL_TIMEOUT", MCP_TOOL_TIMEOUT_MS);
    for k in SCRUBBED_ENV {
        c.env_remove(k);
    }
    c
}

/// The chat process: `base_command`, with Claude Code's own compaction off, because bleep
/// compacts the chat itself (build plan D9). Found in Claude Code 2.1.325.
pub fn chat_command(bin: &Path, paths: &BleepPaths) -> tokio::process::Command {
    let mut c = base_command(bin, paths);
    c.env("DISABLE_AUTO_COMPACT", "1");
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_chat_turns_off_auto_compact() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::BleepPaths::from_root(dir.path().into());
        let env = |c: &tokio::process::Command| c.as_std().get_envs().map(|(k, v)| (k.to_os_string(), v.map(|v| v.to_os_string()))).collect::<Vec<_>>();
        assert!(env(&chat_command(Path::new("/bin/echo"), &paths)).contains(&("DISABLE_AUTO_COMPACT".into(), Some("1".into()))));
        assert!(env(&chat_command(Path::new("/bin/echo"), &paths)).contains(&("MCP_TOOL_TIMEOUT".into(), Some("660000".into()))));
        assert!(!env(&base_command(Path::new("/bin/echo"), &paths)).iter().any(|(k, _)| k == "DISABLE_AUTO_COMPACT"));
    }

    #[test]
    fn child_env_sets_mcp_tool_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::paths::BleepPaths::from_root(dir.path().into());
        let c = base_command(Path::new("/bin/echo"), &paths);
        let envs: Vec<_> = c.as_std().get_envs().map(|(k, v)| (k.to_os_string(), v.map(|v| v.to_os_string()))).collect();
        assert!(envs.contains(&("MCP_TOOL_TIMEOUT".into(), Some("660000".into()))), "{envs:?}");
    }
    use crate::config::Tiers;
    use crate::paths::BleepPaths;
    use crate::runtime::{McpEndpoint, SessionConfig, Tier};
    use std::path::PathBuf;

    fn cfg(resume: Option<&str>) -> SessionConfig {
        SessionConfig {
            system_prompt: "You are bleep.".into(),
            mcp: McpEndpoint { url: "http://127.0.0.1:5555/mcp".into(), token: "tok".into() },
            tier: Tier::Main,
            resume_id: resume.map(String::from),
        }
    }

    fn files() -> RunFiles {
        RunFiles { mcp_config: "/r/mcp.json".into(), system_prompt: "/r/system-prompt.md".into() }
    }

    fn after<'a>(a: &'a [String], flag: &str) -> &'a str {
        let i = a.iter().position(|x| x == flag).unwrap_or_else(|| panic!("missing {flag}"));
        &a[i + 1]
    }

    #[test]
    fn chat_args_exact() {
        let a = chat_args(&cfg(Some("abc")), &files(), &Tiers::default());
        for f in ["-p", "--verbose", "--include-partial-messages", "--strict-mcp-config"] {
            assert!(a.iter().any(|x| x == f), "missing {f}");
        }
        assert_eq!(after(&a, "--input-format"), "stream-json");
        assert_eq!(after(&a, "--output-format"), "stream-json");
        assert_eq!(after(&a, "--tools"), "");
        assert_eq!(after(&a, "--setting-sources"), "");
        assert_eq!(after(&a, "--model"), "sonnet");
        assert_eq!(after(&a, "--mcp-config"), "/r/mcp.json");
        assert_eq!(after(&a, "--append-system-prompt-file"), "/r/system-prompt.md");
        assert_eq!(after(&a, "--allowedTools"), ALLOWED_TOOLS);
        assert_eq!(after(&a, "--resume"), "abc");
        assert!(!chat_args(&cfg(None), &files(), &Tiers::default()).iter().any(|x| x == "--resume"));
    }

    #[test]
    fn small_tier_uses_small_model() {
        let mut c = cfg(None);
        c.tier = Tier::Small;
        assert_eq!(after(&chat_args(&c, &files(), &Tiers::default()), "--model"), "haiku");
    }

    #[test]
    fn one_shot_with_image_builds_stream_json() {
        let a = one_shot_image_args(&serde_json::json!({"type":"object"}), "haiku");
        assert_eq!(a[0], "-p");
        assert_eq!(after(&a, "--input-format"), "stream-json");
        assert_eq!(after(&a, "--output-format"), "stream-json");
        assert!(a.iter().any(|x| x == "--verbose"));
        assert_eq!(after(&a, "--json-schema"), r#"{"type":"object"}"#);
        assert_eq!(after(&a, "--model"), "haiku");
        assert_eq!(after(&a, "--tools"), "");
        assert!(a.iter().any(|x| x == "--no-session-persistence") && a.iter().any(|x| x == "--strict-mcp-config"));
        let img = crate::runtime::ImageIn { media_type: "image/png".into(), data_b64: "AAAA".into() };
        let line = one_shot_image_input("Compare.", &[img]);
        assert!(!line.contains('\n'));
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["type"], "user");
        assert_eq!(v["message"]["content"][0], serde_json::json!({"type": "text", "text": "Compare."}));
        assert_eq!(v["message"]["content"][1], serde_json::json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AAAA"}}));
    }

    #[test]
    fn one_shot_args_exact() {
        let a = one_shot_args("Is 7 prime?", &serde_json::json!({"type":"object"}), "haiku");
        assert_eq!(a[0], "-p");
        assert_eq!(a[1], "Is 7 prime?");
        assert_eq!(after(&a, "--output-format"), "json");
        assert_eq!(after(&a, "--json-schema"), r#"{"type":"object"}"#);
        assert_eq!(after(&a, "--tools"), "");
        assert!(a.iter().any(|x| x == "--no-session-persistence"));
    }

    #[test]
    fn run_files_have_token_and_private_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let paths = BleepPaths::from_root(dir.path().into());
        paths.ensure().unwrap();
        let f = write_run_files(&paths, &cfg(None)).unwrap();
        let mcp: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&f.mcp_config).unwrap()).unwrap();
        assert_eq!(mcp["mcpServers"]["bleep"]["type"], "http");
        assert_eq!(mcp["mcpServers"]["bleep"]["url"], "http://127.0.0.1:5555/mcp");
        assert_eq!(mcp["mcpServers"]["bleep"]["headers"]["Authorization"], "Bearer tok");
        // Claude Code drops a silent tool call after 300 s unless the server sets its own
        // timeout; an approval card may wait 10 minutes.
        assert_eq!(mcp["mcpServers"]["bleep"]["timeout"], 660_000);
        assert_eq!(mcp, mcp_config_json(&cfg(None).mcp));
        assert_eq!(std::fs::metadata(&f.mcp_config).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::read_to_string(&f.system_prompt).unwrap(), "You are bleep.");
    }

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
