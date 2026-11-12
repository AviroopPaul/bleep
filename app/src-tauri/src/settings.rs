//! The settings window. Phase 2 fills only the Google section.
use crate::config::GoogleConfig;
use crate::connectors::ConnectorStatus;
use crate::google::GoogleAuth;
use crate::opener::{Opener, SystemOpener};
use crate::proactive::scheduler::Scheduler;
use crate::AppState;
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const SIGN_IN_WAIT: std::time::Duration = std::time::Duration::from_secs(300);

/// The only pages the settings window can open. The page passes a name, never a URL.
pub fn link_for(which: &str) -> Option<&'static str> {
    Some(match which {
        "console" => "https://console.cloud.google.com/projectcreate",
        "calendar_api" => "https://console.cloud.google.com/apis/library/calendar-json.googleapis.com",
        "gmail_api" => "https://console.cloud.google.com/apis/library/gmail.googleapis.com",
        "consent" => "https://console.cloud.google.com/auth/audience",
        "credentials" => "https://console.cloud.google.com/apis/credentials",
        _ => return None,
    })
}

#[derive(Serialize)]
pub struct GoogleState {
    pub client_id: String,
    pub has_secret: bool,
    pub status: ConnectorStatus,
    pub expired: bool,
    /// This build carries bleep's own Google client, so users only sign in.
    pub builtin: bool,
    /// Settings hold the user's own client, which wins over the built-in one.
    pub using_own: bool,
}

fn google_state_of(state: &AppState, auth: &GoogleAuth) -> GoogleState {
    let cfg = state.config.lock().unwrap().google.clone();
    GoogleState { client_id: cfg.client_id, has_secret: !cfg.client_secret.is_empty(), status: auth.status(), expired: auth.expired(), builtin: auth.has_builtin(), using_own: auth.using_own() }
}

fn push_status(app: &AppHandle, auth: &GoogleAuth) {
    let _ = app.emit("google://status", auth.status());
}

#[tauri::command]
pub fn settings_open(app: AppHandle) {
    open_settings(&app);
}

pub fn open_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let built = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("index.html#settings".into()))
        .title("bleep settings")
        .inner_size(460.0, 520.0)
        .resizable(false)
        .center()
        .build();
    match built {
        Ok(w) => {
            let _ = w.set_focus();
        }
        Err(e) => eprintln!("bleep: settings window: {e}"),
    }
}

#[tauri::command]
pub fn google_state(state: tauri::State<'_, AppState>, auth: tauri::State<'_, Arc<GoogleAuth>>) -> GoogleState {
    google_state_of(&state, &auth)
}

/// Saves the user's own OAuth client. An empty secret keeps the one already saved.
#[tauri::command]
pub async fn google_save_client(app: AppHandle, state: tauri::State<'_, AppState>, auth: tauri::State<'_, Arc<GoogleAuth>>, client_id: String, client_secret: String) -> Result<GoogleState, String> {
    let id = client_id.trim().to_string();
    if id.is_empty() {
        return Err("Add the client ID.".into());
    }
    let google = {
        let mut cfg = state.config.lock().unwrap();
        let secret = match client_secret.trim() {
            "" => cfg.google.client_secret.clone(),
            s => s.to_string(),
        };
        if secret.is_empty() {
            return Err("Add the client secret.".into());
        }
        cfg.google = GoogleConfig { client_id: id, client_secret: secret };
        cfg.save(&state.paths.config_file).map_err(|e| e.to_string())?;
        cfg.google.clone()
    };
    auth.set_client(google).await;
    push_status(&app, &auth);
    Ok(google_state_of(&state, &auth))
}

/// Forgets the user's own client and goes back to bleep's built-in one.
#[tauri::command]
pub async fn google_use_builtin(app: AppHandle, state: tauri::State<'_, AppState>, auth: tauri::State<'_, Arc<GoogleAuth>>) -> Result<GoogleState, String> {
    {
        let mut cfg = state.config.lock().unwrap();
        cfg.google = GoogleConfig::default();
        cfg.save(&state.paths.config_file).map_err(|e| e.to_string())?;
    }
    auth.set_client(GoogleConfig::default()).await;
    push_status(&app, &auth);
    Ok(google_state_of(&state, &auth))
}

#[tauri::command]
pub async fn google_connect(app: AppHandle, auth: tauri::State<'_, Arc<GoogleAuth>>, sched: tauri::State<'_, Arc<Scheduler>>) -> Result<String, String> {
    let account = auth.sign_in(SIGN_IN_WAIT).await.map_err(|e| e.to_string())?;
    push_status(&app, &auth);
    sched.refresh_now().await;
    Ok(account)
}

#[tauri::command]
pub async fn google_disconnect(app: AppHandle, state: tauri::State<'_, AppState>, auth: tauri::State<'_, Arc<GoogleAuth>>, sched: tauri::State<'_, Arc<Scheduler>>) -> Result<(), String> {
    auth.disconnect().await.map_err(|e| e.to_string())?;
    state.store.clear_cal_cache().map_err(|e| e.to_string())?;
    sched.dismiss().await;
    push_status(&app, &auth);
    Ok(())
}

#[tauri::command]
pub fn open_link(app: AppHandle, which: String) -> Result<(), String> {
    let url = link_for(&which).ok_or("Unknown link.")?;
    SystemOpener(app).open(url).map_err(|e| e.to_string())
}

/// Opens a link from a chat reply in the default browser. https only.
#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    let url = crate::bar::safe_join_url(&url).ok_or("bleep only opens https links.")?;
    SystemOpener(app).open(url).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_link_maps_only_known_names() {
        for (k, host) in [("console", "console.cloud.google.com"), ("calendar_api", "calendar-json"), ("gmail_api", "gmail.googleapis.com"), ("consent", "/auth/"), ("credentials", "/credentials")] {
            let url = link_for(k).unwrap_or_else(|| panic!("{k}"));
            assert!(url.starts_with("https://console.cloud.google.com/") && url.contains(host), "{k}: {url}");
        }
        for bad in ["", "https://evil.example", "file:///etc", "CONSOLE"] {
            assert_eq!(link_for(bad), None, "{bad}");
        }
    }
}
