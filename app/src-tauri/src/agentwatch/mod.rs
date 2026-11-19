//! Agent watch (build plan 8.11): see my own Claude Code and Codex sessions that wait on
//! me, approve the safe asks my rules allow, and list the rest.
pub mod claude;
pub mod codex;
pub mod hub;
pub mod install;
pub mod rules;
pub mod screenshot;
pub mod socket;
pub mod transcript;

pub use crate::store::{AgentAsk, AgentSession, AgentState, AgentTool, AskDecision, AskKind, NewAsk};

use crate::paths::BleepPaths;
use serde_json::Value;
use std::path::Path;

/// One message from `bleep-hook`.
#[derive(Clone, Debug, PartialEq)]
pub struct HookMsg {
    pub source: String,
    pub event: String,
    pub wait: bool,
    pub payload: Value,
}

pub fn parse_hook(line: &str) -> Result<HookMsg, String> {
    let v: Value = serde_json::from_str(line.trim()).map_err(|e| format!("not JSON: {e}"))?;
    if v.get("v").and_then(Value::as_u64) != Some(1) {
        return Err("unknown version".into());
    }
    let text = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string).ok_or(format!("missing {k}"));
    let payload = v.get("payload").filter(|p| p.is_object()).cloned().ok_or("payload is not an object")?;
    Ok(HookMsg { source: text("source")?, event: text("event")?, wait: v.get("wait").and_then(Value::as_bool).unwrap_or(false), payload })
}

/// Copies the `bleep-hook` that ships next to bleep's own binary to `~/.bleep/bin`, where
/// the installed hooks point. Every build (prod, nightly, local) uses this one copy.
/// Returns true when it wrote a new copy.
pub fn install_hook_binary(paths: &BleepPaths) -> std::io::Result<bool> {
    let src = std::env::current_exe()?.with_file_name("bleep-hook");
    install_hook_binary_from(&src, paths)
}

pub fn install_hook_binary_from(src: &Path, paths: &BleepPaths) -> std::io::Result<bool> {
    use std::os::unix::fs::PermissionsExt;
    let new = std::fs::read(src)?;
    if std::fs::read(&paths.hook_bin).is_ok_and(|old| old == new) {
        return Ok(false);
    }
    if let Some(dir) = paths.hook_bin.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Write next to it and rename, so a hook running right now never sees half a file.
    let tmp = paths.hook_bin.with_extension("new");
    std::fs::write(&tmp, &new)?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    std::fs::rename(&tmp, &paths.hook_bin)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_binary_is_copied_once_and_executable() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let paths = BleepPaths::from_root(d.path().join("home"));
        paths.ensure().unwrap();
        let src = d.path().join("bleep-hook");
        std::fs::write(&src, b"v1").unwrap();
        assert!(install_hook_binary_from(&src, &paths).unwrap());
        assert_eq!(std::fs::read(&paths.hook_bin).unwrap(), b"v1");
        assert_eq!(std::fs::metadata(&paths.hook_bin).unwrap().permissions().mode() & 0o777, 0o755);
        assert!(!install_hook_binary_from(&src, &paths).unwrap());
        std::fs::write(&src, b"v2").unwrap();
        assert!(install_hook_binary_from(&src, &paths).unwrap());
        assert_eq!(std::fs::read(&paths.hook_bin).unwrap(), b"v2");
    }

    #[test]
    fn missing_sidecar_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        let paths = BleepPaths::from_root(d.path().into());
        paths.ensure().unwrap();
        assert!(install_hook_binary_from(&d.path().join("nope"), &paths).is_err());
    }

    #[test]
    fn paths_for_agent_watch() {
        let paths = BleepPaths::from_root("/r".into());
        assert_eq!(paths.sock, std::path::PathBuf::from("/r/run/bleep.sock"));
        assert_eq!(paths.hook_bin, std::path::PathBuf::from("/r/bin/bleep-hook"));
        assert_eq!(paths.agent_rules, std::path::PathBuf::from("/r/agent-rules.toml"));
    }

    #[test]
    fn parses_a_hook_line() {
        let m = parse_hook(r#"{"v":1,"source":"claude","event":"Stop","wait":true,"payload":{"a":1}}"#).unwrap();
        assert_eq!(m, HookMsg { source: "claude".into(), event: "Stop".into(), wait: true, payload: serde_json::json!({"a":1}) });
    }

    #[test]
    fn rejects_other_versions_and_non_objects() {
        assert!(parse_hook(r#"{"v":2,"source":"claude","event":"Stop","wait":true,"payload":{}}"#).is_err());
        assert!(parse_hook(r#"{"v":1,"source":"claude","event":"Stop","wait":true,"payload":[1]}"#).is_err());
        assert!(parse_hook("nope").is_err());
    }
}
