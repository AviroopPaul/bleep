pub mod chat;
pub mod config;
pub mod hotkey;
pub mod mcp;
pub mod paths;
pub mod runtime;
pub mod startup;
pub mod store;
pub mod windows;

use chat::{ChatEmit, ChatService};
use config::Config;
use paths::BleepPaths;
use runtime::claude::{find_claude, ClaudeRuntime};
use runtime::{McpEndpoint, RuntimeHealth};
use std::sync::{Arc, Mutex};
use store::{Message, Store};
use tauri::{Emitter, Manager};

pub struct AppState {
    pub paths: BleepPaths,
    pub config: Mutex<Config>,
    pub store: Arc<Store>,
    pub chat: Arc<ChatService>,
    pub _mcp: mcp::McpServer,
}

#[tauri::command]
async fn chat_history(state: tauri::State<'_, AppState>, before_id: Option<i64>, limit: u32) -> Result<Vec<Message>, String> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || store.page_messages(before_id, limit.min(200)))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn chat_send(state: tauri::State<'_, AppState>, text: String) -> Result<i64, String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("empty".into());
    }
    state.chat.send(text).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn runtime_health(state: tauri::State<'_, AppState>) -> Result<RuntimeHealth, String> {
    Ok(state.chat.runtime().health().await)
}

/// Opens Terminal (not a browser) with bleep's own Claude sign-in.
#[tauri::command]
fn open_claude_login(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let home = state.paths.claude_home.display().to_string().replace('\'', "");
    let script = format!("tell application \"Terminal\"\nactivate\ndo script \"CLAUDE_CONFIG_DIR='{home}' claude auth login\"\nend tell");
    std::process::Command::new("osascript").args(["-e", &script]).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

fn init(app: &tauri::App) -> anyhow::Result<AppState> {
    let paths = BleepPaths::from_env();
    paths.ensure()?;
    let config = Config::load(&paths.config_file)?;
    let store = Arc::new(Store::open(&paths.db)?);
    let n = store.mark_streaming_as_interrupted()?;
    if n > 0 {
        eprintln!("bleep: marked {n} unfinished replies as interrupted");
    }
    let mcp = tauri::async_runtime::block_on(mcp::McpServer::start(mcp::new_token()))?;
    let bin = find_claude().unwrap_or_else(|| "claude".into());
    let runtime = Arc::new(ClaudeRuntime::new(paths.clone(), config.tiers.clone(), bin));
    let handle = app.handle().clone();
    let emit: chat::Emit = Arc::new(move |m: ChatEmit| {
        let _ = handle.emit("chat://event", m);
    });
    let chat = Arc::new(ChatService::new(store.clone(), runtime, McpEndpoint { url: mcp.url(), token: mcp.token().into() }, emit));
    let c = chat.clone();
    tauri::async_runtime::spawn(async move {
        if c.runtime().health().await.ok() {
            if let Err(e) = c.ensure_session().await {
                eprintln!("bleep: could not start the Claude session: {e}");
            }
        }
    });
    Ok(AppState { paths, config: Mutex::new(config), store, chat, _mcp: mcp })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| windows::open_chat(app, windows::Anchor::Droid)))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let state = init(app)?;
            let cfg = state.config.lock().unwrap().clone();
            app.manage(state);
            app.manage(windows::UiState::default());
            windows::create_windows(app.handle(), &cfg)?;
            windows::setup_tray(app.handle())?;
            if let Err(e) = hotkey::setup(app.handle()) {
                eprintln!("bleep: summon shortcut not set: {e}");
            }
            startup::first_run(app.handle());
            Ok(())
        })
        .on_window_event(|w, ev| {
            if let (tauri::WindowEvent::Moved(_), "droid") = (ev, w.label()) {
                windows::on_droid_moved(w.app_handle());
            }
        })
        .invoke_handler(tauri::generate_handler![
            chat_history,
            chat_send,
            runtime_health,
            open_claude_login,
            windows::chat_open,
            windows::chat_hide,
            windows::droid_toggle_tuck,
            windows::droid_set_visible
        ])
        .run(tauri::generate_context!())
        .expect("error while running bleep");
}
