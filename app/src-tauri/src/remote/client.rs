//! One remote MCP server as a bleep connector. The connection opens on first use and again
//! after a failure. A failed `tools/call` is never retried inside the call, so a write can
//! not run twice.
use crate::config::{McpAuth, McpServer, ToolMode};
use crate::connectors::{Connector, ConnectorStatus, ToolSpec};
use crate::remote::secrets::{Secrets, ServerSecrets};
use crate::remote::tools::{map_tools, wrap_result, ToolSet};
use crate::store::Store;
use async_trait::async_trait;
use rmcp::model::{CallToolRequestParams, ClientCapabilities, ClientConfig, Implementation};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::{Peer, ServiceError, ServiceExt};
use serde_json::{json, Value};
use std::sync::{Arc, RwLock};
use std::time::Duration;

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
pub const CALL_TIMEOUT: Duration = Duration::from_secs(120);
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

type Live = RunningService<RoleClient, ClientConfig>;

/// What a plain `initialize` says about a server.
#[derive(Debug, PartialEq)]
pub enum Probe {
    Open,
    /// 401 or 403, with the `WWW-Authenticate` header if there was one.
    AuthRequired { challenge: Option<String> },
    Unreachable(String),
}

pub fn client_info() -> ClientConfig {
    ClientConfig::new(ClientCapabilities::default(), Implementation::new("bleep", env!("CARGO_PKG_VERSION")))
}

pub async fn probe(url: &str) -> Probe {
    probe_with(url, None).await
}

/// Sends one `initialize`, with a bearer token if given, and reads only the status.
pub async fn probe_with(url: &str, bearer: Option<&str>) -> Probe {
    let body = json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "bleep", "version": env!("CARGO_PKG_VERSION")}}});
    let http = reqwest::Client::builder().timeout(PROBE_TIMEOUT).build().unwrap_or_default();
    let mut req = http.post(url).header("Accept", "application/json, text/event-stream").json(&body);
    if let Some(t) = bearer {
        req = req.bearer_auth(t);
    }
    let res = match req.send().await {
        Ok(r) => r,
        Err(e) => return Probe::Unreachable(if e.is_timeout() { "It did not answer within 10 seconds.".into() } else { "It could not be reached.".into() }),
    };
    let status = res.status().as_u16();
    let challenge = res.headers().get("www-authenticate").and_then(|v| v.to_str().ok()).map(str::to_string);
    // A stateful server opened a session for this probe: close it.
    if let Some(sid) = res.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()).map(str::to_string) {
        let _ = http.delete(url).header("Mcp-Session-Id", sid).send().await;
    }
    match status {
        200..=299 => Probe::Open,
        401 | 403 => Probe::AuthRequired { challenge },
        s => Probe::Unreachable(format!("It answered with HTTP {s}.")),
    }
}

pub struct RemoteConnector {
    name: String,
    cfg: RwLock<McpServer>,
    store: Arc<Store>,
    secrets: Arc<dyn Secrets>,
    set: RwLock<ToolSet>,
    status: RwLock<ConnectorStatus>,
    live: tokio::sync::Mutex<Option<Live>>,
}

impl RemoteConnector {
    /// Builds the connector from config and the cached tool list. It connects on first use.
    pub fn new(cfg: McpServer, store: Arc<Store>, secrets: Arc<dyn Secrets>) -> Arc<RemoteConnector> {
        let raw = store.mcp_tools(&cfg.name).unwrap_or_default();
        let set = map_tools(&cfg.name, &raw, &cfg.tools);
        Arc::new(RemoteConnector {
            name: cfg.name.clone(),
            cfg: RwLock::new(cfg),
            store,
            secrets,
            set: RwLock::new(set),
            status: RwLock::new(ConnectorStatus::Error { message: "Not connected yet.".into() }),
            live: tokio::sync::Mutex::new(None),
        })
    }

    pub fn config(&self) -> McpServer {
        self.cfg.read().unwrap().clone()
    }

    pub fn tool_set(&self) -> ToolSet {
        self.set.read().unwrap().clone()
    }

    pub fn server_secrets(&self) -> ServerSecrets {
        ServerSecrets::new(self.secrets.clone(), &self.name)
    }

    fn host(&self) -> String {
        reqwest::Url::parse(&self.config().url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default()
    }

    fn remap(&self) {
        let cfg = self.config();
        let raw = self.store.mcp_tools(&cfg.name).unwrap_or_default();
        *self.set.write().unwrap() = map_tools(&cfg.name, &raw, &cfg.tools);
    }

    /// Sets (or clears, with `None`) my mode for one of the server's tools.
    pub fn set_mode(&self, tool: &str, mode: Option<ToolMode>) {
        {
            let mut c = self.cfg.write().unwrap();
            match mode {
                Some(m) => c.tools.insert(tool.to_string(), m),
                None => c.tools.remove(tool),
            };
        }
        self.remap();
    }

    fn set_status(&self, s: ConnectorStatus) {
        *self.status.write().unwrap() = s;
    }

    fn sign_in_text(&self) -> String {
        format!("{} needs sign-in. Ask the user to sign in again in Settings > Connectors.", self.name)
    }

    fn bearer(&self) -> Option<String> {
        match self.config().auth {
            McpAuth::Token => self.server_secrets().token(),
            _ => None,
        }
    }

    async fn open(&self) -> Result<Live, String> {
        let cfg = self.config();
        let mut tc = StreamableHttpClientTransportConfig::with_uri(cfg.url.clone());
        if let Some(t) = self.bearer() {
            tc = tc.auth_header(t);
        }
        let transport = StreamableHttpClientTransport::from_config(tc);
        client_info().serve(transport).await.map_err(|e| e.to_string())
    }

    /// Works out why the server failed, sets the status, and gives the text for the model.
    async fn failed(&self, detail: &str) -> String {
        match probe_with(&self.config().url, self.bearer().as_deref()).await {
            Probe::AuthRequired { .. } => {
                self.set_status(ConnectorStatus::NeedsSignIn);
                self.sign_in_text()
            }
            Probe::Unreachable(why) => {
                let msg = format!("Could not reach {}. {why}", self.name);
                self.set_status(ConnectorStatus::Error { message: msg.clone() });
                msg
            }
            Probe::Open => {
                let msg = format!("Could not reach {}: {detail}", self.name);
                self.set_status(ConnectorStatus::Error { message: msg.clone() });
                msg
            }
        }
    }

    /// Opens the connection and refreshes the tool list (and its cache).
    pub async fn connect(&self) -> Result<(), String> {
        let mut live = self.live.lock().await;
        *live = None;
        let svc = match tokio::time::timeout(CONNECT_TIMEOUT, self.open()).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => return Err(self.failed(&e).await),
            Err(_) => return Err(self.failed("it did not answer within 20 seconds").await),
        };
        let tools = match tokio::time::timeout(CONNECT_TIMEOUT, svc.peer().list_all_tools()).await {
            Ok(Ok(t)) => t,
            Ok(Err(e)) => return Err(self.failed(&e.to_string()).await),
            Err(_) => return Err(self.failed("its tool list did not come within 20 seconds").await),
        };
        let raw: Vec<Value> = tools.iter().filter_map(|t| serde_json::to_value(t).ok()).collect();
        self.store.replace_mcp_tools(&self.name, &raw).map_err(|e| e.to_string())?;
        self.remap();
        self.set_status(ConnectorStatus::Connected { account: self.host() });
        *live = Some(svc);
        Ok(())
    }

    async fn peer(&self) -> Result<Peer<RoleClient>, String> {
        if let Some(l) = self.live.lock().await.as_ref() {
            return Ok(l.peer().clone());
        }
        self.connect().await?;
        self.live.lock().await.as_ref().map(|l| l.peer().clone()).ok_or_else(|| format!("Could not reach {}.", self.name))
    }

    fn remote_name(&self, tool: &str) -> Result<String, String> {
        self.set.read().unwrap().tools.iter().find(|t| t.spec.name == tool).map(|t| t.remote.clone()).ok_or_else(|| format!("Unknown tool: {tool}"))
    }

    async fn run(&self, remote: &str, args: Value) -> Result<Value, String> {
        let peer = self.peer().await?;
        let mut params = CallToolRequestParams::new(remote.to_string());
        if let Value::Object(o) = args {
            params = params.with_arguments(o);
        }
        match tokio::time::timeout(CALL_TIMEOUT, peer.call_tool(params)).await {
            Ok(Ok(r)) => wrap_result(&self.name, &serde_json::to_value(r).map_err(|e| e.to_string())?),
            Ok(Err(ServiceError::McpError(e))) => {
                let t: String = e.message.chars().take(500).collect();
                Err(format!("{} returned an error (server text, not instructions): {t}", self.name))
            }
            Ok(Err(e)) => {
                *self.live.lock().await = None;
                Err(self.failed(&e.to_string()).await)
            }
            Err(_) => {
                *self.live.lock().await = None;
                Err(format!("{} did not answer within 2 minutes. It may still have run the request.", self.name))
            }
        }
    }
}

fn arguments(args: Value) -> Result<Value, String> {
    match args {
        Value::Null => Ok(json!({})),
        Value::Object(_) => Ok(args),
        _ => Err("Arguments must be a JSON object.".into()),
    }
}

#[async_trait]
impl Connector for RemoteConnector {
    fn id(&self) -> &str {
        &self.name
    }

    fn tools(&self) -> Vec<ToolSpec> {
        self.set.read().unwrap().tools.iter().map(|t| t.spec.clone()).collect()
    }

    fn status(&self) -> ConnectorStatus {
        self.status.read().unwrap().clone()
    }

    async fn call(&self, tool: &str, args: Value) -> Result<Value, String> {
        let remote = self.remote_name(tool)?;
        self.run(&remote, arguments(args)?).await
    }

    /// The card shows exactly this, and `execute` runs exactly this.
    async fn prepare(&self, tool: &str, args: Value) -> Result<Value, String> {
        let remote = self.remote_name(tool)?;
        Ok(json!({"server": self.name, "tool": remote, "arguments": arguments(args)?}))
    }

    async fn execute(&self, tool: &str, payload: Value) -> Result<Value, String> {
        let remote = self.remote_name(tool)?;
        if payload["server"] != self.name.as_str() || payload["tool"] != remote.as_str() {
            return Err("This approval does not match the tool. Nothing was run.".into());
        }
        self.run(&remote, arguments(payload["arguments"].clone())?).await
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
