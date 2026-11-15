//! Remote server secrets in the Keychain: rmcp's OAuth credentials, a client I registered
//! by hand, and a pasted token. Nothing here reaches config, SQLite or logs.
pub use crate::google::secrets::Secrets;
use async_trait::async_trait;
use rmcp::transport::auth::{AuthError, CredentialStore, StoredCredentials};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

fn key(server: &str) -> String {
    format!("mcp:{server}")
}

/// rmcp's OAuth credentials for one server, kept at `mcp:<server>`.
pub struct KeychainCredentials {
    secrets: Arc<dyn Secrets>,
    server: String,
}

impl KeychainCredentials {
    pub fn new(secrets: Arc<dyn Secrets>, server: &str) -> KeychainCredentials {
        KeychainCredentials { secrets, server: server.into() }
    }
}

fn store_err(e: impl std::fmt::Display) -> AuthError {
    AuthError::CredentialStoreError(e.to_string())
}

#[async_trait]
impl CredentialStore for KeychainCredentials {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        Ok(self.secrets.get(&key(&self.server)).and_then(|s| serde_json::from_str(&s).ok()))
    }

    async fn save(&self, credentials: StoredCredentials) -> Result<(), AuthError> {
        let s = serde_json::to_string(&credentials).map_err(store_err)?;
        self.secrets.set(&key(&self.server), &s).map_err(store_err)
    }

    async fn clear(&self) -> Result<(), AuthError> {
        self.secrets.delete(&key(&self.server)).map_err(store_err)
    }
}

/// A client ID (and secret) I typed for a server with no dynamic registration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Client {
    pub id: String,
    pub secret: Option<String>,
}

/// The other secrets of one server: a pasted token and a hand-registered client.
pub struct ServerSecrets {
    secrets: Arc<dyn Secrets>,
    server: String,
}

impl ServerSecrets {
    pub fn new(secrets: Arc<dyn Secrets>, server: &str) -> ServerSecrets {
        ServerSecrets { secrets, server: server.into() }
    }

    pub fn token(&self) -> Option<String> {
        self.secrets.get(&format!("{}:token", key(&self.server))).filter(|t| !t.is_empty())
    }

    pub fn set_token(&self, t: &str) -> anyhow::Result<()> {
        self.secrets.set(&format!("{}:token", key(&self.server)), t)
    }

    pub fn client(&self) -> Option<Client> {
        self.secrets.get(&format!("{}:client", key(&self.server))).and_then(|s| serde_json::from_str(&s).ok())
    }

    pub fn set_client(&self, c: &Client) -> anyhow::Result<()> {
        self.secrets.set(&format!("{}:client", key(&self.server)), &serde_json::to_string(c)?)
    }

    /// Removes every secret of this server.
    pub fn clear_all(&self) -> anyhow::Result<()> {
        let k = key(&self.server);
        for k in [k.clone(), format!("{k}:token"), format!("{k}:client")] {
            self.secrets.delete(&k)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::google::secrets::MemorySecrets;
    use rmcp::transport::auth::{CredentialStore, StoredCredentials};

    #[tokio::test]
    async fn credentials_save_load_clear() {
        let s: Arc<dyn Secrets> = Arc::new(MemorySecrets::default());
        let store = KeychainCredentials::new(s.clone(), "dayblocks");
        assert!(store.load().await.unwrap().is_none());
        store.save(StoredCredentials::new("client-1".into(), None, vec!["read".into()], Some(1))).await.unwrap();
        let back = store.load().await.unwrap().unwrap();
        assert_eq!((back.client_id.as_str(), back.granted_scopes.as_slice()), ("client-1", &["read".to_string()][..]));
        assert!(s.get("mcp:dayblocks").is_some());
        assert!(s.get("mcp:wiki").is_none(), "per server");
        store.clear().await.unwrap();
        assert!(store.load().await.unwrap().is_none());
    }

    #[test]
    fn token_and_client_and_clear_all() {
        let s: Arc<dyn Secrets> = Arc::new(MemorySecrets::default());
        let k = ServerSecrets::new(s.clone(), "dayblocks");
        assert_eq!(k.token(), None);
        k.set_token("tok").unwrap();
        k.set_client(&Client { id: "cid".into(), secret: Some("cs".into()) }).unwrap();
        assert_eq!(k.token().as_deref(), Some("tok"));
        assert_eq!(k.client(), Some(Client { id: "cid".into(), secret: Some("cs".into()) }));
        s.set("mcp:dayblocks", "{}").unwrap();
        s.set("mcp:wiki:token", "other").unwrap();
        k.clear_all().unwrap();
        assert!(k.token().is_none() && k.client().is_none() && s.get("mcp:dayblocks").is_none());
        assert_eq!(s.get("mcp:wiki:token").as_deref(), Some("other"), "other servers stay");
    }

    #[test]
    fn broken_json_reads_as_none() {
        let s: Arc<dyn Secrets> = Arc::new(MemorySecrets::default());
        s.set("mcp:dayblocks:client", "not json").unwrap();
        assert_eq!(ServerSecrets::new(s, "dayblocks").client(), None);
    }
}
