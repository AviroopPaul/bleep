pub mod approvals;
pub mod bar;
pub mod chat;
pub mod config;
pub mod connectors;
pub mod google;
pub mod hotkey;
pub mod mcp;
pub mod opener;
pub mod paths;
pub mod proactive;
pub mod runtime;
pub mod settings;
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
    pub approvals: Arc<approvals::Approvals>,
    pub _mcp: mcp::McpServer,
}

const GMAIL_API: &str = "https://gmail.googleapis.com/gmail/v1";

/// Built in `init`, finished in `setup` once the windows exist.
struct Parts {
    state: AppState,
    google: Arc<google::GoogleAuth>,
    calendar: Arc<connectors::calendar::CalendarConnector>,
}

/// Approve or Decline on a card.
#[tauri::command]
fn approval_decide(state: tauri::State<'_, AppState>, id: i64, approve: bool) -> Result<(), String> {
    state.approvals.decide(id, approve)
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

fn init(app: &tauri::App) -> anyhow::Result<Parts> {
    let paths = BleepPaths::from_env();
    paths.ensure()?;
    let config = Config::load(&paths.config_file)?;
    let store = Arc::new(Store::open(&paths.db)?);
    let n = store.mark_streaming_as_interrupted()?;
    if n > 0 {
        eprintln!("bleep: marked {n} unfinished replies as interrupted");
    }
    let n = store.expire_pending_approvals()?;
    if n > 0 {
        eprintln!("bleep: {n} approval cards expired while bleep was closed");
    }
    let handle = app.handle().clone();
    let google = google::GoogleAuth::new(
        config.google.clone(),
        google::GoogleEndpoints::google(),
        Arc::new(google::secrets::KeychainSecrets),
        Arc::new(opener::SystemOpener(handle.clone())),
    );
    google.set_builtin(google::builtin_client());
    let http = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build()?;
    let calendar = connectors::calendar::CalendarConnector::new(google.clone(), http.clone(), google::GoogleEndpoints::google().api);
    let gmail = connectors::gmail::GmailConnector::new(google.clone(), http, GMAIL_API.into(), store.clone());
    let h = handle.clone();
    let approvals = approvals::Approvals::new(store.clone(), Arc::new(move |a| {
        let _ = h.emit("approval://changed", a);
    }), approvals::WAIT);
    let turns = mcp::host::TurnTracker::default();
    let tool_host = mcp::host::ToolHost::new(vec![Arc::new(connectors::bleep::BleepConnector), calendar.clone(), gmail], approvals.clone(), turns.clone());
    let mcp = tauri::async_runtime::block_on(mcp::McpServer::start(mcp::new_token(), tool_host))?;
    let bin = find_claude().unwrap_or_else(|| "claude".into());
    let runtime = Arc::new(ClaudeRuntime::new(paths.clone(), config.tiers.clone(), bin).with_hold(approvals.waiting()));
    let h = handle.clone();
    let emit: chat::Emit = Arc::new(move |m: ChatEmit| {
        let _ = h.emit("chat://event", m);
    });
    let chat = Arc::new(ChatService::new(store.clone(), runtime, McpEndpoint { url: mcp.url(), token: mcp.token().into() }, emit, turns));
    let c = chat.clone();
    tauri::async_runtime::spawn(async move {
        if c.runtime().health().await.ok() {
            if let Err(e) = c.ensure_session().await {
                eprintln!("bleep: could not start the Claude session: {e}");
            }
        }
    });
    let state = AppState { paths, config: Mutex::new(config), store, chat, approvals, _mcp: mcp };
    Ok(Parts { state, google, calendar })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| windows::open_chat(app, windows::Anchor::Droid)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let Parts { state, google, calendar } = init(app)?;
            let cfg = state.config.lock().unwrap().clone();
            let store = state.store.clone();
            app.manage(state);
            let ui = windows::UiState::default();
            let paused = ui.paused.clone();
            app.manage(ui);
            app.manage(google.clone());
            windows::create_windows(app.handle(), &cfg)?;
            let sched = proactive::scheduler::Scheduler::new(proactive::scheduler::SchedulerDeps {
                store,
                calendar,
                auth: google,
                paused,
                ui: bar::TauriBarUi::new(app.handle().clone()),
                now: Arc::new(chrono::Utc::now),
            });
            app.manage(sched.clone());
            tauri::async_runtime::spawn(sched.run());
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
            approval_decide,
            bar::bar_current,
            bar::bar_prep,
            bar::bar_snooze,
            bar::bar_join,
            bar::bar_dismiss,
            bar::bubble_click,
            settings::settings_open,
            settings::google_state,
            settings::google_save_client,
            settings::google_use_builtin,
            settings::google_connect,
            settings::google_disconnect,
            settings::open_link,
            settings::open_url,
            windows::chat_open,
            windows::chat_hide,
            windows::droid_toggle_tuck,
            windows::droid_set_visible
        ])
        .run(tauri::generate_context!())
        .expect("error while running bleep");
}
