//! The droid window, the chat window, the tray, and where they sit on screen.
use crate::config::Config;
use crate::AppState;
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const DROID: Size = Size { w: 132.0, h: 132.0 };
pub const CHAT: Size = Size { w: 400.0, h: 600.0 };
pub const BAR: Size = Size { w: 560.0, h: 64.0 };
pub const BUBBLE: Size = Size { w: 240.0, h: 72.0 };
/// Gap between the droid and the chat panel.
pub const GAP: f64 = 8.0;
/// Space kept between a window and the screen edge.
pub const MARGIN: f64 = 12.0;
/// How much of a tucked droid stays visible.
pub const PEEK: f64 = 28.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub w: f64,
    pub h: f64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    v.max(lo).min(hi.max(lo))
}

/// Right of the droid if it fits, else left; bottoms aligned; kept inside the screen.
pub fn chat_pos(droid: Rect, screen: Rect, chat: Size) -> Point {
    let right = droid.x + droid.w + GAP;
    let x = if right + chat.w <= screen.x + screen.w - MARGIN { right } else { droid.x - GAP - chat.w };
    let y = droid.y + droid.h - chat.h;
    Point {
        x: clamp(x, screen.x + MARGIN, screen.x + screen.w - MARGIN - chat.w),
        y: clamp(y, screen.y + MARGIN, screen.y + screen.h - MARGIN - chat.h),
    }
}

/// Slide the droid into the nearest side edge, leaving PEEK px visible.
pub fn tuck_pos(droid: Rect, screen: Rect) -> Point {
    let mid = droid.x + droid.w / 2.0;
    let x = if mid < screen.x + screen.w / 2.0 { screen.x - droid.w + PEEK } else { screen.x + screen.w - PEEK };
    Point { x, y: droid.y }
}

/// Top center of the work area, just under the menu bar.
pub fn bar_pos(work: Rect, bar: Size) -> Point {
    Point { x: work.x + (work.w - bar.w) / 2.0, y: work.y + 6.0 }
}

/// Centered above the droid; below it when there is no room above; kept on screen.
pub fn bubble_pos(droid: Rect, screen: Rect, b: Size) -> Point {
    let above = droid.y - GAP - b.h;
    let y = if above >= screen.y + MARGIN { above } else { droid.y + droid.h + GAP };
    let x = droid.x + droid.w / 2.0 - b.w / 2.0;
    Point { x: clamp(x, screen.x + MARGIN, screen.x + screen.w - MARGIN - b.w), y }
}

pub fn center_pos(screen: Rect, chat: Size) -> Point {
    Point { x: screen.x + (screen.w - chat.w) / 2.0, y: screen.y + (screen.h - chat.h) / 2.0 }
}

pub fn default_droid_pos(screen: Rect, droid: Size) -> Point {
    Point { x: screen.x + screen.w - droid.w - 24.0, y: screen.y + screen.h - droid.h - 24.0 }
}

/// The saved spot if it is on some screen, else bottom right of the first screen.
pub fn restore_pos(saved: Option<(f64, f64)>, screens: &[Rect], droid: Size) -> Point {
    match saved {
        Some((x, y)) if screens.iter().any(|s| s.contains(x + droid.w / 2.0, y + droid.h / 2.0)) => Point { x, y },
        _ => default_droid_pos(screens[0], droid),
    }
}

#[derive(Default)]
pub struct UiState {
    /// Where the droid was before it was tucked.
    pub tucked: Mutex<Option<Point>>,
    /// Shared with the scheduler: on means no proactive output.
    pub paused: std::sync::Arc<AtomicBool>,
    save_gen: AtomicU64,
}

#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Anchor {
    Droid,
    Center,
}

/// Logical work areas of all screens, primary first.
pub(crate) fn screens(app: &AppHandle) -> Vec<Rect> {
    let to_rect = |m: &tauri::Monitor| {
        let s = m.scale_factor();
        let wa = m.work_area();
        Rect { x: wa.position.x as f64 / s, y: wa.position.y as f64 / s, w: wa.size.width as f64 / s, h: wa.size.height as f64 / s }
    };
    let mut out = vec![];
    if let Ok(Some(p)) = app.primary_monitor() {
        out.push(to_rect(&p));
    }
    for m in app.available_monitors().unwrap_or_default() {
        let r = to_rect(&m);
        if !out.contains(&r) {
            out.push(r);
        }
    }
    if out.is_empty() {
        out.push(Rect { x: 0.0, y: 25.0, w: 1440.0, h: 875.0 });
    }
    out
}

pub(crate) fn rect_of(w: &WebviewWindow) -> Option<Rect> {
    let s = w.scale_factor().ok()?;
    let p = w.outer_position().ok()?.to_logical::<f64>(s);
    let z = w.outer_size().ok()?.to_logical::<f64>(s);
    Some(Rect { x: p.x, y: p.y, w: z.width, h: z.height })
}

pub(crate) fn screen_of(app: &AppHandle, r: Rect) -> Rect {
    let all = screens(app);
    all.iter().copied().find(|s| s.contains(r.x + r.w / 2.0, r.y + r.h / 2.0)).unwrap_or(all[0])
}

#[cfg(target_os = "macos")]
fn float_everywhere(w: &WebviewWindow) {
    use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
    if let Ok(ptr) = w.ns_window() {
        // SAFETY: Tauri hands back the live NSWindow for this webview window; called on the main thread.
        let ns: &NSWindow = unsafe { &*(ptr as *const NSWindow) };
        let b = ns.collectionBehavior()
            | NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::Stationary;
        ns.setCollectionBehavior(b);
    }
}
#[cfg(not(target_os = "macos"))]
fn float_everywhere(_w: &WebviewWindow) {}

pub fn create_windows(app: &AppHandle, cfg: &Config) -> tauri::Result<()> {
    let p = restore_pos(cfg.droid_pos, &screens(app), DROID);
    let droid = WebviewWindowBuilder::new(app, "droid", WebviewUrl::App("index.html#droid".into()))
        .title("bleep")
        .inner_size(DROID.w, DROID.h)
        .position(p.x, p.y)
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .focused(false)
        .focusable(false)
        .accept_first_mouse(true)
        .build()?;
    float_everywhere(&droid);
    let chat = WebviewWindowBuilder::new(app, "chat", WebviewUrl::App("index.html#chat".into()))
        .title("bleep")
        .inner_size(CHAT.w, CHAT.h)
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .visible(false)
        .effects(
            tauri::window::EffectsBuilder::new()
                .effect(tauri::window::Effect::Popover)
                .state(tauri::window::EffectState::Active)
                .radius(16.0)
                .build(),
        )
        .build()?;
    float_everywhere(&chat);
    for (label, size) in [("bar", BAR), ("bubble", BUBBLE)] {
        let w = WebviewWindowBuilder::new(app, label, WebviewUrl::App(format!("index.html#{label}").into()))
            .title("bleep")
            .inner_size(size.w, size.h)
            .transparent(true)
            .decorations(false)
            .shadow(false)
            .resizable(false)
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .skip_taskbar(true)
            .visible(false)
            .focused(false)
            .focusable(false)
            .accept_first_mouse(true)
            .build()?;
        float_everywhere(&w);
    }
    Ok(())
}

pub fn open_chat(app: &AppHandle, anchor: Anchor) {
    let Some(chat) = app.get_webview_window("chat") else { return };
    let droid = app.get_webview_window("droid").filter(|d| d.is_visible().unwrap_or(false)).and_then(|d| rect_of(&d));
    let p = match (anchor, droid) {
        (Anchor::Droid, Some(d)) => chat_pos(d, screen_of(app, d), CHAT),
        _ => center_pos(screens(app)[0], CHAT),
    };
    let _ = chat.set_position(LogicalPosition::new(p.x, p.y));
    let _ = chat.set_size(LogicalSize::new(CHAT.w, CHAT.h));
    let _ = chat.show();
    let _ = chat.set_focus();
}

/// Debounced save of the droid's spot after a move. Skipped while tucked.
pub fn on_droid_moved(app: &AppHandle) {
    let ui = app.state::<UiState>();
    let ticket = ui.save_gen.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let ui = app.state::<UiState>();
        if ui.save_gen.load(Ordering::SeqCst) != ticket || ui.tucked.lock().unwrap().is_some() {
            return;
        }
        let Some(r) = app.get_webview_window("droid").and_then(|d| rect_of(&d)) else { return };
        let state = app.state::<AppState>();
        let mut cfg = state.config.lock().unwrap();
        cfg.droid_pos = Some((r.x, r.y));
        if let Err(e) = cfg.save(&state.paths.config_file) {
            eprintln!("bleep: could not save droid position: {e}");
        }
    });
}

#[tauri::command]
pub fn chat_open(app: AppHandle, anchor: Anchor) {
    open_chat(&app, anchor);
}

#[tauri::command]
pub fn chat_hide(app: AppHandle) {
    if let Some(c) = app.get_webview_window("chat") {
        let _ = c.hide();
    }
}

#[tauri::command]
pub fn droid_toggle_tuck(app: AppHandle) {
    let Some(d) = app.get_webview_window("droid") else { return };
    let Some(r) = rect_of(&d) else { return };
    let ui = app.state::<UiState>();
    let mut tucked = ui.tucked.lock().unwrap();
    let to = match tucked.take() {
        Some(back) => back,
        None => {
            *tucked = Some(Point { x: r.x, y: r.y });
            tuck_pos(r, screen_of(&app, r))
        }
    };
    let _ = d.set_position(LogicalPosition::new(to.x, to.y));
}

#[tauri::command]
pub fn droid_set_visible(app: AppHandle, visible: bool) {
    if let Some(d) = app.get_webview_window("droid") {
        let _ = if visible { d.show() } else { d.hide() };
    }
}

pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, "toggle_droid", "Hide droid", true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open_chat", "Open chat", true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, "pause", "Pause bleep", true, false, None::<&str>)?;
    let login_on = {
        use tauri_plugin_autostart::ManagerExt;
        app.autolaunch().is_enabled().unwrap_or(false)
    };
    // A dev build must never register itself at login, so the item is greyed out there.
    let login = CheckMenuItem::with_id(app, "login", "Launch at login", crate::startup::IS_RELEASE, login_on, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit bleep", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &open, &pause, &login, &settings, &PredefinedMenuItem::separator(app)?, &quit])?;
    let mut tray = TrayIconBuilder::with_id("bleep").tooltip("bleep").menu(&menu).show_menu_on_left_click(true);
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.on_menu_event(move |app, ev| match ev.id().as_ref() {
        "toggle_droid" => {
            if let Some(d) = app.get_webview_window("droid") {
                let show = !d.is_visible().unwrap_or(true);
                droid_set_visible(app.clone(), show);
                let _ = toggle.set_text(if show { "Hide droid" } else { "Show droid" });
            }
        }
        "open_chat" => open_chat(app, Anchor::Droid),
        "pause" => {
            let ui = app.state::<UiState>();
            let now = !ui.paused.load(Ordering::SeqCst);
            ui.paused.store(now, Ordering::SeqCst);
            let _ = pause.set_checked(now);
        }
        "login" => {
            use tauri_plugin_autostart::ManagerExt;
            let al = app.autolaunch();
            let on = !al.is_enabled().unwrap_or(false);
            let res = if on { al.enable() } else { al.disable() };
            if let Err(e) = res {
                eprintln!("bleep: launch at login change failed: {e}");
            }
            let _ = login.set_checked(al.is_enabled().unwrap_or(false));
        }
        "settings" => crate::settings::open_settings(app),
        "quit" => app.exit(0),
        _ => {}
    })
    .build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect { x: 0.0, y: 25.0, w: 1440.0, h: 875.0 };
    const CHAT: Size = Size { w: 400.0, h: 600.0 };

    fn droid(x: f64, y: f64) -> Rect {
        Rect { x, y, w: 132.0, h: 132.0 }
    }

    #[test]
    fn bar_is_centered_under_the_menu_bar() {
        let p = bar_pos(SCREEN, BAR);
        assert_eq!(p, Point { x: (1440.0 - 560.0) / 2.0, y: 25.0 + 6.0 });
        let second = Rect { x: 1440.0, y: 0.0, w: 1920.0, h: 1055.0 };
        assert_eq!(bar_pos(second, BAR).x, 1440.0 + (1920.0 - 560.0) / 2.0);
    }

    #[test]
    fn bubble_sits_above_droid_or_below_at_top_edge() {
        let p = bubble_pos(droid(600.0, 600.0), SCREEN, BUBBLE);
        assert_eq!(p, Point { x: 600.0 + 66.0 - 120.0, y: 600.0 - GAP - 72.0 });
        let p = bubble_pos(droid(600.0, 40.0), SCREEN, BUBBLE);
        assert_eq!(p.y, 40.0 + 132.0 + GAP);
    }

    #[test]
    fn bubble_stays_on_screen_at_side_edges() {
        assert_eq!(bubble_pos(droid(0.0, 600.0), SCREEN, BUBBLE).x, MARGIN);
        assert_eq!(bubble_pos(droid(1440.0 - 132.0, 600.0), SCREEN, BUBBLE).x, 1440.0 - MARGIN - 240.0);
    }

    #[test]
    fn chat_goes_right_when_room() {
        let p = chat_pos(droid(100.0, 600.0), SCREEN, CHAT);
        assert_eq!(p.x, 100.0 + 132.0 + GAP);
    }

    #[test]
    fn chat_goes_left_at_right_edge() {
        let p = chat_pos(droid(1280.0, 600.0), SCREEN, CHAT);
        assert_eq!(p.x, 1280.0 - GAP - 400.0);
    }

    #[test]
    fn chat_clamped_to_screen() {
        let p = chat_pos(droid(100.0, 800.0), SCREEN, CHAT);
        assert!(p.y + CHAT.h <= SCREEN.y + SCREEN.h - MARGIN, "{p:?}");
        assert!(p.y >= SCREEN.y + MARGIN);
    }

    #[test]
    fn tuck_snaps_to_nearest_edge() {
        let left = tuck_pos(droid(200.0, 400.0), SCREEN);
        assert_eq!((left.x, left.y), (SCREEN.x - 132.0 + PEEK, 400.0));
        let right = tuck_pos(droid(1100.0, 400.0), SCREEN);
        assert_eq!(right.x, SCREEN.x + SCREEN.w - PEEK);
    }

    #[test]
    fn center_pos_centers() {
        let p = center_pos(SCREEN, CHAT);
        assert_eq!((p.x, p.y), (520.0, 25.0 + 137.5));
    }

    #[test]
    fn offscreen_pos_resets() {
        let screens = [SCREEN];
        assert_eq!(restore_pos(Some((5000.0, 5000.0)), &screens, Size { w: 132.0, h: 132.0 }), default_droid_pos(SCREEN, Size { w: 132.0, h: 132.0 }));
        assert_eq!(restore_pos(None, &screens, Size { w: 132.0, h: 132.0 }), default_droid_pos(SCREEN, Size { w: 132.0, h: 132.0 }));
        let kept = restore_pos(Some((300.0, 300.0)), &screens, Size { w: 132.0, h: 132.0 });
        assert_eq!((kept.x, kept.y), (300.0, 300.0));
    }
}
