use std::io;
use std::path::PathBuf;

/// The three memory files that go into the system prompt (build plan 8.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MemFile {
    User,
    Rules,
    Memory,
}

impl MemFile {
    pub const ALL: [MemFile; 3] = [MemFile::User, MemFile::Rules, MemFile::Memory];

    pub fn file_name(self) -> &'static str {
        match self {
            MemFile::User => "USER.md",
            MemFile::Rules => "RULES.md",
            MemFile::Memory => "MEMORY.md",
        }
    }

    pub fn from_name(s: &str) -> Option<MemFile> {
        MemFile::ALL.into_iter().find(|f| f.file_name().eq_ignore_ascii_case(s))
    }
}

/// Every place bleep keeps data on disk. Root is `$BLEEP_HOME`, or `~/.bleep` (`~/.bleep-nightly`
/// or `~/.bleep-local` for those builds).
#[derive(Clone, Debug)]
pub struct BleepPaths {
    pub root: PathBuf,
    pub db: PathBuf,
    pub memory: PathBuf,
    pub daily: PathBuf,
    pub claude_home: PathBuf,
    pub claude_work: PathBuf,
    pub run: PathBuf,
    pub logs: PathBuf,
    pub config_file: PathBuf,
    pub bin: PathBuf,
    pub hook_bin: PathBuf,
    pub sock: PathBuf,
    pub agent_rules: PathBuf,
}

pub fn resolve_root(bleep_home: Option<String>) -> PathBuf {
    match bleep_home.filter(|s| !s.is_empty()) {
        Some(p) => PathBuf::from(p),
        None => home().join(crate::flavor::HOME_DIR),
    }
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"))
}

impl BleepPaths {
    pub fn from_env() -> Self {
        let custom = std::env::var("BLEEP_HOME").ok().filter(|s| !s.is_empty());
        let mut p = Self::from_root(resolve_root(custom.clone()));
        if custom.is_none() {
            // Every build shares one hook binary, so the Claude Code hook entry is the same
            // whichever app installed it.
            p.hook_bin = home().join(crate::flavor::PROD_HOME_DIR).join("bin").join("bleep-hook");
        }
        if crate::flavor::LOCAL && custom.is_none() {
            // A local build shares the production app's Claude sign-in, so it needs no second
            // login. Its chat sessions stay its own: they run in its own working folder.
            p.claude_home = home().join(crate::flavor::PROD_HOME_DIR).join("claude-home");
        }
        p
    }

    /// The production data folder a nightly or local build seeds from.
    pub fn prod_root() -> PathBuf {
        home().join(crate::flavor::PROD_HOME_DIR)
    }

    pub fn from_root(root: PathBuf) -> Self {
        Self {
            db: root.join("bleep.db"),
            memory: root.join("memory"),
            daily: root.join("memory").join("daily"),
            claude_home: root.join("claude-home"),
            claude_work: root.join("claude-work"),
            run: root.join("run"),
            logs: root.join("logs"),
            config_file: root.join("config.toml"),
            bin: root.join("bin"),
            hook_bin: root.join("bin").join("bleep-hook"),
            sock: root.join("run").join("bleep.sock"),
            agent_rules: root.join("agent-rules.toml"),
            root,
        }
    }

    pub fn memory_file(&self, f: MemFile) -> PathBuf {
        self.memory.join(f.file_name())
    }

    pub fn ensure(&self) -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        for d in [&self.root, &self.memory, &self.daily, &self.claude_home, &self.claude_work, &self.run, &self.logs, &self.bin] {
            std::fs::create_dir_all(d)?;
        }
        std::fs::set_permissions(&self.run, std::fs::Permissions::from_mode(0o700))?;
        Ok(())
    }
}
