//! Test-only MCP server: plain JSON-RPC over HTTP, stateless, with an optional bearer check.
//! Next to it, an OAuth authorization server: metadata, dynamic registration, authorize
//! (redirects at once, as if I clicked Allow), token with PKCE check, refresh and revoke.
use axum::body::Bytes;
use axum::extract::{Form, Query, State};
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
    pub oauth: OAuthMock,
}

/// Knobs and records of the mock authorization server.
pub struct OAuthMock {
    /// Offer a registration endpoint.
    pub dcr: bool,
    /// Answer /authorize with a state that does not match.
    pub wrong_state: bool,
    /// Never redirect back, as if I closed the tab.
    pub no_redirect: bool,
    /// Serve metadata that is not JSON.
    pub broken_metadata: bool,
    /// Keep a challenge that no verifier matches.
    pub tamper_challenge: bool,
    pub expires_in: u64,
    pub refuse_refresh: bool,
    pub registrations: Vec<Value>,
    pub authorize_seen: Vec<HashMap<String, String>>,
    /// grant_type of every /token call.
    pub grants: Vec<String>,
    pub revoked: Vec<String>,
    pending: HashMap<String, String>,
    n: u32,
}

impl Default for OAuthMock {
    fn default() -> Self {
        OAuthMock {
            dcr: true,
            wrong_state: false,
            no_redirect: false,
            broken_metadata: false,
            tamper_challenge: false,
            expires_in: 3600,
            refuse_refresh: false,
            registrations: vec![],
            authorize_seen: vec![],
            grants: vec![],
            revoked: vec![],
            pending: HashMap::new(),
            n: 0,
        }
    }
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

    /// `/mcp` answers 401 until a token from `/token` is sent.
    pub fn require_oauth(&self) {
        self.state.lock().unwrap().token = Some("no-token-yet".into());
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
        .route("/.well-known/oauth-protected-resource/mcp", axum::routing::get(prm))
        .route("/.well-known/oauth-protected-resource", axum::routing::get(prm))
        .route("/.well-known/oauth-authorization-server", axum::routing::get(asm))
        .route("/register", axum::routing::post(register))
        .route("/authorize", axum::routing::get(authorize))
        .route("/token", axum::routing::post(token))
        .route("/revoke", axum::routing::post(revoke))
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

async fn prm(State(ctx): State<Ctx>) -> Response {
    axum::Json(json!({"resource": format!("{}/mcp", ctx.base), "authorization_servers": [ctx.base], "bearer_methods_supported": ["header"]})).into_response()
}

async fn asm(State(ctx): State<Ctx>) -> Response {
    let st = ctx.state.lock().unwrap();
    if st.oauth.broken_metadata {
        return (StatusCode::OK, [("content-type", "application/json")], "{not json").into_response();
    }
    let b = &ctx.base;
    let mut m = json!({
        "issuer": b,
        "authorization_endpoint": format!("{b}/authorize"),
        "token_endpoint": format!("{b}/token"),
        "revocation_endpoint": format!("{b}/revoke"),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none"],
    });
    if st.oauth.dcr {
        m["registration_endpoint"] = json!(format!("{b}/register"));
    }
    axum::Json(m).into_response()
}

async fn register(State(ctx): State<Ctx>, body: Bytes) -> Response {
    let req: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let mut st = ctx.state.lock().unwrap();
    st.oauth.registrations.push(req.clone());
    let id = format!("dyn-{}", st.oauth.registrations.len());
    (StatusCode::CREATED, axum::Json(json!({"client_id": id, "redirect_uris": req["redirect_uris"], "token_endpoint_auth_method": "none", "client_name": req["client_name"]}))).into_response()
}

async fn authorize(State(ctx): State<Ctx>, Query(q): Query<HashMap<String, String>>) -> Response {
    let mut st = ctx.state.lock().unwrap();
    st.oauth.authorize_seen.push(q.clone());
    if st.oauth.no_redirect {
        return "Sign in here".into_response();
    }
    st.oauth.n += 1;
    let code = format!("code-{}", st.oauth.n);
    let challenge = if st.oauth.tamper_challenge { "nope".to_string() } else { q.get("code_challenge").cloned().unwrap_or_default() };
    st.oauth.pending.insert(code.clone(), challenge);
    let state = if st.oauth.wrong_state { "bogus".to_string() } else { q.get("state").cloned().unwrap_or_default() };
    let redirect = q.get("redirect_uri").cloned().unwrap_or_default();
    let to = reqwest::Url::parse_with_params(&redirect, &[("code", code.as_str()), ("state", state.as_str())]).unwrap();
    axum::response::Redirect::to(to.as_str()).into_response()
}

fn pkce(verifier: &str) -> String {
    use base64::Engine;
    use sha2::Digest;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier.as_bytes()))
}

fn issue(st: &mut MockState) -> Response {
    st.oauth.n += 1;
    let access = format!("at-{}", st.oauth.n);
    st.token = Some(access.clone());
    axum::Json(json!({"access_token": access, "token_type": "Bearer", "expires_in": st.oauth.expires_in, "refresh_token": format!("rt-{}", st.oauth.n), "scope": "read write"})).into_response()
}

fn token_error(e: &str) -> Response {
    (StatusCode::BAD_REQUEST, axum::Json(json!({"error": e}))).into_response()
}

async fn token(State(ctx): State<Ctx>, Form(f): Form<HashMap<String, String>>) -> Response {
    let mut st = ctx.state.lock().unwrap();
    let grant = f.get("grant_type").cloned().unwrap_or_default();
    st.oauth.grants.push(grant.clone());
    match grant.as_str() {
        "authorization_code" => {
            let code = f.get("code").cloned().unwrap_or_default();
            let Some(challenge) = st.oauth.pending.remove(&code) else { return token_error("invalid_grant") };
            if pkce(f.get("code_verifier").map(String::as_str).unwrap_or("")) != challenge {
                return token_error("invalid_grant");
            }
            issue(&mut st)
        }
        "refresh_token" if st.oauth.refuse_refresh => token_error("invalid_grant"),
        "refresh_token" => issue(&mut st),
        _ => token_error("unsupported_grant_type"),
    }
}

async fn revoke(State(ctx): State<Ctx>, Form(f): Form<HashMap<String, String>>) -> Response {
    ctx.state.lock().unwrap().oauth.revoked.push(f.get("token").cloned().unwrap_or_default());
    StatusCode::OK.into_response()
}
