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

/// Google OAuth client (Desktop app type) from my own Cloud project. Debug hides the secret.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GoogleConfig {
    pub client_id: String,
    pub client_secret: String,
}

impl std::fmt::Debug for GoogleConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoogleConfig").field("client_id", &self.client_id).field("client_secret", &"<hidden>").finish()
    }
}

/// Mail rules until Phase 5's RULES.md. Nothing is muted without the user's tap.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MailConfig {
    pub vips: Vec<String>,
    pub muted: Vec<String>,
    /// Senders with three "Not useful" pings, waiting for the user to say Mute or Keep.
    pub suggested_mutes: Vec<String>,
}

/// Local times, "HH:MM".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CheckinConfig {
    pub brief: String,
    pub checkins: Vec<String>,
}

impl Default for CheckinConfig {
    fn default() -> Self {
        Self { brief: "09:00".into(), checkins: vec!["13:00".into(), "18:00".into()] }
    }
}

/// Agent watch (Phase 4). Auto-approve stays off until the user turns it on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentsConfig {
    pub auto_approve: bool,
    pub screenshot_check: bool,
    pub codex_watch: bool,
}

impl Default for AgentsConfig {
    fn default() -> Self {
        Self { auto_approve: false, screenshot_check: true, codex_watch: false }
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
    /// The first-run setup wizard was finished. Until then it opens at launch.
    pub setup_done: bool,
    pub google: GoogleConfig,
    pub mail: MailConfig,
    pub checkin: CheckinConfig,
    pub agents: AgentsConfig,
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
            setup_done: false,
            google: GoogleConfig::default(),
            mail: MailConfig::default(),
            checkin: CheckinConfig::default(),
            agents: AgentsConfig::default(),
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
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let tmp = path.with_extension("toml.tmp");
        // 0600: the file holds the Google client secret.
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        f.write_all(toml::to_string_pretty(self)?.as_bytes())?;
        drop(f);
        std::fs::set_permissions(&tmp, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
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
    fn config_agents_defaults_off() {
        let c: Config = toml::from_str("").unwrap();
        assert_eq!(c.agents, AgentsConfig { auto_approve: false, screenshot_check: true, codex_watch: false });
        let c: Config = toml::from_str("[agents]\nauto_approve = true").unwrap();
        assert!(c.agents.auto_approve && c.agents.screenshot_check);
    }

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
        assert!(!c.setup_done, "a fresh install starts with the setup wizard");
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
    fn config_with_google_is_saved_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        let mut c = Config::default();
        c.google = GoogleConfig { client_id: "cid".into(), client_secret: "shh".into() };
        c.save(&p).unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(Config::load(&p).unwrap().google, c.google);
        assert!(!format!("{:?}", c).contains("shh"));
    }

    #[test]
    fn config_reads_check_in_times_and_mail_lists() {
        let c = Config::default();
        assert_eq!(c.checkin.brief, "09:00");
        assert_eq!(c.checkin.checkins, vec!["13:00".to_string(), "18:00".to_string()]);
        assert!(c.mail.vips.is_empty() && c.mail.muted.is_empty() && c.mail.suggested_mutes.is_empty());
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        std::fs::write(&p, "[checkin]\nbrief = \"08:30\"\n[mail]\nvips = [\"priya@example.com\"]\n").unwrap();
        let c = Config::load(&p).unwrap();
        assert_eq!(c.checkin.brief, "08:30");
        assert_eq!(c.checkin.checkins, vec!["13:00".to_string(), "18:00".to_string()], "unset keys keep defaults");
        assert_eq!(c.mail.vips, vec!["priya@example.com".to_string()]);
    }

    #[test]
    fn default_root_is_dot_bleep() {
        assert!(resolve_root(None).ends_with(".bleep"));
    }
}
