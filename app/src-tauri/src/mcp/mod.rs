//! In-app MCP server: streamable HTTP on 127.0.0.1, random port, bearer token (build plan 8.2).
//! Stateless with plain JSON replies: every request stands alone, no SSE.
pub mod tools;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::Response;
use rand::RngCore;
use rmcp::model::{CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::{session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData, RoleServer, ServerHandler};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub fn new_token() -> String {
    let mut b = [0u8; 32];
    rand::rng().fill_bytes(&mut b);
    hex::encode(b)
}

#[derive(Clone)]
struct BleepTools;

impl ServerHandler for BleepTools {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(&self, _r: Option<PaginatedRequestParams>, _c: RequestContext<RoleServer>) -> Result<ListToolsResult, ErrorData> {
        let tools = tools::registry()
            .into_iter()
            .filter(|t| t.risk != tools::Risk::Never)
            .map(|t| Tool::new(t.name, t.description, Arc::new(t.schema.as_object().cloned().unwrap_or_default())))
            .collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(&self, req: CallToolRequestParams, _c: RequestContext<RoleServer>) -> Result<CallToolResponse, ErrorData> {
        let Some(spec) = tools::registry().into_iter().find(|t| t.name == req.name) else {
            return Err(ErrorData::invalid_params(format!("Unknown tool: {}", req.name), None));
        };
        if let Err(why) = tools::gate(&spec) {
            return Ok(CallToolResult::error(vec![ContentBlock::text(why)]).into());
        }
        let args = serde_json::Value::Object(req.arguments.clone().unwrap_or_default());
        Ok(match tools::run(spec.name, &args) {
            Ok(v) => CallToolResult::success(vec![ContentBlock::text(v.to_string())]),
            Err(e) => CallToolResult::error(vec![ContentBlock::text(e)]),
        }
        .into())
    }
}

async fn require_bearer(State(token): State<Arc<String>>, req: Request, next: Next) -> Result<Response, StatusCode> {
    let ok = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .is_some_and(|t| constant_eq(t.as_bytes(), token.as_bytes()));
    if ok { Ok(next.run(req).await) } else { Err(StatusCode::UNAUTHORIZED) }
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Dropping it stops the server.
pub struct McpServer {
    url: String,
    token: String,
    stop: CancellationToken,
}

impl McpServer {
    pub async fn start(token: String) -> anyhow::Result<McpServer> {
        let stop = CancellationToken::new();
        let config = StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .with_sse_keep_alive(None)
            .with_cancellation_token(stop.child_token());
        let service: StreamableHttpService<BleepTools, LocalSessionManager> = StreamableHttpService::new(|| Ok(BleepTools), Default::default(), config);
        let router = axum::Router::new()
            .nest_service("/mcp", service)
            .layer(middleware::from_fn_with_state(Arc::new(token.clone()), require_bearer));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}/mcp", listener.local_addr()?);
        let s = stop.clone();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).with_graceful_shutdown(async move { s.cancelled_owned().await }).await;
        });
        Ok(McpServer { url, token, stop })
    }

    pub fn url(&self) -> String {
        self.url.clone()
    }

    pub fn token(&self) -> &str {
        &self.token
    }
}

impl Drop for McpServer {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    async fn rpc(srv: &McpServer, token: Option<&str>, body: Value) -> reqwest::Response {
        let mut r = reqwest::Client::new()
            .post(srv.url())
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .json(&body);
        if let Some(t) = token {
            r = r.bearer_auth(t);
        }
        r.send().await.unwrap()
    }

    fn call(id: u32, method: &str, params: Value) -> Value {
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
    }

    #[tokio::test]
    async fn rejects_missing_token() {
        let srv = McpServer::start("secret".into()).await.unwrap();
        let r = rpc(&srv, None, call(1, "tools/list", json!({}))).await;
        assert_eq!(r.status(), 401);
        let r = rpc(&srv, Some("wrong"), call(1, "tools/list", json!({}))).await;
        assert_eq!(r.status(), 401);
    }

    #[tokio::test]
    async fn binds_loopback_only() {
        let srv = McpServer::start("secret".into()).await.unwrap();
        assert!(srv.url().starts_with("http://127.0.0.1:"), "{}", srv.url());
        assert!(srv.url().ends_with("/mcp"));
        assert_eq!(srv.token(), "secret");
    }

    #[tokio::test]
    async fn lists_bleep_now() {
        let srv = McpServer::start("secret".into()).await.unwrap();
        let r = rpc(&srv, Some("secret"), call(1, "tools/list", json!({}))).await;
        assert_eq!(r.status(), 200);
        let v: Value = r.json().await.unwrap();
        let names: Vec<&str> = v["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"bleep_now"), "{v}");
    }

    #[tokio::test]
    async fn bleep_now_has_timezone() {
        let srv = McpServer::start("secret".into()).await.unwrap();
        let v: Value = rpc(&srv, Some("secret"), call(2, "tools/call", json!({"name": "bleep_now", "arguments": {}}))).await.json().await.unwrap();
        let text = v["result"]["content"][0]["text"].as_str().unwrap_or_else(|| panic!("{v}"));
        let now: Value = serde_json::from_str(text).unwrap();
        assert!(!now["timezone"].as_str().unwrap().is_empty());
        let off = now["utc_offset"].as_str().unwrap();
        assert!(off.len() == 6 && (off.starts_with('+') || off.starts_with('-')) && &off[3..4] == ":", "{off}");
        for k in ["iso", "date", "time", "weekday"] {
            assert!(now[k].is_string(), "missing {k}");
        }
    }

    #[tokio::test]
    async fn unknown_tool_is_an_error() {
        let srv = McpServer::start("secret".into()).await.unwrap();
        let v: Value = rpc(&srv, Some("secret"), call(3, "tools/call", json!({"name": "gmail_send", "arguments": {}}))).await.json().await.unwrap();
        assert!(v.get("error").is_some() || v["result"]["isError"] == true, "{v}");
    }

    #[test]
    fn new_token_is_64_hex() {
        let t = new_token();
        assert_eq!(t.len(), 64);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(t, new_token());
    }
}
