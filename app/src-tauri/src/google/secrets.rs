//! Secrets live in the macOS Keychain; tests use memory.
pub const SERVICE: &str = "site.avirooppaul.bleep";

pub trait Secrets: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, v: &str) -> anyhow::Result<()>;
    fn delete(&self, key: &str) -> anyhow::Result<()>;
}

pub struct KeychainSecrets;

impl Secrets for KeychainSecrets {
    fn get(&self, key: &str) -> Option<String> {
        keyring::Entry::new(SERVICE, key).ok()?.get_password().ok()
    }

    fn set(&self, key: &str, v: &str) -> anyhow::Result<()> {
        keyring::Entry::new(SERVICE, key)?.set_password(v)?;
        Ok(())
    }

    fn delete(&self, key: &str) -> anyhow::Result<()> {
        match keyring::Entry::new(SERVICE, key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
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
