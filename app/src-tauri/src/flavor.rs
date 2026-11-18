//! Which build this is. A dev build ("bleep (dev)", cargo feature `dev`) installs next to the
//! production app and must never touch its data, its Keychain items, its login item, its
//! updates, or the Claude Code and Codex settings the production app manages.
use std::path::Path;

pub const DEV: bool = cfg!(feature = "dev");

/// The name people see: windows, tray, notifications.
pub const NAME: &str = if DEV { "bleep (dev)" } else { "bleep" };

/// Keychain service for Google, remote MCP and API key secrets.
pub const KEYCHAIN_SERVICE: &str = if DEV { "site.avirooppaul.bleep.dev" } else { "site.avirooppaul.bleep" };

/// Data folder under the home folder.
pub const HOME_DIR: &str = if DEV { ".bleep-dev" } else { ".bleep" };

/// The production data folder, which a dev build reads once to seed itself.
pub const PROD_HOME_DIR: &str = ".bleep";

/// A first dev run starts from a copy of the production data, so Usage and the chat have real
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
    fn no_production_data_means_no_seed() {
        let t = tempfile::tempdir().unwrap();
        assert!(!seed_from(&t.path().join("none"), &t.path().join("dev")).unwrap());
    }
}
