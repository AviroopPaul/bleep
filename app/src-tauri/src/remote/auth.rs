//! OAuth sign-in for remote MCP servers, through rmcp's authorization support. The order
//! is the MCP spec's, as Claude Code does it: the 401 challenge's resource metadata (RFC 9728),
//! then the authorization server metadata (RFC 8414, OIDC fallback), then a client (one I
//! typed, else dynamic registration, RFC 7591), then PKCE in my browser with a loopback reply.
use crate::google::loopback;
use crate::opener::Opener;
use crate::remote::secrets::{KeychainCredentials, Secrets, ServerSecrets};
use rmcp::transport::auth::{AuthorizationManager, AuthorizationRequest, AuthorizationSession, OAuthClientConfig};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

pub const SIGN_IN_WAIT: Duration = Duration::from_secs(5 * 60);
const REVOKE_TIMEOUT: Duration = Duration::from_secs(10);
pub const CALLBACK: &str = "/callback";

#[derive(Debug, PartialEq)]
pub enum SignIn {
    Done,
    /// The server has no dynamic registration and I have not typed a client ID yet.
    NeedsClientId,
}

async fn manager(url: &str, name: &str, secrets: Arc<dyn Secrets>) -> Result<AuthorizationManager, String> {
    let mut m = AuthorizationManager::new(url).await.map_err(|e| format!("Could not find how {name} signs in. {e}"))?;
    m.set_credential_store(KeychainCredentials::new(secrets, name));
    Ok(m)
}

fn state_of(auth_url: &str) -> Option<String> {
    let u = reqwest::Url::parse(auth_url).ok()?;
    let s = u.query_pairs().find(|(k, _)| k == "state").map(|(_, v)| v.to_string());
    s
}

/// Signs in to one server. Opens the authorization page in my browser and waits for the reply.
pub async fn sign_in(url: &str, name: &str, secrets: Arc<dyn Secrets>, opener: &dyn Opener, challenge: Option<String>, wait: Duration) -> Result<SignIn, String> {
    let mut m = manager(url, name, secrets.clone()).await?;
    let found = m.resolve_metadata_from_challenge(challenge.as_deref()).await.map_err(|e| format!("Could not find how {name} signs in. {e}"))?;
    let has_registration = found.metadata.registration_endpoint.is_some();
    m.set_metadata(found.metadata);
    let client = ServerSecrets::new(secrets.clone(), name).client();
    if client.is_none() && !has_registration {
        return Ok(SignIn::NeedsClientId);
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.map_err(|e| format!("Sign-in failed: no free port on this Mac ({e})."))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!("http://127.0.0.1:{port}{CALLBACK}");
    let mut req = AuthorizationRequest::new(redirect).with_client_name("bleep").with_application_type("native");
    if let Some(ch) = &challenge {
        req = req.with_challenge(ch.clone());
    }
    if let Some(c) = client {
        req = req.with_preregistered_client(c.id);
        if let Some(s) = c.secret {
            req = req.with_client_secret(s);
        }
    }
    let session = AuthorizationSession::new(m, req).await.map_err(|(_, e)| format!("Sign-in failed: {name} did not register bleep. {e}"))?;
    let state = state_of(&session.auth_url).ok_or_else(|| "Sign-in failed: the sign-in link has no state.".to_string())?;
    opener.open(&session.auth_url).map_err(|e| format!("Could not open the browser: {e}"))?;
    let q: HashMap<String, String> = loopback::catch_at(listener, CALLBACK, &state, wait).await.map_err(|e| e.to_string())?;
    let code = q.get("code").cloned().unwrap_or_default();
    session
        .auth_manager
        .exchange_code_for_token_with_issuer(&code, &state, q.get("iss").map(String::as_str))
        .await
        .map_err(|e| format!("Sign-in failed: {name} did not accept the sign-in. {e}"))?;
    Ok(SignIn::Done)
}

/// A manager with my saved tokens, or `None` when I have not signed in.
pub async fn saved_manager(url: &str, name: &str, secrets: Arc<dyn Secrets>) -> Result<Option<AuthorizationManager>, String> {
    if secrets.get(&format!("mcp:{name}")).is_none() {
        return Ok(None);
    }
    let mut m = manager(url, name, secrets.clone()).await?;
    if !m.initialize_from_store().await.map_err(|e| e.to_string())? {
        return Ok(None);
    }
    if let Some(c) = ServerSecrets::new(secrets, name).client() {
        if let Some(secret) = c.secret {
            let cfg = OAuthClientConfig::new(c.id, format!("http://127.0.0.1{CALLBACK}")).with_client_secret(secret);
            m.configure_client(cfg).map_err(|e| e.to_string())?;
        }
    }
    Ok(Some(m))
}

/// The access token rmcp saved, for probes.
pub fn saved_access_token(secrets: &dyn Secrets, name: &str) -> Option<String> {
    let v: Value = serde_json::from_str(&secrets.get(&format!("mcp:{name}"))?).ok()?;
    v["token_response"]["access_token"].as_str().map(str::to_string)
}

async fn revoke(url: &str, name: &str, secrets: Arc<dyn Secrets>) -> Option<()> {
    let saved: Value = serde_json::from_str(&secrets.get(&format!("mcp:{name}"))?).ok()?;
    let m = manager(url, name, secrets.clone()).await.ok()?;
    let meta = m.resolve_metadata().await.ok()?.metadata;
    let endpoint = meta.additional_fields.get("revocation_endpoint")?.as_str()?.to_string();
    let client_id = saved["client_id"].as_str().unwrap_or("").to_string();
    let http = reqwest::Client::builder().timeout(REVOKE_TIMEOUT).build().ok()?;
    let tr = &saved["token_response"];
    for (field, hint) in [("refresh_token", "refresh_token"), ("access_token", "access_token")] {
        if let Some(t) = tr[field].as_str() {
            let _ = http.post(&endpoint).form(&[("token", t), ("token_type_hint", hint), ("client_id", client_id.as_str())]).send().await;
        }
    }
    Some(())
}

/// Revokes the tokens where the server allows it (best effort), then clears every secret.
pub async fn forget(url: &str, name: &str, secrets: Arc<dyn Secrets>) {
    let _ = tokio::time::timeout(REVOKE_TIMEOUT, revoke(url, name, secrets.clone())).await;
    let _ = ServerSecrets::new(secrets, name).clear_all();
}

#[cfg(test)]
#[path = "auth_tests.rs"]
mod tests;
