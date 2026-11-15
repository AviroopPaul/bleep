pub mod agents;
pub mod agentwatch;
pub mod approvals;
pub mod bar;
pub mod chat;
pub mod compaction;
pub mod config;
pub mod connectors;
pub mod google;
pub mod hotkey;
pub mod mail;
pub mod mcp;
pub mod memory;
pub mod memory_cmds;
pub mod opener;
pub mod paths;
pub mod proactive;
pub mod remote;
pub mod remote_cmds;
pub mod runtime;
pub mod settings;
pub mod setup;
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
    pub memory: Arc<memory::Memory>,
    pub remotes: Arc<remote::manager::Remotes>,
    /// The runtime everything uses; Settings swaps what is inside (Phase 9).
    pub runtime: Arc<runtime::switch::SwitchRuntime>,
    pub make_runtime: RuntimeFactory,
    pub budget: Arc<runtime::switch::Budget>,
    pub _mcp: mcp::McpServer,
}

pub type RuntimeFactory = Arc<dyn Fn(&Config) -> Arc<dyn runtime::AgentRuntime> + Send + Sync>;

const GMAIL_API: &str = "https://gmail.googleapis.com/gmail/v1";

/// Built in `init`, finished in `setup` once the windows exist.
struct Parts {
    state: AppState,
    google: Arc<google::GoogleAuth>,
    calendar: Arc<connectors::calendar::CalendarConnector>,
    watcher: Arc<mail::watch::MailWatcher>,
    triager: Arc<mail::triage::Triager>,
    /// The runtime for proactive work, stopped by the spend cap.
    capped: Arc<dyn runtime::AgentRuntime>,
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
    let gmail = connectors::gmail::GmailConnector::new(google.clone(), http.clone(), GMAIL_API.into(), store.clone());
    let watcher = mail::watch::MailWatcher::new(google.clone(), http, GMAIL_API.into(), store.clone());
    let h = handle.clone();
    let approvals = approvals::Approvals::new(store.clone(), Arc::new(move |a| {
        let _ = h.emit("approval://changed", a);
    }), approvals::WAIT);
    let memory = Arc::new(memory::Memory::new(paths.clone(), store.clone()));
    if let Err(e) = memory.ensure() {
        eprintln!("bleep: memory files: {e}");
    }
    match memory.reindex_notes() {
        Ok(n) => eprintln!("bleep: indexed {n} daily-note lines"),
        Err(e) => eprintln!("bleep: daily-note index: {e}"),
    }
    let memory_tools = Arc::new(connectors::memory::MemoryConnector::new(memory.clone(), Arc::new(chrono::Local::now)));
    let turns = mcp::host::TurnTracker::default();
    let tool_host = mcp::host::ToolHost::new_logged(vec![Arc::new(connectors::bleep::BleepConnector), calendar.clone(), gmail, memory_tools], approvals.clone(), turns.clone(), store.clone());
    let mcp = tauri::async_runtime::block_on(mcp::McpServer::start(mcp::new_token(), tool_host.clone()))?;
    // Remote MCP servers I added (Phase 7): their cached tools join the host at once.
    let own_port = reqwest::Url::parse(&mcp.url()).ok().and_then(|u| u.port());
    let remotes = remote::manager::Remotes::new(tool_host.clone(), store.clone(), Arc::new(google::secrets::KeychainSecrets), Arc::new(opener::SystemOpener(handle.clone())), own_port);
    remotes.load(&config.mcp.servers);
    let make_runtime: RuntimeFactory = {
        let (paths, store, hold) = (paths.clone(), store.clone(), approvals.waiting());
        let bin = find_claude().unwrap_or_else(|| "claude".into());
        Arc::new(move |cfg: &Config| -> Arc<dyn runtime::AgentRuntime> {
            if cfg.runtime == "api" {
                let rt = runtime::api::from_config(&cfg.api, &google::secrets::KeychainSecrets, store.clone());
                let r = rt.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = r.load_models().await {
                        eprintln!("bleep: model list: {e}");
                    }
                });
                Arc::new(rt)
            } else {
                Arc::new(ClaudeRuntime::new(paths.clone(), cfg.tiers.clone(), bin.clone()).with_hold(hold.clone()))
            }
        })
    };
    let runtime = runtime::switch::SwitchRuntime::new(make_runtime(&config));
    let h = handle.clone();
    let budget = runtime::switch::Budget::new(store.clone(), Arc::new(move || {
        let c = h.state::<AppState>().config.lock().unwrap().clone();
        (c.runtime == "api").then_some(c.api.daily_cap_usd)
    }));
    // Proactive work stops for the day at the API spend cap. Chat does not.
    let capped: Arc<dyn runtime::AgentRuntime> = runtime::switch::CappedRuntime::new(runtime.clone(), budget.clone());
    let h = handle.clone();
    let triager = mail::triage::Triager::new(capped.clone(), store.clone(), Arc::new(move || h.state::<AppState>().config.lock().unwrap().mail.clone()));
    let h = handle.clone();
    let emit: chat::Emit = Arc::new(move |m: ChatEmit| {
        let _ = h.emit("chat://event", m);
    });
    let chat = (ChatService::new(store.clone(), runtime.clone(), McpEndpoint { url: mcp.url(), token: mcp.token().into() }, emit, turns).with_memory(memory.clone()).with_window_sink({
        let h = handle.clone();
        Arc::new(move |w| {
            let st = h.state::<AppState>();
            let mut cfg = st.config.lock().unwrap();
            if cfg.memory.context_window != Some(w) {
                cfg.memory.context_window = Some(w);
                if let Err(e) = cfg.save(&st.paths.config_file) {
                    eprintln!("bleep: save config: {e}");
                }
            }
        })
    }))
    .with_memory_config({
        let h = handle.clone();
        Arc::new(move || h.state::<AppState>().config.lock().unwrap().memory.clone())
    })
    .with_compact_sink({
        let h = handle.clone();
        Arc::new(move |on: bool| {
            let _ = h.emit("chat://compacting", on);
        })
    })
    .with_tools_hash({
        let h = tool_host.clone();
        Arc::new(move || h.tools_hash())
    })
    .into_arc();
    let (r, h) = (remotes.clone(), handle.clone());
    tauri::async_runtime::spawn(async move {
        r.connect_all().await;
        let _ = h.emit("connectors://changed", ());
    });
    let c = chat.clone();
    tauri::async_runtime::spawn(async move {
        if c.runtime().health().await.ok() {
            if let Err(e) = c.ensure_session().await {
                eprintln!("bleep: could not start the chat session: {e}");
            }
        }
    });
    let state = AppState { paths, config: Mutex::new(config), store, chat, approvals, memory, remotes, runtime, make_runtime, budget, _mcp: mcp };
    Ok(Parts { state, google, calendar, watcher, triager, capped })
}

/// Agent watch (Phase 4): the hook socket, the hub, and the Codex client.
fn start_agent_watch(app: &tauri::App, store: Arc<Store>, runtime: Arc<dyn runtime::AgentRuntime>, notifier: Arc<proactive::notifier::Notifier>, paused: Arc<std::sync::atomic::AtomicBool>) {
    let paths = app.state::<AppState>().paths.clone();
    match agentwatch::install_hook_binary(&paths) {
        Ok(true) => eprintln!("bleep: updated {}", paths.hook_bin.display()),
        Ok(false) => {}
        Err(e) => eprintln!("bleep: could not install bleep-hook: {e}"),
    }
    let rules_file = paths.agent_rules.clone();
    let _ = agentwatch::rules::load_rules(&rules_file);
    let (h1, h2) = (app.handle().clone(), app.handle().clone());
    let watch = agentwatch::hub::AgentWatch::new(agentwatch::hub::WatchDeps {
        store: store.clone(),
        rules: Arc::new(move || agentwatch::rules::load_rules(&rules_file)),
        cfg: Arc::new(move || h1.state::<AppState>().config.lock().unwrap().agents.clone()),
        paused,
        checker: agentwatch::screenshot::ModelChecker::new(runtime, store.clone()),
        ping: notifier,
        emit: Arc::new(move || {
            let _ = h2.emit("agents://changed", ());
        }),
        now: Arc::new(chrono::Utc::now),
    });
    let codex = agentwatch::codex::CodexWatch::new(watch.clone(), store);
    app.manage(codex.clone());
    let sock = paths.sock.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = agentwatch::socket::serve(&sock, watch).await {
            eprintln!("bleep: agent watch socket not started: {e}");
        }
    });
    let h = app.handle().clone();
    tauri::async_runtime::spawn(codex.run(agents::codex_home(), Arc::new(move || h.state::<AppState>().config.lock().unwrap().agents.codex_watch)));
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
            let Parts { state, google, calendar, watcher, triager, capped } = init(app)?;
            let budget = state.budget.clone();
            let chat = state.chat.clone();
            let cfg = state.config.lock().unwrap().clone();
            let store = state.store.clone();
            app.manage(state);
            let ui = windows::UiState::default();
            let paused = ui.paused.clone();
            app.manage(ui);
            app.manage(google.clone());
            windows::create_windows(app.handle(), &cfg)?;
            let bar_ui = bar::TauriBarUi::new(app.handle().clone());
            let sched = proactive::scheduler::Scheduler::new(proactive::scheduler::SchedulerDeps {
                store: store.clone(),
                calendar,
                auth: google,
                paused: paused.clone(),
                ui: bar_ui.clone(),
                now: Arc::new(chrono::Utc::now),
            });
            app.manage(sched.clone());
            tauri::async_runtime::spawn(sched.run());
            let h = app.handle().clone();
            let mail_cfg: Arc<dyn Fn() -> config::MailConfig + Send + Sync> = Arc::new(move || h.state::<AppState>().config.lock().unwrap().mail.clone());
            let notifier = proactive::notifier::Notifier::new(store.clone(), bar_ui, paused.clone(), mail_cfg, Arc::new(chrono::Utc::now));
            app.manage(notifier.clone());
            let pipeline = mail::pipeline::MailPipeline::new(watcher, triager, notifier.clone(), store.clone());
            start_agent_watch(app, store.clone(), capped.clone(), notifier.clone(), paused.clone());
            let h = app.handle().clone();
            let patterns = proactive::patterns::PatternJob::new(proactive::patterns::PatternDeps {
                store: store.clone(),
                runtime: capped,
                memory: app.state::<AppState>().memory.clone(),
                nightly: Arc::new(move || h.state::<AppState>().config.lock().unwrap().memory.nightly.clone()),
                paused: paused.clone(),
                ping: notifier,
                now: Arc::new(chrono::Local::now),
            });
            let h = app.handle().clone();
            let checkins = proactive::checkins::CheckinRunner::new(
                store,
                chat,
                Arc::new(move || h.state::<AppState>().config.lock().unwrap().checkin.clone()),
                paused,
                Arc::new(chrono::Utc::now),
                Arc::new(|t: chrono::DateTime<chrono::Utc>| t.with_timezone(&chrono::Local).naive_local()),
            );
            checkins.set_budget(budget);
            // Mail every 2 minutes and check-ins, on one 30 s clock.
            tauri::async_runtime::spawn(async move {
                let mut every = tokio::time::interval(proactive::scheduler::TICK);
                every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    every.tick().await;
                    pipeline.tick(chrono::Utc::now()).await;
                    checkins.tick().await;
                    patterns.tick().await;
                }
            });
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
            setup::setup_open,
            setup::setup_finish,
            settings::google_state,
            settings::google_save_client,
            settings::google_use_builtin,
            settings::google_connect,
            settings::google_disconnect,
            settings::open_link,
            settings::open_url,
            settings::mail_settings,
            settings::mail_set_lists,
            settings::mail_resolve_suggestion,
            settings::activity_list,
            bar::bubble_not_useful,
            windows::chat_open,
            windows::chat_hide,
            windows::droid_toggle_tuck,
            windows::droid_set_visible,
            agents::agents_list,
            agents::agents_decide,
            agents::agents_settings,
            agents::agents_install_claude,
            agents::agents_remove_claude,
            agents::agents_install_codex_notify,
            agents::agents_remove_codex_notify,
            agents::agents_set,
            agents::agents_reveal_rules,
            memory_cmds::memory_list,
            memory_cmds::memory_forget,
            memory_cmds::memory_suggestions,
            memory_cmds::memory_decide,
            memory_cmds::memory_settings,
            memory_cmds::memory_set,
            memory_cmds::memory_compact_now,
            memory_cmds::memory_reveal,
            memory_cmds::chat_context,
            remote_cmds::connectors_list,
            remote_cmds::remote_add,
            remote_cmds::remote_sign_in,
            remote_cmds::remote_cancel_sign_in,
            remote_cmds::remote_set_tool,
            remote_cmds::remote_remove,
            remote_cmds::remote_refresh
        ])
        .run(tauri::generate_context!())
        .expect("error while running bleep");
}
