use super::*;
use crate::config::{McpAuth, McpServer};
use crate::connectors::{Connector, ConnectorStatus};
use crate::google::secrets::MemorySecrets;
use crate::opener::RecordingOpener;
use crate::remote::client::{probe, Probe, RemoteConnector};
use crate::remote::mock::MockMcp;
use crate::remote::secrets::{Client, ServerSecrets};
use crate::store::Store;
use serde_json::json;
use std::time::Duration;

struct Rig {
    mock: MockMcp,
    secrets: Arc<dyn Secrets>,
    opener: Arc<RecordingOpener>,
    store: Arc<Store>,
    _d: tempfile::TempDir,
}

async fn rig() -> Rig {
    let mock = MockMcp::start().await;
    mock.set_tools(vec![json!({"name": "list_tasks", "inputSchema": {"type": "object"}, "annotations": {"readOnlyHint": true}})]);
    mock.require_oauth();
    let d = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(&d.path().join("b.db")).unwrap());
    Rig { mock, secrets: Arc::new(MemorySecrets::default()), opener: Arc::new(RecordingOpener::default()), store, _d: d }
}

async fn challenge(r: &Rig) -> Option<String> {
    match probe(&r.mock.url()).await {
        Probe::AuthRequired { challenge } => challenge,
        p => panic!("expected a 401, got {p:?}"),
    }
}

/// Runs sign-in and plays the browser: waits for the URL, then follows it.
async fn sign_in_with_browser(r: &Rig, wait: Duration) -> Result<SignIn, String> {
    let (url, secrets, opener, ch) = (r.mock.url(), r.secrets.clone(), r.opener.clone(), challenge(r).await);
    let task = tokio::spawn(async move { sign_in(&url, "dayblocks", secrets, opener.as_ref(), ch, wait).await });
    for _ in 0..200 {
        let first = r.opener.urls.lock().unwrap().first().cloned();
        if let Some(u) = first {
            let _ = reqwest::get(&u).await;
            break;
        }
        if task.is_finished() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    task.await.unwrap()
}

fn oauth_conn(r: &Rig) -> Arc<RemoteConnector> {
    let s = McpServer { name: "dayblocks".into(), url: r.mock.url(), enabled: true, auth: McpAuth::Oauth, tools: Default::default() };
    RemoteConnector::new(s, r.store.clone(), r.secrets.clone())
}

#[tokio::test]
async fn full_sign_in_then_the_bearer_is_sent() {
    let r = rig().await;
    assert_eq!(sign_in_with_browser(&r, Duration::from_secs(5)).await.unwrap(), SignIn::Done);
    {
        let st = r.mock.state.lock().unwrap();
        let reg = &st.oauth.registrations[0];
        assert_eq!(reg["client_name"], "bleep");
        let redirect = reg["redirect_uris"][0].as_str().unwrap();
        assert!(redirect.starts_with("http://127.0.0.1:") && redirect.ends_with("/callback"), "{redirect}");
        let q = &st.oauth.authorize_seen[0];
        assert_eq!(q.get("code_challenge_method").map(String::as_str), Some("S256"));
        assert_eq!(q.get("client_id").map(String::as_str), Some("dyn-1"));
        assert_eq!(st.oauth.grants, ["authorization_code"]);
    }
    assert!(r.secrets.get("mcp:dayblocks").is_some(), "tokens are in the Keychain");
    let c = oauth_conn(&r);
    c.connect().await.unwrap();
    let issued = r.mock.state.lock().unwrap().token.clone().unwrap();
    assert!(r.mock.state.lock().unwrap().auth_seen.contains(&Some(format!("Bearer {issued}"))));
    assert!(c.call("dayblocks_list_tasks", json!({})).await.is_ok());
}

#[tokio::test]
async fn oauth_server_without_sign_in_needs_sign_in() {
    let r = rig().await;
    let c = oauth_conn(&r);
    assert_eq!(c.connect().await.unwrap_err(), "dayblocks needs sign-in. Ask the user to sign in again in Settings > Connectors.");
    assert_eq!(c.status(), ConnectorStatus::NeedsSignIn);
}

#[tokio::test]
async fn no_registration_endpoint_asks_for_a_client_id() {
    let r = rig().await;
    r.mock.state.lock().unwrap().oauth.dcr = false;
    assert_eq!(sign_in_with_browser(&r, Duration::from_secs(5)).await.unwrap(), SignIn::NeedsClientId);
    assert!(r.opener.urls.lock().unwrap().is_empty(), "no browser without a client");
    ServerSecrets::new(r.secrets.clone(), "dayblocks").set_client(&Client { id: "pre-1".into(), secret: None }).unwrap();
    assert_eq!(sign_in_with_browser(&r, Duration::from_secs(5)).await.unwrap(), SignIn::Done);
    assert_eq!(r.mock.state.lock().unwrap().oauth.authorize_seen[0].get("client_id").map(String::as_str), Some("pre-1"));
    oauth_conn(&r).connect().await.unwrap();
}

#[tokio::test]
async fn a_reply_with_the_wrong_state_is_refused() {
    let r = rig().await;
    r.mock.state.lock().unwrap().oauth.wrong_state = true;
    let e = sign_in_with_browser(&r, Duration::from_secs(5)).await.unwrap_err();
    assert!(e.contains("did not match"), "{e}");
    assert!(r.secrets.get("mcp:dayblocks").is_none());
}

#[tokio::test]
async fn no_reply_in_time_is_a_plain_error() {
    let r = rig().await;
    r.mock.state.lock().unwrap().oauth.no_redirect = true;
    let e = sign_in_with_browser(&r, Duration::from_millis(300)).await.unwrap_err();
    assert!(e.starts_with("No answer from the browser"), "{e}");
}

#[tokio::test]
async fn a_wrong_verifier_fails_the_exchange() {
    let r = rig().await;
    r.mock.state.lock().unwrap().oauth.tamper_challenge = true;
    let e = sign_in_with_browser(&r, Duration::from_secs(5)).await.unwrap_err();
    assert!(e.starts_with("Sign-in failed"), "{e}");
    assert!(r.secrets.get("mcp:dayblocks").is_none());
}

#[tokio::test]
async fn broken_metadata_is_a_plain_error() {
    let r = rig().await;
    r.mock.state.lock().unwrap().oauth.broken_metadata = true;
    let e = sign_in_with_browser(&r, Duration::from_secs(5)).await.unwrap_err();
    assert!(e.starts_with("Could not find how dayblocks signs in"), "{e}");
}

#[tokio::test]
async fn an_expired_token_is_refreshed() {
    let r = rig().await;
    r.mock.state.lock().unwrap().oauth.expires_in = 1;
    sign_in_with_browser(&r, Duration::from_secs(5)).await.unwrap();
    let c = oauth_conn(&r);
    c.connect().await.unwrap();
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    c.call("dayblocks_list_tasks", json!({})).await.unwrap();
    assert!(r.mock.state.lock().unwrap().oauth.grants.contains(&"refresh_token".to_string()));
}

#[tokio::test]
async fn a_refused_refresh_means_sign_in_again() {
    let r = rig().await;
    {
        let mut st = r.mock.state.lock().unwrap();
        // rmcp refreshes 30 s before expiry, so a 1 s token is refreshed at once.
        st.oauth.expires_in = 1;
        st.oauth.refuse_refresh = true;
    }
    sign_in_with_browser(&r, Duration::from_secs(5)).await.unwrap();
    let c = oauth_conn(&r);
    let e = c.connect().await.unwrap_err();
    assert_eq!(e, "dayblocks needs sign-in. Ask the user to sign in again in Settings > Connectors.", "even though the old token still works on the server");
    assert_eq!(c.status(), ConnectorStatus::NeedsSignIn);
    assert!(r.mock.state.lock().unwrap().oauth.grants.contains(&"refresh_token".to_string()));
}

#[tokio::test]
async fn forget_revokes_and_clears_every_secret() {
    let r = rig().await;
    sign_in_with_browser(&r, Duration::from_secs(5)).await.unwrap();
    ServerSecrets::new(r.secrets.clone(), "dayblocks").set_token("pasted").unwrap();
    forget(&r.mock.url(), "dayblocks", r.secrets.clone()).await;
    assert!(r.mock.state.lock().unwrap().oauth.revoked.iter().any(|t| t.starts_with("rt-")), "the refresh token is revoked");
    assert!(r.secrets.get("mcp:dayblocks").is_none() && ServerSecrets::new(r.secrets.clone(), "dayblocks").token().is_none());
}

#[tokio::test]
async fn forget_without_sign_in_still_clears() {
    let r = rig().await;
    ServerSecrets::new(r.secrets.clone(), "dayblocks").set_token("pasted").unwrap();
    forget("http://127.0.0.1:1/mcp", "dayblocks", r.secrets.clone()).await;
    assert!(ServerSecrets::new(r.secrets.clone(), "dayblocks").token().is_none());
}

#[tokio::test]
async fn a_sign_in_link_that_is_not_https_is_never_opened() {
    let r = rig().await;
    r.mock.state.lock().unwrap().oauth.bad_authorize = true;
    let e = sign_in_with_browser(&r, Duration::from_secs(2)).await.unwrap_err();
    assert!(e.contains("not an https link"), "{e}");
    assert!(r.opener.urls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_rejected_token_without_expiry_is_refreshed_once() {
    let r = rig().await;
    r.mock.state.lock().unwrap().oauth.omit_expiry = true;
    sign_in_with_browser(&r, Duration::from_secs(5)).await.unwrap();
    let c = oauth_conn(&r);
    c.connect().await.unwrap();
    r.mock.state.lock().unwrap().token = Some("rotated".into());
    c.call("dayblocks_list_tasks", json!({})).await.unwrap();
    assert!(r.mock.state.lock().unwrap().oauth.grants.contains(&"refresh_token".to_string()));
    assert_eq!(c.status(), ConnectorStatus::Connected { account: "127.0.0.1".into() });
}
