//! Secrets live in the macOS Keychain; tests use memory.
//!
//! All of bleep's secrets are in ONE Keychain item (`vault`, a JSON map), read once per
//! launch and kept in memory. bleep's signing certificate has no Apple Team ID, so macOS
//! trusts each build by its code hash and asks again after every update, once per item.
//! One item means at most one prompt per update. Older builds kept one item per secret;
//! each moves into the vault the first time it is read, and the old item is deleted.
use std::collections::{BTreeMap, HashSet};
use std::sync::{Mutex, OnceLock};

pub const SERVICE: &str = "site.avirooppaul.bleep";
const VAULT: &str = "vault";

pub trait Secrets: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, v: &str) -> anyhow::Result<()>;
    fn delete(&self, key: &str) -> anyhow::Result<()>;
}

/// Raw Keychain items under `SERVICE`, by account name.
pub trait Items: Send + Sync {
    fn read(&self, account: &str) -> Option<String>;
    fn write(&self, account: &str, v: &str) -> anyhow::Result<()>;
    fn remove(&self, account: &str) -> anyhow::Result<()>;
}

struct KeyringItems;

impl Items for KeyringItems {
    fn read(&self, account: &str) -> Option<String> {
        keyring::Entry::new(SERVICE, account).ok()?.get_password().ok()
    }

    fn write(&self, account: &str, v: &str) -> anyhow::Result<()> {
        keyring::Entry::new(SERVICE, account)?.set_password(v)?;
        Ok(())
    }

    fn remove(&self, account: &str) -> anyhow::Result<()> {
        match keyring::Entry::new(SERVICE, account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[derive(Default)]
struct State {
    map: Option<BTreeMap<String, String>>,
    /// Keys already looked for as old single items this launch.
    legacy_checked: HashSet<String>,
}

pub struct Vault<I: Items> {
    items: I,
    state: Mutex<State>,
}

impl<I: Items> Vault<I> {
    pub fn new(items: I) -> Self {
        Self { items, state: Mutex::new(State::default()) }
    }

    fn loaded<'a>(&self, st: &'a mut State) -> &'a mut BTreeMap<String, String> {
        st.map.get_or_insert_with(|| self.items.read(VAULT).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default())
    }

    fn save(&self, map: &BTreeMap<String, String>) -> anyhow::Result<()> {
        self.items.write(VAULT, &serde_json::to_string(map)?)
    }

    /// Moves an old single item into the vault, once per key per launch.
    fn migrate(&self, st: &mut State, key: &str) {
        if key == VAULT || !st.legacy_checked.insert(key.to_string()) {
            return;
        }
        let Some(v) = self.items.read(key) else { return };
        let mut next = self.loaded(st).clone();
        next.insert(key.to_string(), v);
        if self.save(&next).is_ok() {
            *self.loaded(st) = next;
            let _ = self.items.remove(key);
        } else {
            // Keep using the old item this launch; try again next launch.
            self.loaded(st).insert(key.to_string(), self.items.read(key).unwrap_or_default());
        }
    }
}

impl<I: Items> Secrets for Vault<I> {
    fn get(&self, key: &str) -> Option<String> {
        let mut st = self.state.lock().unwrap();
        if !self.loaded(&mut st).contains_key(key) {
            self.migrate(&mut st, key);
        }
        self.loaded(&mut st).get(key).cloned()
    }

    fn set(&self, key: &str, v: &str) -> anyhow::Result<()> {
        let mut st = self.state.lock().unwrap();
        st.legacy_checked.insert(key.to_string());
        let mut next = self.loaded(&mut st).clone();
        next.insert(key.to_string(), v.to_string());
        self.save(&next)?;
        *self.loaded(&mut st) = next;
        Ok(())
    }

    fn delete(&self, key: &str) -> anyhow::Result<()> {
        let mut st = self.state.lock().unwrap();
        let mut next = self.loaded(&mut st).clone();
        if next.remove(key).is_some() {
            self.save(&next)?;
            *self.loaded(&mut st) = next;
        }
        // An old single item of the same name goes too (once checked, it is gone or never was).
        if st.legacy_checked.insert(key.to_string()) {
            self.items.remove(key)?;
        }
        Ok(())
    }
}

/// The Keychain, through one shared vault for the whole app (every handle sees the same cache).
pub struct KeychainSecrets;

fn vault() -> &'static Vault<KeyringItems> {
    static V: OnceLock<Vault<KeyringItems>> = OnceLock::new();
    V.get_or_init(|| Vault::new(KeyringItems))
}

impl Secrets for KeychainSecrets {
    fn get(&self, key: &str) -> Option<String> {
        vault().get(key)
    }

    fn set(&self, key: &str, v: &str) -> anyhow::Result<()> {
        vault().set(key, v)
    }

    fn delete(&self, key: &str) -> anyhow::Result<()> {
        vault().delete(key)
    }
}

#[derive(Default)]
pub struct MemorySecrets(std::sync::Mutex<std::collections::HashMap<String, String>>);

impl Secrets for MemorySecrets {
    fn get(&self, key: &str) -> Option<String> {
        self.0.lock().unwrap().get(key).cloned()
    }

    fn set(&self, key: &str, v: &str) -> anyhow::Result<()> {
        self.0.lock().unwrap().insert(key.into(), v.into());
        Ok(())
    }

    fn delete(&self, key: &str) -> anyhow::Result<()> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;

    /// Keychain items in memory, counting reads (each read of a real item can prompt).
    #[derive(Default, Clone)]
    struct FakeItems {
        items: Arc<Mutex<HashMap<String, String>>>,
        reads: Arc<Mutex<Vec<String>>>,
        fail_writes: Arc<std::sync::atomic::AtomicBool>,
    }

    impl Items for FakeItems {
        fn read(&self, a: &str) -> Option<String> {
            self.reads.lock().unwrap().push(a.into());
            self.items.lock().unwrap().get(a).cloned()
        }
        fn write(&self, a: &str, v: &str) -> anyhow::Result<()> {
            if self.fail_writes.load(std::sync::atomic::Ordering::SeqCst) {
                anyhow::bail!("denied");
            }
            self.items.lock().unwrap().insert(a.into(), v.into());
            Ok(())
        }
        fn remove(&self, a: &str) -> anyhow::Result<()> {
            self.items.lock().unwrap().remove(a);
            Ok(())
        }
    }

    fn item_reads(f: &FakeItems, a: &str) -> usize {
        f.reads.lock().unwrap().iter().filter(|r| *r == a).count()
    }

    #[test]
    fn everything_lives_in_one_item_read_once_per_launch() {
        let f = FakeItems::default();
        let v = Vault::new(f.clone());
        v.set("google-refresh-token", "r1").unwrap();
        v.set("api:openrouter:key", "k1").unwrap();
        let names: Vec<String> = f.items.lock().unwrap().keys().cloned().collect();
        assert_eq!(names, ["vault"], "one Keychain item");
        f.reads.lock().unwrap().clear();
        let next_launch = Vault::new(f.clone());
        for _ in 0..3 {
            assert_eq!(next_launch.get("google-refresh-token").as_deref(), Some("r1"));
            assert_eq!(next_launch.get("api:openrouter:key").as_deref(), Some("k1"));
        }
        assert_eq!(item_reads(&f, "vault"), 1, "the vault is read once, then served from memory");
    }

    #[test]
    fn old_single_items_move_into_the_vault_and_are_deleted() {
        let f = FakeItems::default();
        f.items.lock().unwrap().insert("google-account".into(), "me@example.com".into());
        f.items.lock().unwrap().insert("mcp:dayblocks".into(), "{}".into());
        let v = Vault::new(f.clone());
        assert_eq!(v.get("google-account").as_deref(), Some("me@example.com"));
        assert_eq!(v.get("mcp:dayblocks").as_deref(), Some("{}"));
        let mut names: Vec<String> = f.items.lock().unwrap().keys().cloned().collect();
        names.sort();
        assert_eq!(names, ["vault"], "old items are gone");
        let again = Vault::new(f.clone());
        assert_eq!(again.get("google-account").as_deref(), Some("me@example.com"));
        assert_eq!(item_reads(&f, "google-account"), 1, "an old item is read once, ever");
    }

    #[test]
    fn a_missing_key_is_looked_for_once_per_launch() {
        let f = FakeItems::default();
        let v = Vault::new(f.clone());
        for _ in 0..5 {
            assert!(v.get("api:anthropic:key").is_none());
        }
        assert_eq!(item_reads(&f, "api:anthropic:key"), 1);
    }

    #[test]
    fn delete_removes_from_the_vault_and_any_old_item() {
        let f = FakeItems::default();
        f.items.lock().unwrap().insert("mcp:wiki:token".into(), "old".into());
        let v = Vault::new(f.clone());
        v.set("google-account", "a").unwrap();
        v.delete("mcp:wiki:token").unwrap();
        v.delete("google-account").unwrap();
        assert!(v.get("google-account").is_none() && v.get("mcp:wiki:token").is_none());
        let names: Vec<String> = f.items.lock().unwrap().keys().cloned().collect();
        assert_eq!(names, ["vault"]);
        assert!(Vault::new(f.clone()).get("google-account").is_none(), "gone after a restart too");
    }

    #[test]
    fn a_failed_write_keeps_the_old_values() {
        let f = FakeItems::default();
        let v = Vault::new(f.clone());
        v.set("a", "1").unwrap();
        f.fail_writes.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(v.set("a", "2").is_err());
        assert_eq!(v.get("a").as_deref(), Some("1"), "memory matches the Keychain");
    }
}
