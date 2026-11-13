//! The meeting bar and the droid bubble: Tauri side of the scheduler's `BarUi`, and the
//! commands behind the bar's buttons.
use crate::chat::{prep_label, prep_prompt, ChatError};
use crate::opener::{Opener, SystemOpener};
use crate::proactive::cues::BarCue;
use crate::proactive::scheduler::{BarUi, Scheduler};
use crate::windows::{bar_pos, bubble_pos, open_chat, rect_of, screen_of, screens, Anchor, UiState, BAR, BUBBLE};
use crate::AppState;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, LogicalPosition, Manager};

pub const BUBBLE_SECS: u64 = 12;
pub const BUSY_NOTE: &str = "bleep is busy. Try Prep me again in a moment.";

/// Only plain https links open from the bar. Anything else in an invite is ignored.
pub fn safe_join_url(url: &str) -> Option<&str> {
    let ok = url.starts_with("https://") && !url.chars().any(char::is_whitespace) && reqwest::Url::parse(url).is_ok_and(|u| u.host_str().is_some_and(|h| !h.is_empty()));
    ok.then_some(url)
}

pub struct TauriBarUi {
    app: AppHandle,
    /// Bumped per bubble, so an old timer never hides a newer bubble.
    bubble_gen: Arc<AtomicU64>,
}

impl TauriBarUi {
    pub fn new(app: AppHandle) -> Arc<TauriBarUi> {
        Arc::new(TauriBarUi { app, bubble_gen: Default::default() })
    }
}

impl BarUi for TauriBarUi {
    fn show(&self, cue: &BarCue) {
        if let Some(w) = self.app.get_webview_window("bar") {
            let p = bar_pos(screens(&self.app)[0], BAR);
            let _ = w.set_position(LogicalPosition::new(p.x, p.y));
            let _ = w.show();
        }
        let _ = self.app.emit("bar://cue", cue);
        let _ = self.app.emit("droid://mood", json!({"mood": "alert"}));
    }

    fn hide(&self) {
        if let Some(w) = self.app.get_webview_window("bar") {
            let _ = w.hide();
        }
        let _ = self.app.emit("bar://hide", ());
        let _ = self.app.emit("droid://mood", json!({"mood": "idle"}));
    }

    fn bubble(&self, text: &str) {
        self.show_bubble(text, None);
    }

    fn ping(&self, text: &str, ping_id: i64) {
        self.show_bubble(text, Some(ping_id));
    }
}

impl TauriBarUi {
    /// Skipped when the droid is hidden or tucked: the bar or the next check-in is enough then.
    fn show_bubble(&self, text: &str, ping_id: Option<i64>) {
        let Some(d) = self.app.get_webview_window("droid") else { return };
        let tucked = self.app.state::<UiState>().tucked.lock().unwrap().is_some();
        if tucked || !d.is_visible().unwrap_or(false) {
            return;
        }
        let (Some(r), Some(b)) = (rect_of(&d), self.app.get_webview_window("bubble")) else { return };
        let p = bubble_pos(r, screen_of(&self.app, r), BUBBLE);
        let _ = b.set_position(LogicalPosition::new(p.x, p.y));
        let _ = self.app.emit("bubble://text", json!({"text": text, "ping_id": ping_id}));
        let _ = b.show();
        let ticket = self.bubble_gen.fetch_add(1, Ordering::SeqCst) + 1;
        let (app, latest) = (self.app.clone(), self.bubble_gen.clone());
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(BUBBLE_SECS)).await;
            if latest.load(Ordering::SeqCst) == ticket {
                if let Some(b) = app.get_webview_window("bubble") {
                    let _ = b.hide();
                }
            }
        });
    }
}

fn hide_bubble(app: &AppHandle) {
    if let Some(b) = app.get_webview_window("bubble") {
        let _ = b.hide();
    }
}

#[tauri::command]
pub async fn bar_current(sched: tauri::State<'_, Arc<Scheduler>>) -> Result<Option<BarCue>, String> {
    Ok(sched.current().await)
}

#[tauri::command]
pub async fn bar_snooze(sched: tauri::State<'_, Arc<Scheduler>>) -> Result<(), String> {
    sched.snooze().await;
    Ok(())
}

#[tauri::command]
pub async fn bar_dismiss(sched: tauri::State<'_, Arc<Scheduler>>) -> Result<(), String> {
    sched.dismiss().await;
    Ok(())
}

/// Opens the meeting link (https only) and puts the bar away.
#[tauri::command]
pub async fn bar_join(app: AppHandle, sched: tauri::State<'_, Arc<Scheduler>>) -> Result<(), String> {
    let Some(cue) = sched.current().await else { return Ok(()) };
    let Some(url) = cue.meet_link.as_deref().and_then(safe_join_url) else { return Err("This meeting has no link bleep can open.".into()) };
    SystemOpener(app.clone()).open(url).map_err(|e| e.to_string())?;
    sched.dismiss().await;
    Ok(())
}

/// Runs a "Prep me" turn in the chat and opens it. The bar comes back at the 10 minute cue.
#[tauri::command]
pub async fn bar_prep(app: AppHandle, sched: tauri::State<'_, Arc<Scheduler>>, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let Some(cue) = sched.current().await else { return Ok(()) };
    let ev = state.store.cal_event(&cue.event_id).ok().flatten().ok_or("This meeting is no longer on your calendar.")?;
    sched.snooze().await;
    hide_bubble(&app);
    open_chat(&app, Anchor::Droid);
    match state.chat.send_scheduled(prep_label(&ev.title), prep_prompt(&ev)).await {
        Ok(_) => Ok(()),
        Err(ChatError::Busy) => {
            let _ = app.emit("chat://notice", json!({"text": BUSY_NOTE}));
            Ok(())
        }
        Err(e) => {
            let _ = app.emit("chat://notice", json!({"text": e.to_string()}));
            Err(e.to_string())
        }
    }
}

/// "Not useful" on a mail ping. The third for one sender adds a mute suggestion in settings.
#[tauri::command]
pub fn bubble_not_useful(app: AppHandle, state: tauri::State<'_, AppState>, notifier: tauri::State<'_, Arc<crate::proactive::notifier::Notifier>>, ping_id: i64) -> Result<(), String> {
    hide_bubble(&app);
    if let Some(sender) = notifier.not_useful(ping_id) {
        let mut cfg = state.config.lock().unwrap();
        cfg.mail.suggested_mutes.push(sender);
        cfg.save(&state.paths.config_file).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn bubble_click(app: AppHandle) {
    hide_bubble(&app);
    open_chat(&app, Anchor::Droid);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_refuses_non_https_links() {
        assert_eq!(safe_join_url("https://meet.google.com/abc-defg-hij"), Some("https://meet.google.com/abc-defg-hij"));
        assert_eq!(safe_join_url("https://zoom.us/j/123?pwd=x"), Some("https://zoom.us/j/123?pwd=x"));
        for bad in ["http://meet.google.com/x", "file:///etc/passwd", "javascript:alert(1)", "tel:+1555", "https://", "https://exa mple.com", "  https://meet.google.com/x"] {
            assert_eq!(safe_join_url(bad), None, "{bad}");
        }
    }
}
