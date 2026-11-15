//! Test-only MCP server: plain JSON-RPC over HTTP, stateless, with an optional bearer check.
//! Task 4 adds an OAuth authorization server next to it.
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct MockState {
    pub tools: Vec<Value>,
    /// When set, `/mcp` needs `Authorization: Bearer <token>`.
    pub token: Option<String>,
    /// Answers 503 to everything.
    pub down: bool,
    /// Results by tool name. Others get "ran <name>".
    pub results: HashMap<String, Value>,
    /// (tool, arguments) of every tools/call.
    pub calls: Vec<(String, Value)>,
    /// The Authorization header of every request.
    pub auth_seen: Vec<Option<String>>,
}

pub type Shared = Arc<Mutex<MockState>>;

pub struct MockMcp {
    pub base: String,
    pub state: Shared,
}

impl MockMcp {
    pub async fn start() -> MockMcp {
        let state: Shared = Arc::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let app = router(state.clone(), base.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        MockMcp { base, state }
    }

    pub fn url(&self) -> String {
        format!("{}/mcp", self.base)
    }

    pub fn set_tools(&self, tools: Vec<Value>) {
        self.state.lock().unwrap().tools = tools;
    }
}

#[derive(Clone)]
struct Ctx {
    state: Shared,
    base: String,
}

fn router(state: Shared, base: String) -> axum::Router {
    axum::Router::new()
        .route("/mcp", axum::routing::post(mcp).get(|| async { StatusCode::METHOD_NOT_ALLOWED }).delete(|| async { StatusCode::OK }))
        .with_state(Ctx { state, base })
}

fn rpc_result(id: &Value, result: Value) -> Response {
    axum::Json(json!({"jsonrpc": "2.0", "id": id, "result": result})).into_response()
}

async fn mcp(State(ctx): State<Ctx>, headers: HeaderMap, body: Bytes) -> Response {
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).map(str::to_string);
    let mut st = ctx.state.lock().unwrap();
    st.auth_seen.push(auth.clone());
    if st.down {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    if let Some(t) = &st.token {
        if auth.as_deref() != Some(&format!("Bearer {t}")) {
            let challenge = format!("Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource/mcp\"", ctx.base);
            return (StatusCode::UNAUTHORIZED, [("www-authenticate", challenge)]).into_response();
        }
    }
    let Ok(req) = serde_json::from_slice::<Value>(&body) else { return StatusCode::BAD_REQUEST.into_response() };
    let Some(id) = req.get("id").cloned() else { return StatusCode::ACCEPTED.into_response() };
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    match req["method"].as_str().unwrap_or("") {
        "initialize" => rpc_result(
            &id,
            json!({"protocolVersion": params["protocolVersion"].as_str().unwrap_or("2025-06-18"), "capabilities": {"tools": {}}, "serverInfo": {"name": "mock", "version": "1"}}),
        ),
        "ping" => rpc_result(&id, json!({})),
        "tools/list" => rpc_result(&id, json!({"tools": st.tools})),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or("").to_string();
            st.calls.push((name.clone(), params.get("arguments").cloned().unwrap_or(json!({}))));
            let r = st.results.get(&name).cloned().unwrap_or_else(|| json!({"content": [{"type": "text", "text": format!("ran {name}")}]}));
            rpc_result(&id, r)
        }
        _ => axum::Json(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "no such method"}})).into_response(),
    }
}
