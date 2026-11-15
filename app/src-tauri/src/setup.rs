//! First-run setup wizard: Claude Code, Google, and the agent hooks, in one window.
use crate::config::Config;
use crate::windows::{open_chat, Anchor};
use crate::AppState;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// The wizard opens at launch until the user finishes it once.
pub fn should_open(cfg: &Config) -> bool {
    !cfg.setup_done
}

pub fn open_setup(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("setup") {
        crate::windows::bring_to_droid_screen(app, &w);
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let (at, size) = crate::windows::panel_on_droid_screen(app, crate::windows::Size { w: 520.0, h: 640.0 });
    let built = WebviewWindowBuilder::new(app, "setup", WebviewUrl::App("index.html#setup".into()))
        .title("Set up bleep")
        .inner_size(size.w, size.h)
        .resizable(false)
        .position(at.x, at.y)
        .build();
    match built {
        Ok(w) => {
            let _ = w.set_focus();
        }
        Err(e) => eprintln!("bleep: setup window: {e}"),
    }
}

#[tauri::command]
pub fn setup_open(app: AppHandle) {
    open_setup(&app);
}

/// Marks setup as done, closes the wizard and opens the chat next to the droid.
#[tauri::command]
pub fn setup_finish(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), String> {
    {
        let mut cfg = state.config.lock().unwrap();
        cfg.setup_done = true;
        cfg.save(&state.paths.config_file).map_err(|e| format!("Could not save settings: {e}"))?;
    }
    if let Some(w) = app.get_webview_window("setup") {
        let _ = w.close();
    }
    open_chat(&app, Anchor::Droid);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wizard_opens_until_finished() {
        let mut c = Config::default();
        assert!(should_open(&c));
        c.setup_done = true;
        assert!(!should_open(&c));
    }
}
