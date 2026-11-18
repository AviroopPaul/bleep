//! The droid window, the chat window, the tray, and where they sit on screen.
use crate::config::Config;
use crate::AppState;
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

pub const DROID: Size = Size { w: 132.0, h: 132.0 };
pub const CHAT: Size = Size { w: 400.0, h: 600.0 };
/// The enlarged chat. Shrunk to fit a small screen.
pub const CHAT_BIG: Size = Size { w: 760.0, h: 860.0 };
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

/// Where a panel window (Settings, Setup) opens: centered on the screen that holds the
/// droid, or the main screen when there is no droid on any screen. Shrunk to fit.
pub fn panel_place(droid: Option<Rect>, screens: &[Rect], want: Size) -> (Point, Size) {
    let on = droid.and_then(|d| screens.iter().copied().find(|s| s.contains(d.x + d.w / 2.0, d.y + d.h / 2.0)));
    let screen = on.or_else(|| screens.first().copied()).unwrap_or(Rect { x: 0.0, y: 25.0, w: 1440.0, h: 875.0 });
    let size = Size { w: want.w.min(screen.w - 2.0 * MARGIN), h: want.h.min(screen.h - 2.0 * MARGIN) };
    (center_pos(screen, size), size)
}

/// `panel_place` for the running app.
pub(crate) fn panel_on_droid_screen(app: &AppHandle, want: Size) -> (Point, Size) {
    let droid = app.get_webview_window("droid").and_then(|w| rect_of(&w));
    panel_place(droid, &screens(app), want)
}

/// Moves an open panel to the droid's screen when it sits on another one.
pub(crate) fn bring_to_droid_screen(app: &AppHandle, w: &WebviewWindow) {
    let (Some(droid), Some(r)) = (app.get_webview_window("droid").and_then(|d| rect_of(&d)), rect_of(w)) else { return };
    if screen_of(app, droid) != screen_of(app, r) {
        let (p, _) = panel_place(Some(droid), &screens(app), Size { w: r.w, h: r.h });
        let _ = w.set_position(tauri::LogicalPosition::new(p.x, p.y));
    }
}

pub fn default_droid_pos(screen: Rect, droid: Size) -> Point {
    Point { x: screen.x + screen.w - droid.w - 24.0, y: screen.y + screen.h - droid.h - 24.0 }
}

/// One screen: the whole monitor (menu bar and Dock included) and its work area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    pub full: Rect,
    pub work: Rect,
}

/// The work area the droid should move to: the screen under the cursor, when the droid
/// sits on another screen. None when the droid is already there or the cursor is off screen.
pub fn follow_target(droid: Rect, cursor: Point, displays: &[Display]) -> Option<Rect> {
    let active = displays.iter().find(|d| d.full.contains(cursor.x, cursor.y))?;
    (!active.full.contains(droid.x + droid.w / 2.0, droid.y + droid.h / 2.0)).then_some(active.work)
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
    /// The chat is enlarged. Only the small chat closes when it loses focus.
    pub chat_big: AtomicBool,
    save_gen: AtomicU64,
    /// When the droid last moved. A recent move means a drag may be under way.
    moved_at: Mutex<Option<std::time::Instant>>,
}

#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Anchor {
    Droid,
    Center,
}

/// All screens in logical points, primary first.
pub(crate) fn displays(app: &AppHandle) -> Vec<Display> {
    let to_display = |m: &tauri::Monitor| {
        let s = m.scale_factor();
        let wa = m.work_area();
        let (p, z) = (m.position(), m.size());
        Display {
            full: Rect { x: p.x as f64 / s, y: p.y as f64 / s, w: z.width as f64 / s, h: z.height as f64 / s },
            work: Rect { x: wa.position.x as f64 / s, y: wa.position.y as f64 / s, w: wa.size.width as f64 / s, h: wa.size.height as f64 / s },
        }
    };
    let mut out = vec![];
    if let Ok(Some(p)) = app.primary_monitor() {
        out.push(to_display(&p));
    }
    for m in app.available_monitors().unwrap_or_default() {
        let d = to_display(&m);
        if !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

/// Logical work areas of all screens, primary first.
pub(crate) fn screens(app: &AppHandle) -> Vec<Rect> {
    let out: Vec<Rect> = displays(app).iter().map(|d| d.work).collect();
    if out.is_empty() {
        return vec![Rect { x: 0.0, y: 25.0, w: 1440.0, h: 875.0 }];
    }
    out
}

/// The cursor in logical points. macOS reports it scaled by the main screen's factor.
fn cursor_point(app: &AppHandle) -> Option<Point> {
    let c = app.cursor_position().ok()?;
    let s = app.primary_monitor().ok()??.scale_factor();
    Some(Point { x: c.x / s, y: c.y / s })
}

/// Moves the droid to the bottom right of the screen the user works on (the one under the
/// cursor). The cursor must stay on that screen for two checks, so a quick pass does not
/// count. Nothing moves while the chat is open, the droid is hidden, or it is being dragged.
pub fn follow_active_screen(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut pending: Option<Rect> = None;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let target = active_screen_target(&app);
            match (target, pending) {
                (Some(t), Some(p)) if t == p => {
                    move_droid_to(&app, t);
                    pending = None;
                }
                _ => pending = target,
            }
        }
    });
}

fn active_screen_target(app: &AppHandle) -> Option<Rect> {
    let droid = app.get_webview_window("droid")?;
    if !droid.is_visible().unwrap_or(false) || app.get_webview_window("chat").is_some_and(|c| c.is_visible().unwrap_or(false)) {
        return None;
    }
    let ui = app.state::<UiState>();
    if ui.moved_at.lock().unwrap().is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(1500)) {
        return None;
    }
    let r = rect_of(&droid)?;
    // A tucked droid is judged by the spot it came from, since it hangs off the screen edge.
    let home = match *ui.tucked.lock().unwrap() {
        Some(back) => Rect { x: back.x, y: back.y, w: r.w, h: r.h },
        None => r,
    };
    follow_target(home, cursor_point(app)?, &displays(app))
}

fn move_droid_to(app: &AppHandle, work: Rect) {
    let Some(d) = app.get_webview_window("droid") else { return };
    let home = default_droid_pos(work, DROID);
    let ui = app.state::<UiState>();
    let to = {
        let mut tucked = ui.tucked.lock().unwrap();
        match tucked.as_mut() {
            Some(back) => {
                *back = home;
                tuck_pos(Rect { x: home.x, y: home.y, w: DROID.w, h: DROID.h }, work)
            }
            None => home,
        }
    };
    let _ = d.set_position(LogicalPosition::new(to.x, to.y));
    if let Some(b) = app.get_webview_window("bubble").filter(|b| b.is_visible().unwrap_or(false)) {
        let p = bubble_pos(Rect { x: to.x, y: to.y, w: DROID.w, h: DROID.h }, work, BUBBLE);
        let _ = b.set_position(LogicalPosition::new(p.x, p.y));
    }
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

/// Where the small chat goes: next to the droid, or centered on the main screen.
fn small_chat_pos(app: &AppHandle, anchor: Anchor) -> Point {
    let droid = app.get_webview_window("droid").filter(|d| d.is_visible().unwrap_or(false)).and_then(|d| rect_of(&d));
    match (anchor, droid) {
        (Anchor::Droid, Some(d)) => chat_pos(d, screen_of(app, d), CHAT),
        _ => center_pos(screens(app)[0], CHAT),
    }
}

fn set_chat_big(app: &AppHandle, on: bool) {
    app.state::<UiState>().chat_big.store(on, Ordering::SeqCst);
    let _ = app.emit("chat://expanded", on);
}

pub fn open_chat(app: &AppHandle, anchor: Anchor) {
    let Some(chat) = app.get_webview_window("chat") else { return };
    set_chat_big(app, false);
    let _ = chat.set_resizable(false);
    let p = small_chat_pos(app, anchor);
    let _ = chat.set_position(LogicalPosition::new(p.x, p.y));
    let _ = chat.set_size(LogicalSize::new(CHAT.w, CHAT.h));
    let _ = chat.show();
    let _ = chat.set_focus();
    // The bubble's line is in the chat now, so the bubble can go.
    if let Some(b) = app.get_webview_window("bubble") {
        let _ = b.hide();
    }
}

/// Debounced save of the droid's spot after a move. Skipped while tucked.
pub fn on_droid_moved(app: &AppHandle) {
    let ui = app.state::<UiState>();
    *ui.moved_at.lock().unwrap() = Some(std::time::Instant::now());
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

/// Enlarge the chat (centered on its screen) or shrink it back next to the droid.
#[tauri::command]
pub fn chat_set_expanded(app: AppHandle, expanded: bool) {
    let Some(chat) = app.get_webview_window("chat") else { return };
    let (p, s) = if expanded {
        panel_place(rect_of(&chat), &screens(&app), CHAT_BIG)
    } else {
        (small_chat_pos(&app, Anchor::Droid), CHAT)
    };
    // Only the big chat can be resized, and never smaller than the small one.
    let _ = chat.set_resizable(expanded);
    let _ = chat.set_min_size(expanded.then(|| LogicalSize::new(CHAT.w, CHAT.h)));
    let _ = chat.set_size(LogicalSize::new(s.w, s.h));
    let _ = chat.set_position(LogicalPosition::new(p.x, p.y));
    set_chat_big(&app, expanded);
}

/// The small chat closes when the user clicks away from it. The enlarged one stays.
pub fn on_chat_blur(app: &AppHandle) {
    if app.state::<UiState>().chat_big.load(Ordering::SeqCst) {
        return;
    }
    if let Some(c) = app.get_webview_window("chat") {
        let _ = c.hide();
    }
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
    let toggle = MenuItem::with_id(app, "toggle_droid", "Hide buddy", true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open_chat", "Open chat", true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, "pause", "Pause bleep", true, false, None::<&str>)?;
    let login_on = {
        use tauri_plugin_autostart::ManagerExt;
        app.autolaunch().is_enabled().unwrap_or(false)
    };
    // A dev build must never register itself at login, so the item is greyed out there.
    let login = CheckMenuItem::with_id(app, "login", "Launch at login", crate::startup::IS_RELEASE, login_on, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let setup = MenuItem::with_id(app, "setup", "Set up bleep…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit bleep", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &open, &pause, &login, &settings, &setup, &PredefinedMenuItem::separator(app)?, &quit])?;
    // The menu bar shows the chosen buddy as an outline. Sources: icons/tray.svg, icons/tray/*.svg.
    // As a template image, macOS tints it to match a light or dark menu bar.
    let tray = TrayIconBuilder::with_id("bleep")
        .tooltip("bleep")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .icon(crate::avatar::tray_icon(&crate::avatar::current(app).kind))
        .icon_as_template(true);
    tray.on_menu_event(move |app, ev| match ev.id().as_ref() {
        "toggle_droid" => {
            if let Some(d) = app.get_webview_window("droid") {
                let show = !d.is_visible().unwrap_or(true);
                droid_set_visible(app.clone(), show);
                let _ = toggle.set_text(if show { "Hide buddy" } else { "Show buddy" });
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
        "setup" => crate::setup::open_setup(app),
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
    fn panels_open_on_the_droids_screen() {
        let laptop = Rect { x: 0.0, y: 25.0, w: 1440.0, h: 875.0 };
        let monitor = Rect { x: 1440.0, y: 0.0, w: 2560.0, h: 1415.0 };
        let want = Size { w: 860.0, h: 620.0 };
        let droid = Rect { x: 3500.0, y: 1200.0, w: 132.0, h: 132.0 };
        let (p, s) = panel_place(Some(droid), &[laptop, monitor], want);
        assert_eq!((s.w, s.h), (860.0, 620.0));
        assert_eq!((p.x, p.y), (1440.0 + (2560.0 - 860.0) / 2.0, (1415.0 - 620.0) / 2.0), "centered on the monitor");
        let (p, _) = panel_place(None, &[laptop, monitor], want);
        assert!(laptop.contains(p.x + 430.0, p.y + 310.0), "no droid: the main screen");
        let off = Rect { x: 9000.0, y: 9000.0, w: 132.0, h: 132.0 };
        let (p, _) = panel_place(Some(off), &[laptop, monitor], want);
        assert!(laptop.contains(p.x + 430.0, p.y + 310.0), "droid off every screen: the main screen");
    }

    #[test]
    fn panels_shrink_to_fit_a_small_screen() {
        let small = Rect { x: 0.0, y: 25.0, w: 800.0, h: 560.0 };
        let (p, s) = panel_place(None, &[small], Size { w: 860.0, h: 620.0 });
        assert_eq!((s.w, s.h), (800.0 - 2.0 * MARGIN, 560.0 - 2.0 * MARGIN));
        assert_eq!((p.x, p.y), (MARGIN, 25.0 + MARGIN));
    }

    #[test]
    fn big_chat_centers_on_the_chats_screen() {
        let laptop = Rect { x: 0.0, y: 25.0, w: 1440.0, h: 875.0 };
        let monitor = Rect { x: 1440.0, y: 0.0, w: 2560.0, h: 1415.0 };
        let chat = Rect { x: 3000.0, y: 700.0, w: 400.0, h: 600.0 };
        let (p, s) = panel_place(Some(chat), &[laptop, monitor], CHAT_BIG);
        assert_eq!((s.w, s.h), (CHAT_BIG.w, CHAT_BIG.h));
        assert!(monitor.contains(p.x + s.w / 2.0, p.y + s.h / 2.0));
        let chat = Rect { x: 600.0, y: 300.0, w: 400.0, h: 600.0 };
        let (p, s) = panel_place(Some(chat), &[laptop, monitor], CHAT_BIG);
        assert_eq!(s.h, 875.0 - 2.0 * MARGIN, "shrunk to the laptop screen");
        assert_eq!(p.y, 25.0 + MARGIN);
    }

    #[test]
    fn droid_follows_the_screen_under_the_cursor() {
        let laptop = Display { full: Rect { x: 0.0, y: 0.0, w: 1440.0, h: 900.0 }, work: Rect { x: 0.0, y: 25.0, w: 1440.0, h: 875.0 } };
        let monitor = Display { full: Rect { x: 1440.0, y: 0.0, w: 2560.0, h: 1440.0 }, work: Rect { x: 1440.0, y: 25.0, w: 2560.0, h: 1415.0 } };
        let all = [laptop, monitor];
        let on_laptop = droid(1284.0, 744.0);
        // Cursor on the other screen: go there.
        assert_eq!(follow_target(on_laptop, Point { x: 2000.0, y: 500.0 }, &all), Some(monitor.work));
        // Cursor on the droid's own screen, menu bar included: stay.
        assert_eq!(follow_target(on_laptop, Point { x: 300.0, y: 10.0 }, &all), None);
        // Cursor off every screen: stay.
        assert_eq!(follow_target(on_laptop, Point { x: -50.0, y: -50.0 }, &all), None);
        // The new spot is the bottom right of the monitor's work area.
        let p = default_droid_pos(monitor.work, Size { w: 132.0, h: 132.0 });
        assert_eq!((p.x, p.y), (1440.0 + 2560.0 - 132.0 - 24.0, 25.0 + 1415.0 - 132.0 - 24.0));
        assert_eq!(follow_target(droid(p.x, p.y), Point { x: 2000.0, y: 500.0 }, &all), None);
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
