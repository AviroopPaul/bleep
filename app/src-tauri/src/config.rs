use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tiers {
    pub main: String,
    pub small: String,
}

impl Default for Tiers {
    fn default() -> Self {
        Self { main: "sonnet".into(), small: "haiku".into() }
    }
}

/// `~/.bleep/config.toml`. Unknown keys are ignored so later phases can add fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub runtime: String,
    pub tiers: Tiers,
    pub droid_pos: Option<(f64, f64)>,
    pub summon_fallback: String,
    pub autostart_set: bool,
    pub summon_note_shown: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            runtime: "claude".into(),
            tiers: Tiers::default(),
            droid_pos: None,
            summon_fallback: "Alt+Space".into(),
            autostart_set: false,
            summon_note_shown: false,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Config> {
        match std::fs::read_to_string(path) {
            Ok(s) => Ok(toml::from_str(&s)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, toml::to_string_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::{resolve_root, BleepPaths};
    use std::path::PathBuf;

    #[test]
    fn load_missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let c = Config::load(&dir.path().join("config.toml")).unwrap();
        assert_eq!(c.runtime, "claude");
        assert_eq!(c.tiers.main, "sonnet");
        assert_eq!(c.tiers.small, "haiku");
        assert_eq!(c.summon_fallback, "Alt+Space");
        assert_eq!(c.droid_pos, None);
        assert!(!c.autostart_set);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        let mut c = Config::default();
        c.droid_pos = Some((40.0, 600.0));
        c.save(&p).unwrap();
        assert_eq!(Config::load(&p).unwrap(), c);
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "runtime = \"claude\"\nfuture_thing = 3\n").unwrap();
        assert_eq!(Config::load(&p).unwrap().runtime, "claude");
    }

    #[test]
    fn paths_respect_bleep_home() {
        let p = BleepPaths::from_root(resolve_root(Some("/tmp/x".into())));
        assert_eq!(p.db, PathBuf::from("/tmp/x/bleep.db"));
        assert_eq!(p.claude_home, PathBuf::from("/tmp/x/claude-home"));
        assert_eq!(p.claude_work, PathBuf::from("/tmp/x/claude-work"));
        assert_eq!(p.config_file, PathBuf::from("/tmp/x/config.toml"));
    }

    #[test]
    fn default_root_is_dot_bleep() {
        assert!(resolve_root(None).ends_with(".bleep"));
    }
}
