use std::io;
use std::path::PathBuf;

/// Every place bleep keeps data on disk. Root is `$BLEEP_HOME` or `~/.bleep`.
#[derive(Clone, Debug)]
pub struct BleepPaths {
    pub root: PathBuf,
    pub db: PathBuf,
    pub memory: PathBuf,
    pub claude_home: PathBuf,
    pub claude_work: PathBuf,
    pub run: PathBuf,
    pub logs: PathBuf,
    pub config_file: PathBuf,
}

pub fn resolve_root(bleep_home: Option<String>) -> PathBuf {
    match bleep_home.filter(|s| !s.is_empty()) {
        Some(p) => PathBuf::from(p),
        None => dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")).join(".bleep"),
    }
}

impl BleepPaths {
    pub fn from_env() -> Self {
        Self::from_root(resolve_root(std::env::var("BLEEP_HOME").ok()))
    }

    pub fn from_root(root: PathBuf) -> Self {
        Self {
            db: root.join("bleep.db"),
            memory: root.join("memory"),
            claude_home: root.join("claude-home"),
            claude_work: root.join("claude-work"),
            run: root.join("run"),
            logs: root.join("logs"),
            config_file: root.join("config.toml"),
            root,
        }
    }

    pub fn ensure(&self) -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        for d in [&self.root, &self.memory, &self.claude_home, &self.claude_work, &self.run, &self.logs] {
            std::fs::create_dir_all(d)?;
        }
        std::fs::set_permissions(&self.run, std::fs::Permissions::from_mode(0o700))?;
        Ok(())
    }
}
