//! Every remote server I added: add (with auth discovery), sign in again, tool modes,
//! remove, and the list Settings shows. Tauri commands call this; it knows nothing of Tauri.
use crate::config::{McpAuth, McpServer, ToolMode};
use crate::connectors::{Connector, ConnectorStatus};
use crate::mcp::host::ToolHost;
use crate::opener::Opener;
use crate::remote::auth::{forget, sign_in, SignIn, SIGN_IN_WAIT};
use crate::remote::client::{probe, Probe, RemoteConnector};
use crate::remote::names::{check_server_name, check_url, server_name_from};
use crate::remote::secrets::{Client, Secrets, ServerSecrets};
use crate::store::Store;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const CANCELLED: &str = "Sign-in was cancelled.";
pub const BUSY: &str = "Another sign-in is still open. Finish or cancel it first.";
const DESC_CHARS: usize = 200;

#[derive(Clone, Debug, Deserialize)]
pub struct AddRequest {
    pub url: String,
    pub name: Option<String>,
    pub token: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    /// Added and connected. The caller saves this to config.
    Added(McpServer),
    /// OAuth without dynamic registration: ask me for a client ID.
    NeedsClientId,
    /// Sign-in needed but no OAuth found: ask me for a token.
    NeedsToken { message: String },
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ToolView {
    pub name: String,
    pub description: String,
    pub mode: ToolMode,
    pub default_mode: ToolMode,
    /// Why bleep leaves this tool out, if it does.
    pub skipped: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ServerView {
    pub name: String,
    pub url: String,
    pub auth: McpAuth,
    pub enabled: bool,
    pub status: ConnectorStatus,
    pub tools: Vec<ToolView>,
}

fn nonempty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

use crate::remote::tools::default_mode;

pub struct Remotes {
    host: Arc<ToolHost>,
    pub(crate) store: Arc<Store>,
    secrets: Arc<dyn Secrets>,
    opener: Arc<dyn Opener>,
    own_port: Option<u16>,
    wait: Mutex<Duration>,
    list: Mutex<Vec<Arc<RemoteConnector>>>,
    signing: Mutex<Option<tokio::task::AbortHandle>>,
}

impl Remotes {
    pub fn new(host: Arc<ToolHost>, store: Arc<Store>, secrets: Arc<dyn Secrets>, opener: Arc<dyn Opener>, own_port: Option<u16>) -> Arc<Remotes> {
        Arc::new(Remotes { host, store, secrets, opener, own_port, wait: Mutex::new(SIGN_IN_WAIT), list: Mutex::default(), signing: Mutex::default() })
    }

    pub fn with_sign_in_wait(self: Arc<Self>, d: Duration) -> Arc<Self> {
        *self.wait.lock().unwrap() = d;
        self
    }

    fn find(&self, name: &str) -> Result<Arc<RemoteConnector>, String> {
        self.list.lock().unwrap().iter().find(|c| c.id() == name).cloned().ok_or_else(|| format!("No server named {name}."))
    }

    /// Builds the configured servers from the cache. Enabled ones give tools at once.
    pub fn load(&self, servers: &[McpServer]) {
        for s in servers {
            let c = RemoteConnector::new(s.clone(), self.store.clone(), self.secrets.clone());
            if s.enabled {
                self.host.replace(c.clone());
            }
            self.list.lock().unwrap().push(c);
        }
    }

    /// Connects every enabled server, one after another. Failures only set their status.
    pub async fn connect_all(&self) {
        let list: Vec<_> = self.list.lock().unwrap().iter().filter(|c| c.config().enabled).cloned().collect();
        for c in list {
            let _ = c.connect().await;
        }
    }

    /// Runs one sign-in at a time, and lets `cancel_sign_in` stop it.
    async fn guarded<T: Send + 'static>(&self, f: impl std::future::Future<Output = Result<T, String>> + Send + 'static) -> Result<T, String> {
        let task = {
            let mut slot = self.signing.lock().unwrap();
            if slot.is_some() {
                return Err(BUSY.into());
            }
            let task = tokio::spawn(f);
            *slot = Some(task.abort_handle());
            task
        };
        let res = task.await;
        *self.signing.lock().unwrap() = None;
        match res {
            Ok(r) => r,
            Err(e) if e.is_cancelled() => Err(CANCELLED.into()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn cancel_sign_in(&self) -> bool {
        match self.signing.lock().unwrap().take() {
            Some(h) => {
                h.abort();
                true
            }
            None => false,
        }
    }

    /// Finds how the server signs in (open, token, OAuth), signs in, connects and adds it.
    /// A failed or cancelled add leaves no secret and no tools behind.
    pub async fn add(self: &Arc<Self>, req: AddRequest, taken: &[&str]) -> Result<Outcome, String> {
        let url = check_url(&req.url, self.own_port)?.to_string();
        let name = nonempty(req.name.clone()).unwrap_or_else(|| server_name_from(&url));
        let mine: Vec<String> = self.list.lock().unwrap().iter().map(|c| c.id().to_string()).collect();
        let all: Vec<&str> = taken.iter().copied().chain(mine.iter().map(String::as_str)).collect();
        check_server_name(&name, &all)?;
        let me = self.clone();
        let n = name.clone();
        let res = self.guarded(async move { me.add_inner(url, n, req).await }).await;
        if !matches!(res, Ok(Outcome::Added(_))) {
            let _ = ServerSecrets::new(self.secrets.clone(), &name).clear_all();
            let _ = self.store.delete_mcp_tools(&name);
        }
        res
    }

    async fn add_inner(self: Arc<Self>, url: String, name: String, req: AddRequest) -> Result<Outcome, String> {
        let ss = ServerSecrets::new(self.secrets.clone(), &name);
        let e = |e: anyhow::Error| e.to_string();
        let auth = if let Some(t) = nonempty(req.token) {
            ss.set_token(&t).map_err(e)?;
            McpAuth::Token
        } else {
            match probe(&url).await {
                Probe::Open => McpAuth::None,
                Probe::Unreachable(why) => return Err(format!("Could not reach {name}. {why}")),
                Probe::AuthRequired { challenge } => {
                    if let Some(id) = nonempty(req.client_id) {
                        ss.set_client(&Client { id, secret: nonempty(req.client_secret) }).map_err(e)?;
                    }
                    let wait = *self.wait.lock().unwrap();
                    match sign_in(&url, &name, self.secrets.clone(), self.opener.as_ref(), challenge, wait).await {
                        Ok(SignIn::Done) => McpAuth::Oauth,
                        Ok(SignIn::NeedsClientId) => return Ok(Outcome::NeedsClientId),
                        Err(m) if m.starts_with("Could not find how") => {
                            return Ok(Outcome::NeedsToken { message: format!("{name} needs sign-in, but it does not offer a sign-in bleep can use. Paste a token for it instead.") })
                        }
                        Err(m) => return Err(m),
                    }
                }
            }
        };
        let cfg = McpServer { name, url, enabled: true, auth, tools: Default::default() };
        let conn = RemoteConnector::new(cfg.clone(), self.store.clone(), self.secrets.clone());
        conn.connect().await?;
        self.host.replace(conn.clone());
        self.list.lock().unwrap().push(conn);
        Ok(Outcome::Added(cfg))
    }

    /// Signs in again (OAuth), or saves a new token, then reconnects.
    pub async fn sign_in_again(self: &Arc<Self>, name: &str, token: Option<String>) -> Result<(), String> {
        let c = self.find(name)?;
        let cfg = c.config();
        if let Some(t) = nonempty(token) {
            c.server_secrets().set_token(&t).map_err(|e| e.to_string())?;
            return c.connect().await;
        }
        if cfg.auth != McpAuth::Oauth {
            return c.connect().await;
        }
        let (secrets, opener, wait) = (self.secrets.clone(), self.opener.clone(), *self.wait.lock().unwrap());
        let (url, n) = (cfg.url.clone(), cfg.name.clone());
        self.guarded(async move {
            let challenge = match probe(&url).await {
                Probe::AuthRequired { challenge } => challenge,
                _ => None,
            };
            match sign_in(&url, &n, secrets, opener.as_ref(), challenge, wait).await? {
                SignIn::Done => Ok(()),
                SignIn::NeedsClientId => Err(format!("{n} needs a client ID. Remove it and add it again with one.")),
            }
        })
        .await?;
        c.connect().await
    }

    /// Turns a server on (its tools come back, and it connects) or off (its tools go, its
    /// sign-in stays). Returns the config to save.
    pub async fn set_enabled(&self, name: &str, on: bool) -> Result<McpServer, String> {
        let c = self.find(name)?;
        c.set_enabled(on);
        if on {
            self.host.replace(c.clone());
            if let Err(e) = c.connect().await {
                eprintln!("bleep: connect {name}: {e}");
            }
        } else {
            self.host.remove(name);
        }
        Ok(c.config())
    }

    /// The server added for this URL, if any.
    pub fn by_url(&self, url: &str) -> Option<McpServer> {
        let norm = |u: &str| u.trim_end_matches('/').to_ascii_lowercase();
        self.list.lock().unwrap().iter().map(|c| c.config()).find(|c| norm(&c.url) == norm(url))
    }

    pub async fn refresh(&self, name: &str) -> Result<(), String> {
        self.find(name)?.refresh_tools().await
    }

    /// Sets my mode for one tool and returns the config to save.
    pub fn set_mode(&self, name: &str, tool: &str, mode: Option<ToolMode>) -> Result<McpServer, String> {
        let c = self.find(name)?;
        c.set_mode(tool, mode);
        Ok(c.config())
    }

    /// Removes the server's tools, revokes and clears its secrets, and drops its cache.
    pub async fn remove(&self, name: &str) -> Result<(), String> {
        let c = self.find(name)?;
        self.host.remove(name);
        self.list.lock().unwrap().retain(|x| x.id() != name);
        forget(&c.config().url, name, self.secrets.clone()).await;
        // A cache row left behind is harmless; the server is gone from config either way.
        if let Err(e) = self.store.delete_mcp_tools(name) {
            eprintln!("bleep: remove {name}: cached tools stay: {e}");
        }
        Ok(())
    }

    pub fn views(&self) -> Vec<ServerView> {
        let list = self.list.lock().unwrap().clone();
        list.iter()
            .map(|c| {
                let cfg = c.config();
                let set = c.tool_set();
                let tools = self
                    .store
                    .mcp_tools(&cfg.name)
                    .unwrap_or_default()
                    .iter()
                    .map(|t| {
                        let name = t["name"].as_str().unwrap_or("").to_string();
                        let def = default_mode(t);
                        let description: String = t["description"].as_str().unwrap_or("").trim().chars().take(DESC_CHARS).collect();
                        let skipped = set.skipped.iter().find(|s| s.tool == name).map(|s| s.reason.clone());
                        ToolView { mode: cfg.mode(&name).unwrap_or(def), default_mode: def, name, description, skipped }
                    })
                    .collect();
                ServerView { name: cfg.name.clone(), url: cfg.url.clone(), auth: cfg.auth, enabled: cfg.enabled, status: c.status(), tools }
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;
