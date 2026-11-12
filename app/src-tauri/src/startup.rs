//! First-run behaviour: launch at login, and open the chat when Claude needs a sign-in.
use crate::config::Config;
use crate::windows::{open_chat, Anchor};
use crate::AppState;
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

pub const IS_RELEASE: bool = !cfg!(debug_assertions);

/// On by default (spec 8.8), set once on first run, and never for a dev build.
pub fn should_enable_autostart(cfg: &Config, release: bool) -> bool {
    release && !cfg.autostart_set
}

pub fn first_run(app: &AppHandle) {
    let state = app.state::<AppState>();
    {
        let mut cfg = state.config.lock().unwrap();
        if should_enable_autostart(&cfg, IS_RELEASE) {
            match app.autolaunch().enable() {
                Ok(()) => {
                    cfg.autostart_set = true;
                    if let Err(e) = cfg.save(&state.paths.config_file) {
                        eprintln!("bleep: could not save config: {e}");
                    }
                }
                Err(e) => eprintln!("bleep: launch at login not set: {e}"),
            }
        }
    }
    let chat = state.chat.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if !chat.runtime().health().await.ok() {
            let a = app.clone();
            let _ = app.run_on_main_thread(move || open_chat(&a, Anchor::Droid));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn autostart_turns_on_once_in_release_only() {
        let mut c = Config::default();
        assert!(should_enable_autostart(&c, true));
        assert!(!should_enable_autostart(&c, false), "a dev build must never register itself at login");
        c.autostart_set = true;
        assert!(!should_enable_autostart(&c, true), "respect the user's choice after first run");
    }
}
