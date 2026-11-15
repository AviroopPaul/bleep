use super::*;
use crate::config::{McpAuth, McpServer, ToolMode};
use crate::connectors::{Connector, ConnectorStatus, Risk};
use crate::google::secrets::MemorySecrets;
use crate::remote::mock::MockMcp;
use serde_json::json;

struct Rig {
    mock: MockMcp,
    store: Arc<Store>,
    secrets: Arc<dyn Secrets>,
    _d: tempfile::TempDir,
}

async fn rig() -> Rig {
    let mock = MockMcp::start().await;
    mock.set_tools(vec![
        json!({"name": "list_tasks", "description": "Open tasks.", "inputSchema": {"type": "object"}, "annotations": {"readOnlyHint": true}}),
        json!({"name": "move_task", "description": "Move a task.", "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}, "day": {"type": "string"}}}}),
    ]);
    let d = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(&d.path().join("b.db")).unwrap());
    Rig { mock, store, secrets: Arc::new(MemorySecrets::default()), _d: d }
}

fn server(url: &str, auth: McpAuth) -> McpServer {
    McpServer { name: "dayblocks".into(), url: url.into(), enabled: true, auth, tools: Default::default() }
}

fn conn(r: &Rig, auth: McpAuth) -> Arc<RemoteConnector> {
    RemoteConnector::new(server(&r.mock.url(), auth), r.store.clone(), r.secrets.clone())
}

fn names(c: &RemoteConnector) -> Vec<(String, Risk)> {
    c.tools().into_iter().map(|t| (t.name.to_string(), t.risk)).collect()
}

#[tokio::test]
async fn connect_lists_and_caches_tools() {
    let r = rig().await;
    let c = conn(&r, McpAuth::None);
    assert!(c.tools().is_empty());
    c.connect().await.unwrap();
    assert_eq!(names(&c), [("dayblocks_list_tasks".into(), Risk::Read), ("dayblocks_move_task".into(), Risk::Write)]);
    assert_eq!(c.status(), ConnectorStatus::Connected { account: "127.0.0.1".into() });
    assert_eq!(r.store.mcp_tools("dayblocks").unwrap().len(), 2);
    assert_eq!(c.id(), "dayblocks");
}

#[tokio::test]
async fn cached_tools_show_before_connect_after_a_restart() {
    let r = rig().await;
    conn(&r, McpAuth::None).connect().await.unwrap();
    r.mock.state.lock().unwrap().down = true;
    let again = conn(&r, McpAuth::None);
    assert_eq!(names(&again).len(), 2, "the model sees the same tools after a restart");
    let e = again.call("dayblocks_list_tasks", json!({})).await.unwrap_err();
    assert!(e.starts_with("Could not reach dayblocks"), "{e}");
    assert!(matches!(again.status(), ConnectorStatus::Error { .. }));
}

#[tokio::test]
async fn read_call_returns_untrusted_data() {
    let r = rig().await;
    let c = conn(&r, McpAuth::None);
    c.connect().await.unwrap();
    let out = c.call("dayblocks_list_tasks", json!({"day": "2026-11-15"})).await.unwrap();
    assert_eq!(out, json!({"server": "dayblocks", "content_untrusted": "ran list_tasks"}));
    assert_eq!(r.mock.state.lock().unwrap().calls, [("list_tasks".to_string(), json!({"day": "2026-11-15"}))]);
}

#[tokio::test]
async fn call_connects_on_first_use() {
    let r = rig().await;
    conn(&r, McpAuth::None).connect().await.unwrap();
    let c = conn(&r, McpAuth::None);
    assert!(c.call("dayblocks_list_tasks", json!({})).await.is_ok());
}

#[tokio::test]
async fn write_runs_exactly_the_prepared_payload() {
    let r = rig().await;
    let c = conn(&r, McpAuth::None);
    c.connect().await.unwrap();
    let p = c.prepare("dayblocks_move_task", json!({"id": "t1", "day": "2026-11-16"})).await.unwrap();
    assert_eq!(p, json!({"server": "dayblocks", "tool": "move_task", "arguments": {"id": "t1", "day": "2026-11-16"}}));
    c.execute("dayblocks_move_task", p).await.unwrap();
    assert_eq!(r.mock.state.lock().unwrap().calls, [("move_task".to_string(), json!({"id": "t1", "day": "2026-11-16"}))]);
    let bad = json!({"server": "dayblocks", "tool": "delete_all", "arguments": {}});
    assert!(c.execute("dayblocks_move_task", bad).await.is_err(), "a payload for another tool is refused");
    assert!(c.prepare("dayblocks_move_task", json!(["not", "an", "object"])).await.is_err());
    assert_eq!(c.prepare("dayblocks_move_task", Value::Null).await.unwrap()["arguments"], json!({}));
}

#[tokio::test]
async fn unknown_and_hidden_tools_are_refused() {
    let r = rig().await;
    let mut s = server(&r.mock.url(), McpAuth::None);
    s.tools.insert("move_task".into(), ToolMode::Off);
    let c = RemoteConnector::new(s, r.store.clone(), r.secrets.clone());
    c.connect().await.unwrap();
    assert_eq!(names(&c).len(), 1);
    assert_eq!(c.call("dayblocks_move_task", json!({})).await.unwrap_err(), "Unknown tool: dayblocks_move_task");
    assert!(r.mock.state.lock().unwrap().calls.is_empty());
}

#[tokio::test]
async fn set_mode_remaps_from_the_cache() {
    let r = rig().await;
    let c = conn(&r, McpAuth::None);
    c.connect().await.unwrap();
    c.set_mode("move_task", Some(ToolMode::Auto));
    assert_eq!(names(&c)[1], ("dayblocks_move_task".into(), Risk::Read));
    c.set_mode("list_tasks", Some(ToolMode::Off));
    assert_eq!(names(&c).len(), 1);
    c.set_mode("list_tasks", None);
    assert_eq!(names(&c).len(), 2);
    assert_eq!(c.config().tools.get("move_task"), Some(&ToolMode::Auto));
}

#[tokio::test]
async fn token_auth_sends_the_pasted_token() {
    let r = rig().await;
    r.mock.state.lock().unwrap().token = Some("tok".into());
    let c = conn(&r, McpAuth::Token);
    c.connect().await.unwrap_err();
    assert_eq!(c.status(), ConnectorStatus::NeedsSignIn, "a 401 means sign-in, not a broken server");
    crate::remote::secrets::ServerSecrets::new(r.secrets.clone(), "dayblocks").set_token("tok").unwrap();
    c.connect().await.unwrap();
    assert!(r.mock.state.lock().unwrap().auth_seen.contains(&Some("Bearer tok".into())));
    let e = conn(&r, McpAuth::Token);
    r.mock.state.lock().unwrap().token = Some("rotated".into());
    let err = e.call("dayblocks_list_tasks", json!({})).await.unwrap_err();
    assert_eq!(err, "dayblocks needs sign-in. Ask the user to sign in again in Settings > Connectors.");
}

#[tokio::test]
async fn probe_tells_open_from_auth_from_down() {
    let r = rig().await;
    assert_eq!(probe(&r.mock.url()).await, Probe::Open);
    r.mock.state.lock().unwrap().token = Some("t".into());
    match probe(&r.mock.url()).await {
        Probe::AuthRequired { challenge } => assert!(challenge.unwrap().contains("resource_metadata=")),
        p => panic!("{p:?}"),
    }
    r.mock.state.lock().unwrap().down = true;
    assert!(matches!(probe(&r.mock.url()).await, Probe::Unreachable(_)));
    assert!(matches!(probe("http://127.0.0.1:1/mcp").await, Probe::Unreachable(_)));
}

#[tokio::test]
async fn reconnects_once_after_the_server_restarts() {
    let r = rig().await;
    let c = conn(&r, McpAuth::None);
    c.connect().await.unwrap();
    r.mock.state.lock().unwrap().down = true;
    assert!(c.call("dayblocks_list_tasks", json!({})).await.is_err());
    r.mock.state.lock().unwrap().down = false;
    assert!(c.call("dayblocks_list_tasks", json!({})).await.is_ok(), "the next call connects again");
}
