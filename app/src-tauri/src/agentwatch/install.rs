//! Installs and removes bleep's entries in my own agent config, only after my tap.
//! Every other entry stays as it was. A backup is written before the first change.
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct HookFiles {
    pub claude_settings: PathBuf,
    pub codex_config: PathBuf,
    pub hook_bin: PathBuf,
}

impl HookFiles {
    pub fn real(paths: &crate::paths::BleepPaths) -> HookFiles {
        let home = dirs::home_dir().unwrap_or_default();
        HookFiles { claude_settings: home.join(".claude/settings.json"), codex_config: home.join(".codex/config.toml"), hook_bin: paths.hook_bin.clone() }
    }
}

/// (event, waits for bleep's answer, takes a tool matcher)
pub const CLAUDE_EVENTS: &[(&str, bool, bool)] = &[
    ("SessionStart", false, false),
    ("UserPromptSubmit", false, false),
    ("PermissionRequest", true, true),
    ("PostToolUse", false, true),
    ("Notification", false, false),
    ("Stop", true, false),
    ("SessionEnd", false, false),
];

fn hook_command(f: &HookFiles, event: &str) -> String {
    let bin = f.hook_bin.display().to_string();
    let bin = if bin.contains(' ') { format!("'{bin}'") } else { bin };
    format!("{bin} claude {event}")
}

fn is_ours(f: &HookFiles, entry_hook: &Value) -> bool {
    let bin = f.hook_bin.display().to_string();
    entry_hook.get("command").and_then(Value::as_str).is_some_and(|c| c.contains(&bin))
}

/// The real file behind a symlink, so dotfile setups keep their link.
fn target(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn backup_once(file: &Path) -> Result<(), String> {
    let name = format!("{}.bleep-backup", file.file_name().and_then(|n| n.to_str()).unwrap_or("settings"));
    let b = file.with_file_name(name);
    if file.exists() && !b.exists() {
        std::fs::copy(file, &b).map_err(|e| format!("Could not save a backup of {}: {e}", file.display()))?;
    }
    Ok(())
}

/// Writes next to the file and renames it into place.
fn write_atomic(file: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = file.with_extension("bleep-tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    if let Ok(m) = std::fs::metadata(file) {
        let _ = std::fs::set_permissions(&tmp, m.permissions());
    }
    std::fs::rename(&tmp, file).map_err(|e| e.to_string())
}

/// `None` when the file does not exist.
fn read_settings(file: &Path) -> Result<Option<Map<String, Value>>, String> {
    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Could not read {}: {e}", file.display())),
    };
    if text.trim().is_empty() {
        return Ok(Some(Map::new()));
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(m)) => Ok(Some(m)),
        _ => Err(format!("{} is not valid JSON, so bleep did not change it. Fix it and try again.", file.display())),
    }
}

/// Takes bleep's commands out of every hook entry, and drops what is left empty.
fn strip_ours(f: &HookFiles, settings: &mut Map<String, Value>) {
    let Some(Value::Object(hooks)) = settings.get_mut("hooks") else { return };
    for list in hooks.values_mut() {
        if let Value::Array(entries) = list {
            for e in entries.iter_mut() {
                if let Some(Value::Array(hs)) = e.get_mut("hooks") {
                    hs.retain(|h| !is_ours(f, h));
                }
            }
            entries.retain(|e| e.get("hooks").and_then(Value::as_array).is_none_or(|h| !h.is_empty()));
        }
    }
    hooks.retain(|_, v| v.as_array().is_none_or(|a| !a.is_empty()));
    if hooks.is_empty() {
        settings.remove("hooks");
    }
}

pub fn claude_status(f: &HookFiles) -> bool {
    let Ok(Some(s)) = read_settings(&target(&f.claude_settings)) else { return false };
    CLAUDE_EVENTS.iter().all(|(event, _, _)| {
        s.get("hooks").and_then(|h| h.get(*event)).and_then(Value::as_array).is_some_and(|entries| {
            entries.iter().any(|e| e.get("hooks").and_then(Value::as_array).is_some_and(|hs| hs.iter().any(|h| h.get("command").and_then(Value::as_str) == Some(&hook_command(f, event)))))
        })
    })
}

pub fn install_claude(f: &HookFiles) -> Result<(), String> {
    let file = target(&f.claude_settings);
    let mut s = read_settings(&file)?.unwrap_or_default();
    backup_once(&file)?;
    strip_ours(f, &mut s);
    let hooks = s.entry("hooks").or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(hooks) = hooks else { return Err(format!("\"hooks\" in {} is not an object, so bleep did not change it.", file.display())) };
    for (event, waits, matcher) in CLAUDE_EVENTS {
        let mut h = json!({"type": "command", "command": hook_command(f, event)});
        if *waits {
            h["timeout"] = json!(30);
        }
        let mut entry = Map::new();
        if *matcher {
            entry.insert("matcher".into(), json!("*"));
        }
        entry.insert("hooks".into(), json!([h]));
        let list = hooks.entry(event.to_string()).or_insert_with(|| json!([]));
        let Value::Array(list) = list else { return Err(format!("hooks.{event} in {} is not a list, so bleep did not change it.", file.display())) };
        list.push(Value::Object(entry));
    }
    write_atomic(&file, &(serde_json::to_string_pretty(&Value::Object(s)).map_err(|e| e.to_string())? + "\n"))
}

pub fn remove_claude(f: &HookFiles) -> Result<(), String> {
    let file = target(&f.claude_settings);
    let Some(mut s) = read_settings(&file)? else { return Ok(()) };
    let before = s.clone();
    strip_ours(f, &mut s);
    if s == before {
        return Ok(());
    }
    write_atomic(&file, &(serde_json::to_string_pretty(&Value::Object(s)).map_err(|e| e.to_string())? + "\n"))
}

/// `None` when the file does not exist.
fn read_codex(file: &Path) -> Result<Option<toml_edit::DocumentMut>, String> {
    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Could not read {}: {e}", file.display())),
    };
    text.parse().map(Some).map_err(|_| format!("{} is not valid TOML, so bleep did not change it. Fix it and try again.", file.display()))
}

fn notify_list(doc: &toml_edit::DocumentMut) -> Option<Vec<String>> {
    doc.get("notify")?.as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
}

fn is_our_notify(f: &HookFiles, list: &[String]) -> bool {
    list.first().map(String::as_str) == Some(&f.hook_bin.display().to_string()) && list.get(1).map(String::as_str) == Some("codex-notify")
}

pub fn codex_notify_status(f: &HookFiles) -> bool {
    matches!(read_codex(&target(&f.codex_config)), Ok(Some(doc)) if notify_list(&doc).is_some_and(|l| is_our_notify(f, &l)))
}

/// Puts bleep-hook in front of Codex's `notify` program, which still runs first.
pub fn install_codex_notify(f: &HookFiles) -> Result<(), String> {
    let file = target(&f.codex_config);
    let mut doc = read_codex(&file)?.unwrap_or_default();
    let current = notify_list(&doc).unwrap_or_default();
    if is_our_notify(f, &current) {
        return Ok(());
    }
    backup_once(&file)?;
    let mut list = toml_edit::Array::new();
    list.push(f.hook_bin.display().to_string());
    list.push("codex-notify");
    list.push("--");
    for x in &current {
        list.push(x.as_str());
    }
    doc["notify"] = toml_edit::value(list);
    write_atomic(&file, &doc.to_string())
}

/// Puts back the `notify` program Codex had before, or removes the key if there was none.
pub fn remove_codex_notify(f: &HookFiles) -> Result<(), String> {
    let file = target(&f.codex_config);
    let Some(mut doc) = read_codex(&file)? else { return Ok(()) };
    let current = notify_list(&doc).unwrap_or_default();
    if !is_our_notify(f, &current) {
        return Ok(());
    }
    let rest = if current.get(2).map(String::as_str) == Some("--") { &current[3..] } else { &current[2..] };
    let original: Vec<&str> = rest.iter().map(String::as_str).collect();
    if original.is_empty() {
        doc.remove("notify");
    } else {
        doc["notify"] = toml_edit::value(toml_edit::Array::from_iter(original));
    }
    write_atomic(&file, &doc.to_string())
}

#[cfg(test)]
mod codex_tests {
    use super::*;

    fn files(d: &Path) -> HookFiles {
        HookFiles { claude_settings: d.join("claude/settings.json"), codex_config: d.join("codex/config.toml"), hook_bin: PathBuf::from("/Users/me/.bleep/bin/bleep-hook") }
    }

    const MINE: &str = "# my codex settings\nmodel = \"gpt-5\"\nnotify = [\"/x/Sky Client\", \"turn-ended\"] # computer use\n\n[features]\nhooks = true\n";

    fn notify(f: &HookFiles) -> Option<Vec<String>> {
        let doc: toml_edit::DocumentMut = std::fs::read_to_string(&f.codex_config).unwrap().parse().unwrap();
        doc.get("notify").and_then(|n| n.as_array()).map(|a| a.iter().map(|v| v.as_str().unwrap().to_string()).collect())
    }

    fn write(f: &HookFiles, text: &str) {
        std::fs::create_dir_all(f.codex_config.parent().unwrap()).unwrap();
        std::fs::write(&f.codex_config, text).unwrap();
    }

    #[test]
    fn notify_install_keeps_comments_and_original() {
        let d = tempfile::tempdir().unwrap();
        let f = files(d.path());
        write(&f, MINE);
        assert!(!codex_notify_status(&f));
        install_codex_notify(&f).unwrap();
        assert!(codex_notify_status(&f));
        assert_eq!(notify(&f).unwrap(), vec!["/Users/me/.bleep/bin/bleep-hook", "codex-notify", "--", "/x/Sky Client", "turn-ended"]);
        let text = std::fs::read_to_string(&f.codex_config).unwrap();
        assert!(text.starts_with("# my codex settings\n") && text.contains("[features]\nhooks = true"), "{text}");
        install_codex_notify(&f).unwrap();
        assert_eq!(notify(&f).unwrap().len(), 5, "install twice wraps once");
        assert_eq!(std::fs::read_to_string(f.codex_config.with_file_name("config.toml.bleep-backup")).unwrap(), MINE);
    }

    #[test]
    fn notify_remove_restores_exact_array() {
        let d = tempfile::tempdir().unwrap();
        let f = files(d.path());
        write(&f, MINE);
        install_codex_notify(&f).unwrap();
        remove_codex_notify(&f).unwrap();
        assert!(!codex_notify_status(&f));
        assert_eq!(notify(&f).unwrap(), vec!["/x/Sky Client", "turn-ended"]);
        assert!(std::fs::read_to_string(&f.codex_config).unwrap().contains("# my codex settings"));
    }

    #[test]
    fn notify_install_without_existing_notify() {
        let d = tempfile::tempdir().unwrap();
        let f = files(d.path());
        write(&f, "model = \"gpt-5\"\n\n[features]\nhooks = true\n");
        install_codex_notify(&f).unwrap();
        assert_eq!(notify(&f).unwrap(), vec!["/Users/me/.bleep/bin/bleep-hook", "codex-notify", "--"]);
        let doc: toml_edit::DocumentMut = std::fs::read_to_string(&f.codex_config).unwrap().parse().unwrap();
        assert_eq!(doc["features"]["hooks"].as_bool(), Some(true), "notify stays a top-level key");
        remove_codex_notify(&f).unwrap();
        assert_eq!(notify(&f), None);
    }

    #[test]
    fn invalid_codex_config_refuses() {
        let d = tempfile::tempdir().unwrap();
        let f = files(d.path());
        write(&f, "model = [broken");
        assert!(install_codex_notify(&f).unwrap_err().contains("not valid TOML"));
        assert_eq!(std::fs::read_to_string(&f.codex_config).unwrap(), "model = [broken");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn files(d: &Path) -> HookFiles {
        HookFiles { claude_settings: d.join("claude/settings.json"), codex_config: d.join("codex/config.toml"), hook_bin: PathBuf::from("/Users/me/.bleep/bin/bleep-hook") }
    }

    fn read(p: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    const MINE: &str = r#"{
  "model": "opus",
  "hooks": {
    "Stop": [{"hooks": [{"type": "command", "command": "say done"}]}]
  },
  "permissions": {"allow": ["Bash(npm test)"]}
}"#;

    #[test]
    fn install_into_missing_file() {
        let d = tempfile::tempdir().unwrap();
        let f = files(d.path());
        assert!(!claude_status(&f));
        install_claude(&f).unwrap();
        assert!(claude_status(&f));
        let v = read(&f.claude_settings);
        assert_eq!(v["hooks"]["PermissionRequest"][0], json!({"matcher": "*", "hooks": [{"type": "command", "command": "/Users/me/.bleep/bin/bleep-hook claude PermissionRequest", "timeout": 30}]}));
        assert_eq!(v["hooks"]["SessionStart"][0], json!({"hooks": [{"type": "command", "command": "/Users/me/.bleep/bin/bleep-hook claude SessionStart"}]}));
        assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["timeout"], 30);
        assert_eq!(v["hooks"].as_object().unwrap().len(), 7);
        assert!(!f.claude_settings.with_file_name("settings.json.bleep-backup").exists(), "nothing to back up");
    }

    #[test]
    fn install_keeps_other_hooks_and_key_order() {
        let d = tempfile::tempdir().unwrap();
        let f = files(d.path());
        std::fs::create_dir_all(f.claude_settings.parent().unwrap()).unwrap();
        std::fs::write(&f.claude_settings, MINE).unwrap();
        install_claude(&f).unwrap();
        let v = read(&f.claude_settings);
        assert_eq!(v.as_object().unwrap().keys().collect::<Vec<_>>(), vec!["model", "hooks", "permissions"]);
        let stop = v["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        assert_eq!(stop[0]["hooks"][0]["command"], "say done");
        assert_eq!(v["permissions"]["allow"][0], "Bash(npm test)");
    }

    #[test]
    fn install_twice_is_one_entry_each() {
        let d = tempfile::tempdir().unwrap();
        let f = files(d.path());
        install_claude(&f).unwrap();
        install_claude(&f).unwrap();
        let v = read(&f.claude_settings);
        for (e, _, _) in CLAUDE_EVENTS {
            assert_eq!(v["hooks"][e].as_array().unwrap().len(), 1, "{e}");
        }
    }

    #[test]
    fn install_follows_symlink() {
        let d = tempfile::tempdir().unwrap();
        let f = files(d.path());
        let real = d.path().join("dotfiles/claude.json");
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::create_dir_all(f.claude_settings.parent().unwrap()).unwrap();
        std::fs::write(&real, MINE).unwrap();
        std::os::unix::fs::symlink(&real, &f.claude_settings).unwrap();
        install_claude(&f).unwrap();
        assert!(std::fs::symlink_metadata(&f.claude_settings).unwrap().file_type().is_symlink());
        assert_eq!(read(&real)["hooks"]["Stop"].as_array().unwrap().len(), 2);
        assert!(real.with_file_name("claude.json.bleep-backup").exists());
    }

    #[test]
    fn invalid_json_refuses() {
        let d = tempfile::tempdir().unwrap();
        let f = files(d.path());
        std::fs::create_dir_all(f.claude_settings.parent().unwrap()).unwrap();
        std::fs::write(&f.claude_settings, "{ not json").unwrap();
        let e = install_claude(&f).unwrap_err();
        assert!(e.contains("not valid JSON"), "{e}");
        assert_eq!(std::fs::read_to_string(&f.claude_settings).unwrap(), "{ not json");
        assert!(remove_claude(&f).is_err());
    }

    #[test]
    fn remove_deletes_only_bleep_entries() {
        let d = tempfile::tempdir().unwrap();
        let f = files(d.path());
        std::fs::create_dir_all(f.claude_settings.parent().unwrap()).unwrap();
        std::fs::write(&f.claude_settings, MINE).unwrap();
        install_claude(&f).unwrap();
        remove_claude(&f).unwrap();
        assert!(!claude_status(&f));
        assert_eq!(read(&f.claude_settings), serde_json::from_str::<Value>(MINE).unwrap());
        remove_claude(&f).unwrap();
        let empty = tempfile::tempdir().unwrap();
        remove_claude(&files(empty.path())).unwrap();
    }

    #[test]
    fn backup_written_once() {
        let d = tempfile::tempdir().unwrap();
        let f = files(d.path());
        std::fs::create_dir_all(f.claude_settings.parent().unwrap()).unwrap();
        std::fs::write(&f.claude_settings, MINE).unwrap();
        install_claude(&f).unwrap();
        remove_claude(&f).unwrap();
        install_claude(&f).unwrap();
        let b = f.claude_settings.with_file_name("settings.json.bleep-backup");
        assert_eq!(std::fs::read_to_string(b).unwrap(), MINE);
    }
}
