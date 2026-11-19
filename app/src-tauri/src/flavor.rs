//! Which build this is. Three apps can live side by side, each with its own name, bundle id,
//! data folder, Keychain items and icon:
//! - **prod** ("bleep"): the release.
//! - **nightly** ("bleep (nightly)", cargo feature `nightly`): built by CI from `main` every
//!   night. It does everything prod does, but updates from the nightly feed only.
//! - **local** ("bleep (local)", cargo feature `local`): built on this Mac from the repo. It
//!   never updates itself, never sets a login item and starts paused.
//! The Claude Code hook is shared: one hook entry, and it reaches every running app. Only the
//! first running app in `ALL` order (prod, nightly, local) answers agents. See `bleep-hook`.
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    Prod,
    Nightly,
    Local,
}

impl Flavor {
    /// Priority order: the first running app answers agents.
    pub const ALL: [Flavor; 3] = [Flavor::Prod, Flavor::Nightly, Flavor::Local];

    /// Data folder under the home folder. `bleep-hook` has the same list.
    pub const fn home_dir(self) -> &'static str {
        match self {
            Flavor::Prod => ".bleep",
            Flavor::Nightly => ".bleep-nightly",
            Flavor::Local => ".bleep-local",
        }
    }

    /// The short tag after the name, or "" for prod.
    pub const fn tag(self) -> &'static str {
        match self {
            Flavor::Prod => "",
            Flavor::Nightly => "nightly",
            Flavor::Local => "local",
        }
    }
}

pub const FLAVOR: Flavor = if cfg!(feature = "local") {
    Flavor::Local
} else if cfg!(feature = "nightly") {
    Flavor::Nightly
} else {
    Flavor::Prod
};

/// A local build: no updates, no login item, starts paused.
pub const LOCAL: bool = matches!(FLAVOR, Flavor::Local);

/// The name people see: windows, tray, notifications.
pub const NAME: &str = match FLAVOR {
    Flavor::Prod => "bleep",
    Flavor::Nightly => "bleep (nightly)",
    Flavor::Local => "bleep (local)",
};

/// Keychain service for Google, remote MCP and API key secrets.
pub const KEYCHAIN_SERVICE: &str = match FLAVOR {
    Flavor::Prod => "site.avirooppaul.bleep",
    Flavor::Nightly => "site.avirooppaul.bleep.nightly",
    Flavor::Local => "site.avirooppaul.bleep.local",
};

/// Data folder under the home folder.
pub const HOME_DIR: &str = FLAVOR.home_dir();

/// The production data folder, which nightly and local builds read once to seed themselves.
pub const PROD_HOME_DIR: &str = Flavor::Prod.home_dir();

/// The data folder of the old "bleep (dev)" build, which a local build takes over once.
pub const OLD_DEV_HOME_DIR: &str = ".bleep-dev";

/// The agent-watch socket of each flavor, in priority order.
pub fn sockets(home: &Path) -> Vec<(Flavor, PathBuf)> {
    Flavor::ALL.iter().map(|f| (*f, home.join(f.home_dir()).join("run").join("bleep.sock"))).collect()
}

/// True when no app before this one in priority order is running, so this app answers agents
/// (Claude Code permissions and replies, Codex approvals).
pub fn answers_agents(home: &Path) -> bool {
    first_live(&sockets(home), |p| std::os::unix::net::UnixStream::connect(p).is_ok()).map_or(true, |f| f == FLAVOR)
}

fn first_live(socks: &[(Flavor, PathBuf)], live: impl Fn(&Path) -> bool) -> Option<Flavor> {
    socks.iter().find(|(_, p)| live(p)).map(|(f, _)| *f)
}

/// A first nightly or local run starts from a copy of the production data, so Usage and the chat have real
/// history to show. Copies the database (a consistent snapshot), config, agent rules and memory.
/// Does nothing when the dev data already exists or there is no production data. Returns true
/// when it seeded.
pub fn seed_from(prod: &Path, dev: &Path) -> anyhow::Result<bool> {
    let (src_db, dst_db) = (prod.join("bleep.db"), dev.join("bleep.db"));
    if dst_db.exists() || !src_db.exists() {
        return Ok(false);
    }
    std::fs::create_dir_all(dev)?;
    let conn = rusqlite::Connection::open_with_flags(&src_db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.execute("VACUUM INTO ?1", [dst_db.to_string_lossy()])?;
    for f in ["config.toml", "agent-rules.toml"] {
        if prod.join(f).exists() {
            std::fs::copy(prod.join(f), dev.join(f))?;
        }
    }
    copy_dir(&prod.join("memory"), &dev.join("memory"))?;
    Ok(true)
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    if !from.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let p = e.path();
        if e.file_type()?.is_dir() {
            copy_dir(&p, &to.join(e.file_name()))?;
        } else {
            std::fs::copy(&p, to.join(e.file_name()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_once_from_production_data() {
        let t = tempfile::tempdir().unwrap();
        let (prod, dev) = (t.path().join("prod"), t.path().join("dev"));
        std::fs::create_dir_all(prod.join("memory/daily")).unwrap();
        let c = rusqlite::Connection::open(prod.join("bleep.db")).unwrap();
        c.execute_batch("CREATE TABLE x (v TEXT); INSERT INTO x VALUES ('hi');").unwrap();
        drop(c);
        std::fs::write(prod.join("config.toml"), "runtime = \"claude\"\n").unwrap();
        std::fs::write(prod.join("memory/USER.md"), "me").unwrap();
        std::fs::write(prod.join("memory/daily/2026-11-17.md"), "- note").unwrap();

        assert!(seed_from(&prod, &dev).unwrap());
        let c = rusqlite::Connection::open(dev.join("bleep.db")).unwrap();
        let v: String = c.query_row("SELECT v FROM x", [], |r| r.get(0)).unwrap();
        assert_eq!(v, "hi");
        assert_eq!(std::fs::read_to_string(dev.join("memory/daily/2026-11-17.md")).unwrap(), "- note");
        assert!(dev.join("config.toml").exists());
        assert!(!dev.join("agent-rules.toml").exists());

        std::fs::write(dev.join("memory/USER.md"), "changed in dev").unwrap();
        assert!(!seed_from(&prod, &dev).unwrap(), "a second run must not overwrite dev data");
        assert_eq!(std::fs::read_to_string(dev.join("memory/USER.md")).unwrap(), "changed in dev");
    }

    #[test]
    fn first_running_app_answers() {
        let socks = sockets(Path::new("/h"));
        assert_eq!(socks[0].1, PathBuf::from("/h/.bleep/run/bleep.sock"));
        assert_eq!(socks[2].1, PathBuf::from("/h/.bleep-local/run/bleep.sock"));
        let only = |name: &'static str| move |p: &Path| p.starts_with(format!("/h/{name}/"));
        assert_eq!(first_live(&socks, only(".bleep-nightly")), Some(Flavor::Nightly));
        assert_eq!(first_live(&socks, |_| true), Some(Flavor::Prod));
        assert_eq!(first_live(&socks, |_| false), None);
    }

    #[test]
    fn no_production_data_means_no_seed() {
        let t = tempfile::tempdir().unwrap();
        assert!(!seed_from(&t.path().join("none"), &t.path().join("dev")).unwrap());
    }
}
