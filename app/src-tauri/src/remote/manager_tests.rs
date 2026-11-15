use super::*;
use crate::config::{McpAuth, ToolMode};
use crate::google::secrets::MemorySecrets;
use crate::mcp::host::tests::FakeGate;
use crate::mcp::host::{Decision, ToolHost, TurnTracker};
use crate::opener::RecordingOpener;
use crate::remote::mock::MockMcp;
use serde_json::json;
use std::time::Duration;

struct Rig {
    mock: MockMcp,
    remotes: Arc<Remotes>,
    host: Arc<ToolHost>,
    secrets: Arc<dyn Secrets>,
    opener: Arc<RecordingOpener>,
    _d: tempfile::TempDir,
}

async fn rig() -> Rig {
    let mock = MockMcp::start().await;
    mock.set_tools(vec![
        json!({"name": "list_tasks", "description": "Open tasks.", "inputSchema": {"type": "object"}, "annotations": {"readOnlyHint": true}}),
        json!({"name": "move_task", "description": "Move a task.", "inputSchema": {"type": "object"}}),
    ]);
    let d = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(&d.path().join("b.db")).unwrap());
    let host = ToolHost::new(vec![Arc::new(crate::connectors::bleep::BleepConnector)], FakeGate::new(Decision::Approved), TurnTracker::default());
    let secrets: Arc<dyn Secrets> = Arc::new(MemorySecrets::default());
    let opener = Arc::new(RecordingOpener::default());
    let remotes = Remotes::new(host.clone(), store, secrets.clone(), opener.clone(), Some(4100)).with_sign_in_wait(Duration::from_secs(5));
    Rig { mock, remotes, host, secrets, opener, _d: d }
}

fn req(url: &str) -> AddRequest {
    AddRequest { url: url.into(), name: None, token: None, client_id: None, client_secret: None }
}

/// Plays the browser for one sign-in: follows the first URL the opener gets.
fn browser(r: &Rig) {
    let opener = r.opener.clone();
    tokio::spawn(async move {
        for _ in 0..300 {
            let u = opener.urls.lock().unwrap().first().cloned();
            if let Some(u) = u {
                let _ = reqwest::get(&u).await;
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
}

fn tool_names(h: &ToolHost) -> Vec<String> {
    h.specs().iter().map(|t| t.name.to_string()).collect()
}

#[tokio::test]
async fn add_an_open_server() {
    let r = rig().await;
    let out = r.remotes.add(req(&r.mock.url()), &[]).await.unwrap();
    let Outcome::Added(cfg) = out else { panic!("{out:?}") };
    assert_eq!((cfg.name.as_str(), cfg.auth), ("local", McpAuth::None));
    assert!(tool_names(&r.host).contains(&"local_move_task".to_string()));
    let v = r.remotes.views();
    assert_eq!(v[0].status, ConnectorStatus::Connected { account: "127.0.0.1".into() });
    let modes: Vec<_> = v[0].tools.iter().map(|t| (t.name.as_str(), t.mode, t.default_mode)).collect();
    assert_eq!(modes, [("list_tasks", ToolMode::Auto, ToolMode::Auto), ("move_task", ToolMode::Ask, ToolMode::Ask)]);
}

#[tokio::test]
async fn add_checks_the_url_and_the_name_first() {
    let r = rig().await;
    assert!(r.remotes.add(req("http://example.com/mcp"), &[]).await.unwrap_err().starts_with("Use an https:// address"));
    let mut q = req(&r.mock.url());
    q.name = Some("gmail".into());
    assert!(r.remotes.add(q, &[]).await.unwrap_err().contains("used by bleep"));
    assert!(r.remotes.add(req(&r.mock.url()), &["local"]).await.unwrap_err().contains("already added"));
    assert!(r.remotes.add(req("http://127.0.0.1:4100/mcp"), &[]).await.unwrap_err().contains("bleep's own server"));
    assert!(r.remotes.add(req("http://127.0.0.1:1/mcp"), &[]).await.unwrap_err().starts_with("Could not reach"));
    assert_eq!(tool_names(&r.host), ["bleep_now"]);
}

#[tokio::test]
async fn add_an_oauth_server_signs_in_in_the_browser() {
    let r = rig().await;
    r.mock.require_oauth();
    browser(&r);
    let mut q = req(&r.mock.url());
    q.name = Some("dayblocks".into());
    let Outcome::Added(cfg) = r.remotes.add(q, &[]).await.unwrap() else { panic!() };
    assert_eq!(cfg.auth, McpAuth::Oauth);
    assert!(tool_names(&r.host).contains(&"dayblocks_list_tasks".to_string()));
}

#[tokio::test]
async fn a_server_without_registration_asks_for_a_client_id_and_adds_nothing() {
    let r = rig().await;
    r.mock.require_oauth();
    r.mock.state.lock().unwrap().oauth.dcr = false;
    assert!(matches!(r.remotes.add(req(&r.mock.url()), &[]).await.unwrap(), Outcome::NeedsClientId));
    assert!(r.remotes.views().is_empty());
    browser(&r);
    let mut q = req(&r.mock.url());
    q.client_id = Some("pre-1".into());
    assert!(matches!(r.remotes.add(q, &[]).await.unwrap(), Outcome::Added(_)));
}

#[tokio::test]
async fn a_server_with_no_oauth_metadata_asks_for_a_token() {
    let r = rig().await;
    r.mock.state.lock().unwrap().token = Some("tok".into());
    r.mock.state.lock().unwrap().oauth.broken_metadata = true;
    let out = r.remotes.add(req(&r.mock.url()), &[]).await.unwrap();
    assert!(matches!(&out, Outcome::NeedsToken { .. }), "{out:?}");
    assert!(r.secrets.get("mcp:local:token").is_none());
    let mut q = req(&r.mock.url());
    q.token = Some("tok".into());
    let Outcome::Added(cfg) = r.remotes.add(q, &[]).await.unwrap() else { panic!() };
    assert_eq!(cfg.auth, McpAuth::Token);
    assert_eq!(r.secrets.get("mcp:local:token").as_deref(), Some("tok"));
}

#[tokio::test]
async fn a_wrong_token_is_not_kept() {
    let r = rig().await;
    r.mock.state.lock().unwrap().token = Some("tok".into());
    let mut q = req(&r.mock.url());
    q.token = Some("wrong".into());
    assert!(r.remotes.add(q, &[]).await.unwrap_err().contains("needs sign-in"));
    assert!(r.secrets.get("mcp:local:token").is_none(), "a failed add leaves no secret");
    assert!(r.remotes.views().is_empty());
}

#[tokio::test]
async fn cancel_stops_a_sign_in() {
    let r = rig().await;
    r.mock.require_oauth();
    r.mock.state.lock().unwrap().oauth.no_redirect = true;
    let rem = r.remotes.clone();
    let url = r.mock.url();
    let task = tokio::spawn(async move { rem.add(req(&url), &[]).await });
    for _ in 0..300 {
        if !r.opener.urls.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(r.remotes.cancel_sign_in());
    assert_eq!(task.await.unwrap().unwrap_err(), "Sign-in was cancelled.");
    assert!(r.remotes.views().is_empty());
    assert!(!r.remotes.cancel_sign_in(), "nothing left to cancel");
}

#[tokio::test]
async fn set_tool_mode_changes_risk_and_off_hides() {
    let r = rig().await;
    r.remotes.add(req(&r.mock.url()), &[]).await.unwrap();
    let cfg = r.remotes.set_mode("local", "move_task", Some(ToolMode::Auto)).unwrap();
    assert_eq!(cfg.tools.get("move_task"), Some(&ToolMode::Auto));
    let cfg = r.remotes.set_mode("local", "list_tasks", Some(ToolMode::Off)).unwrap();
    assert_eq!(cfg.tools.get("list_tasks"), Some(&ToolMode::Off));
    assert!(!tool_names(&r.host).contains(&"local_list_tasks".to_string()));
    let off = r.remotes.views()[0].tools.iter().find(|t| t.name == "list_tasks").unwrap().mode;
    assert_eq!(off, ToolMode::Off, "an off tool still shows in Settings");
    assert!(r.remotes.set_mode("nope", "x", None).is_err());
}

#[tokio::test]
async fn remove_takes_everything_away() {
    let r = rig().await;
    r.mock.state.lock().unwrap().token = Some("tok".into());
    let mut q = req(&r.mock.url());
    q.token = Some("tok".into());
    r.remotes.add(q, &[]).await.unwrap();
    r.remotes.remove("local").await.unwrap();
    assert_eq!(tool_names(&r.host), ["bleep_now"]);
    assert!(r.remotes.views().is_empty() && r.secrets.get("mcp:local:token").is_none());
    assert!(r.remotes.remove("local").await.is_err());
}

#[tokio::test]
async fn load_adds_configured_servers_from_the_cache() {
    let r = rig().await;
    let Outcome::Added(cfg) = r.remotes.add(req(&r.mock.url()), &[]).await.unwrap() else { panic!() };
    r.host.remove("local");
    let again = Remotes::new(r.host.clone(), r.remotes.store.clone(), r.secrets.clone(), r.opener.clone(), None);
    r.mock.state.lock().unwrap().down = true;
    let mut off = cfg.clone();
    off.name = "other".into();
    off.enabled = false;
    again.load(&[cfg, off]);
    assert!(tool_names(&r.host).contains(&"local_move_task".to_string()), "cached tools before any connect");
    assert_eq!(again.views().len(), 2);
    assert!(!tool_names(&r.host).iter().any(|t| t.starts_with("other_")), "a disabled server adds no tools");
}

#[tokio::test]
async fn views_never_carry_secrets() {
    let r = rig().await;
    r.mock.state.lock().unwrap().token = Some("s3cret-token".into());
    let mut q = req(&r.mock.url());
    q.token = Some("s3cret-token".into());
    r.remotes.add(q, &[]).await.unwrap();
    let text = serde_json::to_string(&r.remotes.views()).unwrap();
    assert!(!text.contains("s3cret"), "{text}");
}
