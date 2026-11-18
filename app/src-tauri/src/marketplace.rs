//! The connector marketplace (0.5.0, F3): every connector we configured, each with an on or
//! off switch and its sign-in. Google ones are in bleep's code and share one Google sign-in,
//! asking only for their own scopes. Remote ones are MCP servers with a URL we set.
//! "Add by URL" (`remote_cmds`) stays for anything not listed here.
use crate::connectors::ConnectorStatus;
use crate::google::{scopes_for, GoogleAuth, GOOGLE_CONNECTORS};
use crate::proactive::scheduler::Scheduler;
use crate::remote::manager::{AddRequest, Outcome};
use crate::connectors::{Connector, Risk, ToolSpec};
use crate::AppState;
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Google,
    Remote,
    /// Runs on this Mac: nothing to sign in to.
    Local,
}

#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub id: &'static str,
    pub name: &'static str,
    pub blurb: &'static str,
    pub kind: Kind,
    /// Remote only: the server URL and how it signs in.
    pub url: Option<&'static str>,
    /// Remote only: the server needs a pasted token (no OAuth bleep can use).
    pub token: bool,
}

const fn google(id: &'static str, name: &'static str, blurb: &'static str) -> Entry {
    Entry { id, name, blurb, kind: Kind::Google, url: None, token: false }
}

const fn local(id: &'static str, name: &'static str, blurb: &'static str) -> Entry {
    Entry { id, name, blurb, kind: Kind::Local, url: None, token: false }
}

const fn remote(id: &'static str, name: &'static str, blurb: &'static str, url: &'static str, token: bool) -> Entry {
    Entry { id, name, blurb, kind: Kind::Remote, url: Some(url), token }
}

/// The catalog, in the order Settings shows it. Remote ids are also their server names.
pub const CATALOG: &[Entry] = &[
    google("gmail", "Gmail", "Sorts your mail, finds bills and drafts replies. Sending always asks you first."),
    google("calendar", "Google Calendar", "Your meetings, the meeting bar, and event changes after you approve."),
    google("drive", "Google Drive", "Finds and reads your Docs, Sheets, Slides and text files. Read only."),
    local("files", "Files on this Mac", "Lists, finds, greps and reads files in Documents, Desktop and Downloads. Commands ask you first."),
    remote("linear", "Linear", "Your issues and projects. Changes ask you first.", "https://mcp.linear.app/mcp", false),
    remote("notion", "Notion", "Your pages and databases. Changes ask you first.", "https://mcp.notion.com/mcp", false),
    remote("github", "GitHub", "Repos, issues and pull requests. Needs a personal access token.", "https://api.githubcopilot.com/mcp/", true),
];

/// Always on, nothing to sign in to.
pub const ALWAYS_ON: &[(&str, &str)] = &[("bleep", "Date and time"), ("memory", "Memory"), ("reminders", "Reminders")];

pub fn entry(id: &str) -> Option<&'static Entry> {
    CATALOG.iter().find(|e| e.id == id)
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Item {
    pub id: &'static str,
    pub name: &'static str,
    pub blurb: &'static str,
    pub kind: Kind,
    pub token: bool,
    pub on: bool,
    /// Sign-in state. For an item that is off, what turning it on would need.
    pub status: ConnectorStatus,
    /// Remote: the server's tool count, once connected.
    pub tools: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct CatalogView {
    pub items: Vec<Item>,
    pub always_on: Vec<&'static str>,
}

/// Built-in connectors that run on this Mac.
pub const LOCAL_CONNECTORS: &[&str] = &["files"];

/// The built-in connectors (Google and local) that are off: the tool host hides them.
pub fn hidden_builtin(on: &[String]) -> Vec<String> {
    GOOGLE_CONNECTORS.iter().chain(LOCAL_CONNECTORS).filter(|id| !on.iter().any(|x| x == *id)).map(|s| s.to_string()).collect()
}

/// Applies the on list to Google sign-in checks and the tool host.
pub fn apply_on(state: &AppState, auth: &GoogleAuth) {
    let on = state.config.lock().unwrap().connectors.on.clone();
    auth.set_on(on.iter().filter(|id| GOOGLE_CONNECTORS.contains(&id.as_str())).cloned());
    state.tools.set_hidden(hidden_builtin(&on));
}

pub fn view(state: &AppState, auth: &GoogleAuth) -> CatalogView {
    let cfg = state.config.lock().unwrap().clone();
    let servers = state.remotes.views();
    let items = CATALOG
        .iter()
        .map(|e| match e.kind {
            Kind::Google => Item { id: e.id, name: e.name, blurb: e.blurb, kind: e.kind, token: false, on: cfg.connectors.is_on(e.id), status: auth.status_for(e.id), tools: 0 },
            Kind::Local => Item { id: e.id, name: e.name, blurb: e.blurb, kind: e.kind, token: false, on: cfg.connectors.is_on(e.id), status: ConnectorStatus::Connected { account: "this Mac".into() }, tools: 0 },
            Kind::Remote => {
                let s = servers.iter().find(|s| s.name == e.id);
                Item {
                    id: e.id,
                    name: e.name,
                    blurb: e.blurb,
                    kind: e.kind,
                    token: e.token,
                    on: s.is_some_and(|s| s.enabled),
                    status: s.map(|s| s.status.clone()).unwrap_or(ConnectorStatus::NeedsSignIn),
                    tools: s.map(|s| s.tools.iter().filter(|t| t.skipped.is_none()).count()).unwrap_or(0),
                }
            }
        })
        .collect();
    CatalogView { items, always_on: ALWAYS_ON.iter().map(|a| a.1).collect() }
}

fn changed(app: &AppHandle, auth: &GoogleAuth) {
    let _ = app.emit("connectors://changed", ());
    let _ = app.emit("google://status", auth.status());
}

fn save_on(state: &AppState, id: &str, on: bool) -> Result<(), String> {
    let mut c = state.config.lock().unwrap();
    c.connectors.set(id, on);
    c.save(&state.paths.config_file).map_err(|e| e.to_string())
}

/// Signs in to Google for these connectors when the sign-in lacks their scopes.
pub async fn ensure_google(auth: &GoogleAuth, ids: &[&str]) -> Result<(), String> {
    let missing = !matches!(auth.status(), ConnectorStatus::Connected { .. }) || ids.iter().any(|id| !auth.has_scopes(id));
    if missing {
        let scopes: Vec<&str> = ids.iter().flat_map(|id| scopes_for(id).iter().copied()).collect();
        auth.sign_in_for(&scopes, crate::settings::SIGN_IN_WAIT).await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn catalog_list(state: tauri::State<'_, AppState>, auth: tauri::State<'_, Arc<GoogleAuth>>) -> CatalogView {
    view(&state, &auth)
}

/// Turns a connector on (signing in first when needed) or off. A failed sign-in leaves it off.
#[tauri::command]
pub async fn catalog_set(app: AppHandle, id: String, on: bool, token: Option<String>) -> Result<CatalogView, String> {
    let e = entry(&id).ok_or("No such connector.")?;
    let state = app.state::<AppState>();
    let auth = app.state::<Arc<GoogleAuth>>().inner().clone();
    match e.kind {
        Kind::Google => {
            if on {
                ensure_google(&auth, &[e.id]).await?;
            }
            save_on(&state, e.id, on)?;
            apply_on(&state, &auth);
            if e.id == "calendar" {
                if let Some(s) = app.try_state::<Arc<Scheduler>>() {
                    if on {
                        s.refresh_now().await;
                    } else {
                        s.dismiss().await;
                    }
                }
            }
        }
        Kind::Local => {
            save_on(&state, e.id, on)?;
            apply_on(&state, &auth);
        }
        Kind::Remote => {
            let remotes = state.remotes.clone();
            let existing = remotes.views().into_iter().find(|s| s.name == e.id);
            let cfg = match (existing, on) {
                (Some(_), _) => {
                    if let Some(t) = token.filter(|t| !t.trim().is_empty()) {
                        remotes.sign_in_again(e.id, Some(t)).await?;
                    }
                    remotes.set_enabled(e.id, on).await?
                }
                (None, false) => return Ok(view(&state, &auth)),
                (None, true) => {
                    let taken: Vec<String> = state.config.lock().unwrap().mcp.servers.iter().map(|s| s.name.clone()).collect();
                    let taken: Vec<&str> = taken.iter().map(String::as_str).collect();
                    let req = AddRequest { url: e.url.unwrap_or_default().into(), name: Some(e.id.into()), token, client_id: None, client_secret: None };
                    match remotes.add(req, &taken).await? {
                        Outcome::Added(cfg) => cfg,
                        Outcome::NeedsToken { .. } | Outcome::NeedsClientId => return Err(format!("{} needs a token. Paste one and turn it on again.", e.name)),
                    }
                }
            };
            let mut c = state.config.lock().unwrap();
            crate::remote_cmds::upsert(&mut c.mcp.servers, e.id, Some(cfg));
            c.save(&state.paths.config_file).map_err(|e| e.to_string())?;
        }
    }
    changed(&app, &auth);
    Ok(view(&state, &auth))
}

/// Looks up one catalog item as it is now.
pub type LookFn = Arc<dyn Fn(&str) -> Option<Item> + Send + Sync>;

/// Always on: lets the model offer a connector that is off. The chat shows the connector's
/// card, with its switch, under the step (`connector_offer`).
pub struct OfferConnector(pub LookFn);

pub const OFFER_NOTE: &str = "The user now sees a card for this connector in the chat, with a switch to turn it on. Tell them in one short line to turn it on there and ask again. Do not send them to Settings.";

pub fn offer_description() -> String {
    let list: Vec<String> = CATALOG.iter().map(|e| format!("{} ({})", e.id, e.name)).collect();
    format!(
        "Offer the user a connector that is off. Call it when they ask for something one of these would do and its tools are not available to you: {}. The chat then shows the connector with a switch.",
        list.join(", ")
    )
}

#[async_trait]
impl Connector for OfferConnector {
    fn id(&self) -> &'static str {
        "catalog"
    }

    fn tools(&self) -> Vec<ToolSpec> {
        let ids: Vec<&str> = CATALOG.iter().map(|e| e.id).collect();
        vec![ToolSpec {
            name: "connector_offer".into(),
            description: offer_description().into(),
            schema: json!({"type": "object", "properties": {"id": {"type": "string", "enum": ids}}, "required": ["id"]}),
            risk: Risk::Read,
        }]
    }

    fn status(&self) -> ConnectorStatus {
        ConnectorStatus::Connected { account: "bleep".into() }
    }

    async fn call(&self, tool: &str, args: Value) -> Result<Value, String> {
        if tool != "connector_offer" {
            return Err(format!("Unknown tool: {tool}"));
        }
        let id = args.get("id").and_then(Value::as_str).unwrap_or("");
        let Some(item) = entry(id).and_then(|_| (self.0)(id)) else {
            return Err(format!("No such connector. Use one of: {}.", CATALOG.iter().map(|e| e.id).collect::<Vec<_>>().join(", ")));
        };
        let ready = item.on && matches!(item.status, ConnectorStatus::Connected { .. });
        let note = if ready { format!("{} is already on. Its tools load with the user's next message.", item.name) } else { OFFER_NOTE.into() };
        Ok(json!({"id": item.id, "name": item.name, "on": item.on, "status": item.status, "note": note}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_ids_are_unique_and_names_are_valid_server_names() {
        let mut ids: Vec<&str> = CATALOG.iter().map(|e| e.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), CATALOG.len());
        for e in CATALOG.iter().filter(|e| e.kind == Kind::Remote) {
            crate::remote::names::check_server_name(e.id, &[]).unwrap_or_else(|m| panic!("{}: {m}", e.id));
            assert!(e.url.is_some_and(|u| u.starts_with("https://")));
        }
        for e in CATALOG.iter().filter(|e| e.kind == Kind::Google) {
            assert!(GOOGLE_CONNECTORS.contains(&e.id) && !scopes_for(e.id).is_empty());
        }
        for e in CATALOG.iter().filter(|e| e.kind == Kind::Local) {
            assert!(LOCAL_CONNECTORS.contains(&e.id));
        }
    }

    #[test]
    fn copy_has_no_em_dash() {
        for e in CATALOG {
            assert!(!e.blurb.contains('\u{2014}') && !e.name.contains('\u{2014}'), "{}", e.id);
        }
    }

    #[test]
    fn hidden_is_every_builtin_connector_that_is_off() {
        assert_eq!(hidden_builtin(&["calendar".into(), "linear".into()]), ["gmail", "drive", "files"]);
        assert!(hidden_builtin(&["gmail".into(), "calendar".into(), "drive".into(), "files".into()]).is_empty());
    }

    fn item(id: &'static str, on: bool, status: ConnectorStatus) -> Item {
        let e = entry(id).unwrap();
        Item { id, name: e.name, blurb: e.blurb, kind: e.kind, token: e.token, on, status, tools: 0 }
    }

    #[tokio::test]
    async fn offer_returns_the_item_and_tells_the_model_what_the_user_sees() {
        let c = OfferConnector(Arc::new(|id| match id {
            "linear" => Some(item("linear", false, ConnectorStatus::NeedsSignIn)),
            "files" => Some(item("files", true, ConnectorStatus::Connected { account: "this Mac".into() })),
            _ => None,
        }));
        let v = c.call("connector_offer", json!({"id": "linear"})).await.unwrap();
        assert_eq!((v["id"].as_str(), v["on"].as_bool()), (Some("linear"), Some(false)));
        assert_eq!(v["note"], OFFER_NOTE);
        let v = c.call("connector_offer", json!({"id": "files"})).await.unwrap();
        assert!(v["note"].as_str().unwrap().contains("already on"));
        let e = c.call("connector_offer", json!({"id": "slack"})).await.unwrap_err();
        assert!(e.contains("linear") && e.contains("files"), "{e}");
        assert_eq!(c.tools()[0].risk, Risk::Read);
    }

    #[test]
    fn offer_lists_every_catalog_connector() {
        let d = offer_description();
        assert!(CATALOG.iter().all(|e| d.contains(e.id)) && !d.contains('\u{2014}') && !OFFER_NOTE.contains('\u{2014}'));
    }

    #[test]
    fn google_scopes_do_not_overlap() {
        assert!(scopes_for("drive").iter().all(|s| !scopes_for("gmail").contains(s)));
    }
}
