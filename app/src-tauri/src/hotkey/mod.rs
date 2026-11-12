//! Summon: double tap Option (needs Input Monitoring), with Option Space as the fallback.
pub mod detector;
#[cfg(target_os = "macos")]
pub mod macos;

use crate::store::{Kind, MsgStatus, NewMessage, Role};
use crate::windows::{open_chat, Anchor};
use crate::AppState;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub const SUMMON_NOTE: &str = "To summon bleep with a double tap of Option, allow bleep in System Settings, Privacy and Security, Input Monitoring. Until then, use Option Space.";

fn summon(app: &AppHandle) {
    let a = app.clone();
    let _ = app.run_on_main_thread(move || open_chat(&a, Anchor::Center));
}

pub fn setup(app: &AppHandle) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let a = app.clone();
        if macos::listen_input_access() && macos::start_option_tap(move || summon(&a)).is_ok() {
            return Ok(());
        }
        macos::request_input_access();
    }
    let state = app.state::<AppState>();
    let fallback = state.config.lock().unwrap().summon_fallback.clone();
    app.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(|app, _sc, ev| {
                if ev.state() == ShortcutState::Pressed {
                    open_chat(app, Anchor::Center);
                }
            })
            .build(),
    )?;
    app.global_shortcut().register(fallback.as_str())?;
    let mut cfg = state.config.lock().unwrap();
    if !cfg.summon_note_shown {
        state.store.insert_message(NewMessage { role: Role::System, kind: Kind::System, content: SUMMON_NOTE.into(), status: MsgStatus::Done, session_id: None })?;
        cfg.summon_note_shown = true;
        cfg.save(&state.paths.config_file)?;
    }
    Ok(())
}
