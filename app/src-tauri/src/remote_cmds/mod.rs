//! Tauri commands for Settings > Connectors. The work is in `remote::manager::Remotes`.
use crate::config::{McpServer, ToolMode};
use crate::connectors::ConnectorStatus;
use crate::google::GoogleAuth;
use crate::remote::manager::{AddRequest, Outcome, ServerView};
use crate::AppState;
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Debug, PartialEq)]
pub struct BuiltinView {
    pub id: &'static str,
    pub label: &'static str,
    pub status: ConnectorStatus,
}

#[derive(Serialize, Debug)]
pub struct ConnectorsView {
    pub builtin: Vec<BuiltinView>,
    pub servers: Vec<ServerView>,
}

/// bleep's own connectors, in the order Settings lists them. Gmail and Calendar share Google's sign-in.
pub fn builtins(google: ConnectorStatus) -> Vec<BuiltinView> {
    let here = || ConnectorStatus::Connected { account: "this Mac".into() };
    vec![
        BuiltinView { id: "gmail", label: "Gmail", status: google.clone() },
        BuiltinView { id: "calendar", label: "Google Calendar", status: google },
        BuiltinView { id: "memory", label: "Memory", status: here() },
        BuiltinView { id: "bleep", label: "Date and time", status: here() },
    ]
}

/// Replaces or adds `cfg` by name, or removes `name` when `cfg` is `None`.
pub fn upsert(servers: &mut Vec<McpServer>, name: &str, cfg: Option<McpServer>) {
    match (servers.iter().position(|s| s.name == name), cfg) {
        (Some(i), Some(c)) => servers[i] = c,
        (None, Some(c)) => servers.push(c),
        (Some(i), None) => {
            servers.remove(i);
        }
        (None, None) => {}
    }
}

fn save(app: &AppHandle, state: &AppState, name: &str, cfg: Option<McpServer>) -> Result<(), String> {
    {
        let mut c = state.config.lock().unwrap();
        upsert(&mut c.mcp.servers, name, cfg);
        c.save(&state.paths.config_file).map_err(|e| e.to_string())?;
    }
    changed(app);
    Ok(())
}

fn changed(app: &AppHandle) {
    let _ = app.emit("connectors://changed", ());
}

#[tauri::command]
pub fn connectors_list(state: tauri::State<'_, AppState>, auth: tauri::State<'_, Arc<GoogleAuth>>) -> ConnectorsView {
    ConnectorsView { builtin: builtins(auth.status()), servers: state.remotes.views() }
}

#[tauri::command]
pub async fn remote_add(app: AppHandle, state: tauri::State<'_, AppState>, req: AddRequest) -> Result<Outcome, String> {
    let taken: Vec<String> = state.config.lock().unwrap().mcp.servers.iter().map(|s| s.name.clone()).collect();
    let taken: Vec<&str> = taken.iter().map(String::as_str).collect();
    let remotes = state.remotes.clone();
    let out = remotes.add(req, &taken).await?;
    if let Outcome::Added(cfg) = &out {
        save(&app, &state, &cfg.name.clone(), Some(cfg.clone()))?;
    }
    Ok(out)
}

#[tauri::command]
pub async fn remote_sign_in(app: AppHandle, state: tauri::State<'_, AppState>, name: String, token: Option<String>) -> Result<(), String> {
    let remotes = state.remotes.clone();
    let res = remotes.sign_in_again(&name, token).await;
    changed(&app);
    res
}

#[tauri::command]
pub fn remote_cancel_sign_in(state: tauri::State<'_, AppState>) -> bool {
    state.remotes.cancel_sign_in()
}

#[tauri::command]
pub fn remote_set_tool(app: AppHandle, state: tauri::State<'_, AppState>, name: String, tool: String, mode: Option<ToolMode>) -> Result<(), String> {
    let cfg = state.remotes.set_mode(&name, &tool, mode)?;
    save(&app, &state, &name, Some(cfg))
}

#[tauri::command]
pub async fn remote_remove(app: AppHandle, state: tauri::State<'_, AppState>, name: String) -> Result<(), String> {
    let remotes = state.remotes.clone();
    remotes.remove(&name).await?;
    save(&app, &state, &name, None)
}

#[tauri::command]
pub async fn remote_refresh(app: AppHandle, state: tauri::State<'_, AppState>, name: String) -> Result<(), String> {
    let remotes = state.remotes.clone();
    let res = remotes.refresh(&name).await;
    changed(&app);
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::McpAuth;

    fn s(name: &str) -> McpServer {
        McpServer { name: name.into(), url: format!("https://{name}.io/mcp"), enabled: true, auth: McpAuth::None, tools: Default::default() }
    }

    #[test]
    fn builtins_follow_google() {
        let b = builtins(ConnectorStatus::NeedsSignIn);
        let ids: Vec<_> = b.iter().map(|v| v.id).collect();
        assert_eq!(ids, ["gmail", "calendar", "memory", "bleep"]);
        assert_eq!((b[0].status.clone(), b[1].status.clone()), (ConnectorStatus::NeedsSignIn, ConnectorStatus::NeedsSignIn));
        assert_eq!(b[2].status, ConnectorStatus::Connected { account: "this Mac".into() });
    }

    #[test]
    fn upsert_adds_replaces_and_removes() {
        let mut v = vec![s("a")];
        upsert(&mut v, "b", Some(s("b")));
        let mut b2 = s("b");
        b2.enabled = false;
        upsert(&mut v, "b", Some(b2));
        assert_eq!((v.len(), v[1].enabled), (2, false));
        upsert(&mut v, "a", None);
        upsert(&mut v, "zzz", None);
        assert_eq!(v.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["b"]);
    }
}
