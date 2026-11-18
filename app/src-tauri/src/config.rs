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

/// Self-update. On by default: new releases download and install in the background,
/// then run after the next restart.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdatesConfig {
    pub auto_install: bool,
}

impl Default for UpdatesConfig {
    fn default() -> Self {
        Self { auto_install: true }
    }
}

/// Voice: the wake word, the mic, and spoken replies. Empty device names mean
/// the system default, which follows whatever macOS picks (AirPods in, AirPods used).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceConfig {
    /// Listen for the wake phrase. Off until the user turns it on: it keeps the mic open.
    pub wake: bool,
    pub phrase: String,
    /// "strict", "normal" or "loose": how sure bleep must be that it heard the phrase.
    pub sensitivity: String,
    /// Input device name.
    pub mic: String,
    /// Read replies to spoken requests aloud.
    pub speak: bool,
    /// Output device name for spoken replies.
    pub speaker: String,
    /// A macOS voice name. Empty is the system voice.
    pub voice: String,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self { wake: false, phrase: crate::voice::wake::DEFAULT_PHRASE.into(), sensitivity: "normal".into(), mic: String::new(), speak: true, speaker: String::new(), voice: String::new() }
    }
}

/// Memory and compaction (Phase 5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryConfig {
    /// Compact when the context passes this many tokens (capped by the window, see `threshold`).
    pub compact_at: u64,
    /// Local "HH:MM" for the nightly suggestions job.
    pub nightly: String,
    /// The window the runtime last reported, in tokens.
    pub context_window: Option<u64>,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self { compact_at: 250_000, nightly: "22:00".into(), context_window: None }
    }
}

pub const MIN_COMPACT_AT: u64 = 20_000;
pub const DEFAULT_WINDOW: u64 = 200_000;

impl MemoryConfig {
    pub fn window(&self) -> u64 {
        self.context_window.filter(|w| *w > 0).unwrap_or(DEFAULT_WINDOW)
    }

    /// The lower of `compact_at` and 80% of the window, never below 20K.
    pub fn threshold(&self) -> u64 {
        self.compact_at.min(self.window() * 8 / 10).max(MIN_COMPACT_AT)
    }

    /// Past 95% of the window bleep starts fresh even without a summary.
    pub fn hard_stop(&self) -> u64 {
        self.window() * 95 / 100
    }
}

/// How a remote MCP tool runs: ask with a card, run at once, or hidden.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolMode {
    Ask,
    Auto,
    Off,
}

/// How a remote MCP server signs in. Secrets are in the Keychain, never here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpAuth {
    #[default]
    None,
    Oauth,
    Token,
}

fn yes() -> bool {
    true
}

/// One remote MCP server I added (Phase 7).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct McpServer {
    pub name: String,
    pub url: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub auth: McpAuth,
    /// Per-tool mode by the server's own tool name. A missing tool uses its default.
    #[serde(default)]
    pub tools: std::collections::BTreeMap<String, ToolMode>,
}

impl McpServer {
    pub fn mode(&self, tool: &str) -> Option<ToolMode> {
        self.tools.get(tool).copied()
    }
}

/// Files on this Mac (0.5.0): the folders bleep may search and read. `~` is the home folder.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilesConfig {
    pub folders: Vec<String>,
}

impl Default for FilesConfig {
    fn default() -> Self {
        Self { folders: ["~/Documents", "~/Desktop", "~/Downloads"].map(String::from).to_vec() }
    }
}

/// Settings > Connectors (0.5.0, F3): which built-in connectors are on. Remote ones keep
/// their own `enabled` in `mcp.servers`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConnectorsConfig {
    pub on: Vec<String>,
    /// Set once the list was first filled. Before 0.5.0 Gmail and Calendar were always on.
    pub initialized: bool,
}

impl ConnectorsConfig {
    /// First run of 0.5.0: someone who already set bleep up keeps Gmail and Calendar on.
    /// Returns true when it changed something to save.
    pub fn init(&mut self, setup_done: bool) -> bool {
        if self.initialized {
            return false;
        }
        if setup_done {
            for id in ["gmail", "calendar"] {
                if !self.on.iter().any(|x| x == id) {
                    self.on.push(id.into());
                }
            }
        }
        self.initialized = true;
        true
    }

    pub fn is_on(&self, id: &str) -> bool {
        self.on.iter().any(|x| x == id)
    }

    pub fn set(&mut self, id: &str, on: bool) {
        self.on.retain(|x| x != id);
        if on {
            self.on.push(id.into());
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct McpConfig {
    pub servers: Vec<McpServer>,
}

/// The API runtime (Phase 9). The key is in the Keychain at `api:<provider>:key`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiConfig {
    pub provider: String,
    pub base_url: String,
    pub main: String,
    pub small: String,
    /// US dollars a day. When reached, proactive work stops until tomorrow.
    pub daily_cap_usd: f64,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self { provider: "openrouter".into(), base_url: "https://openrouter.ai/api/v1".into(), main: String::new(), small: String::new(), daily_cap_usd: 1.0 }
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
    pub memory: MemoryConfig,
    pub mcp: McpConfig,
    pub connectors: ConnectorsConfig,
    pub files: FilesConfig,
    pub api: ApiConfig,
    pub updates: UpdatesConfig,
    pub voice: VoiceConfig,
    /// The floating buddy. See `avatar.rs`.
    pub avatar: crate::avatar::AvatarConfig,
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
            memory: MemoryConfig::default(),
            mcp: McpConfig::default(),
            connectors: ConnectorsConfig::default(),
            files: FilesConfig::default(),
            api: ApiConfig::default(),
            updates: UpdatesConfig::default(),
            voice: VoiceConfig::default(),
            avatar: crate::avatar::AvatarConfig::default(),
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
    fn connectors_init_keeps_gmail_and_calendar_for_existing_setups() {
        let mut c = ConnectorsConfig::default();
        assert!(c.init(true));
        assert!(c.is_on("gmail") && c.is_on("calendar") && !c.is_on("drive"));
        assert!(!c.init(true), "only once");
        let mut fresh = ConnectorsConfig::default();
        fresh.init(false);
        assert!(fresh.on.is_empty(), "a new install starts with every connector off");
        c.set("drive", true);
        c.set("gmail", false);
        assert_eq!(c.on, ["calendar", "drive"]);
    }

    #[test]
    fn memory_defaults_and_threshold() {
        let c: Config = toml::from_str("").unwrap();
        assert_eq!(c.memory, MemoryConfig { compact_at: 250_000, nightly: "22:00".into(), context_window: None });
        assert_eq!(c.memory.threshold(), 160_000, "unknown window counts as 200K");
        let m = |compact_at, w| MemoryConfig { compact_at, nightly: "22:00".into(), context_window: w };
        assert_eq!(m(250_000, Some(1_000_000)).threshold(), 250_000);
        assert_eq!(m(300_000, Some(200_000)).threshold(), 160_000);
        assert_eq!(m(5_000, Some(1_000_000)).threshold(), 20_000, "never below 20K");
        assert_eq!(m(250_000, Some(1_000_000)).hard_stop(), 950_000);
        let c: Config = toml::from_str("[memory]\ncompact_at = 20000").unwrap();
        assert_eq!((c.memory.compact_at, c.memory.nightly.as_str()), (20_000, "22:00"));
    }

    #[test]
    fn mcp_servers_round_trip_with_defaults() {
        let c: Config = toml::from_str("").unwrap();
        assert!(c.mcp.servers.is_empty());
        let c: Config = toml::from_str("[[mcp.servers]]\nname = \"dayblocks\"\nurl = \"https://dayblocks.vercel.app/api/mcp\"\n").unwrap();
        let s = &c.mcp.servers[0];
        assert_eq!((s.name.as_str(), s.enabled, s.auth), ("dayblocks", true, McpAuth::None));
        assert!(s.tools.is_empty(), "no tools map is fine");
        let mut c = c.clone();
        c.mcp.servers[0].auth = McpAuth::Oauth;
        c.mcp.servers[0].tools.insert("move_task".into(), ToolMode::Auto);
        c.mcp.servers[0].tools.insert("delete_task".into(), ToolMode::Off);
        let back: Config = toml::from_str(&toml::to_string_pretty(&c).unwrap()).unwrap();
        assert_eq!(back.mcp, c.mcp);
        assert_eq!(back.mcp.servers[0].mode("move_task"), Some(ToolMode::Auto));
        assert_eq!(back.mcp.servers[0].mode("list_tasks"), None);
    }

    #[test]
    fn paths_have_memory_files() {
        let p = BleepPaths::from_root(PathBuf::from("/x"));
        assert_eq!(p.memory_file(crate::paths::MemFile::Memory), PathBuf::from("/x/memory/MEMORY.md"));
        assert_eq!(p.memory_file(crate::paths::MemFile::User), PathBuf::from("/x/memory/USER.md"));
        assert_eq!(p.memory_file(crate::paths::MemFile::Rules), PathBuf::from("/x/memory/RULES.md"));
        assert_eq!(p.daily, PathBuf::from("/x/memory/daily"));
    }

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
    fn voice_defaults_off_with_system_devices() {
        let c: Config = toml::from_str("").unwrap();
        assert_eq!(c.voice, VoiceConfig { wake: false, phrase: "hey bleep".into(), sensitivity: "normal".into(), mic: String::new(), speak: true, speaker: String::new(), voice: String::new() });
        let c: Config = toml::from_str("[voice]\nwake = true\nphrase = \"jarvis\"\nmic = \"AirPods\"").unwrap();
        assert!(c.voice.wake && c.voice.speak);
        assert_eq!((c.voice.phrase.as_str(), c.voice.mic.as_str(), c.voice.speaker.as_str()), ("jarvis", "AirPods", ""));
    }

    #[test]
    fn default_root_is_dot_bleep() {
        assert!(resolve_root(None).ends_with(".bleep"));
    }
}
