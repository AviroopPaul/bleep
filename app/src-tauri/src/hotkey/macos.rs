//! A listen-only CGEventTap for the Option double tap. Needs Input Monitoring.
use super::detector::{KeyEv, OptionTapDetector};
use std::ffi::c_void;
use std::sync::Mutex;
use std::time::Instant;

pub const KEY_DOWN: u32 = 10;
pub const FLAGS_CHANGED: u32 = 12;
const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
const FLAG_ALT: u64 = 0x0008_0000;

/// One raw event to a detector event, plus the new Option state.
pub fn classify(ev_type: u32, flags: u64, prev_alt: bool) -> (Option<KeyEv>, bool) {
    let alt = flags & FLAG_ALT != 0;
    match ev_type {
        KEY_DOWN => (Some(KeyEv::OtherKey), prev_alt),
        FLAGS_CHANGED if alt && !prev_alt => (Some(KeyEv::OptionDown), true),
        FLAGS_CHANGED if !alt && prev_alt => (Some(KeyEv::OptionUp), false),
        FLAGS_CHANGED => (Some(KeyEv::OtherKey), alt),
        _ => (None, prev_alt),
    }
}

type Ref = *mut c_void;
type TapCallback = extern "C" fn(Ref, u32, Ref, *mut c_void) -> Ref;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGEventTapCreate(tap: u32, place: u32, options: u32, mask: u64, callback: TapCallback, user_info: *mut c_void) -> Ref;
    fn CGEventTapEnable(tap: Ref, enable: bool);
    fn CGEventGetFlags(event: Ref) -> u64;
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFMachPortCreateRunLoopSource(alloc: *const c_void, port: Ref, order: isize) -> Ref;
    fn CFRunLoopGetCurrent() -> Ref;
    fn CFRunLoopAddSource(rl: Ref, source: Ref, mode: *const c_void);
    fn CFRunLoopRun();
    static kCFRunLoopCommonModes: *const c_void;
}

pub fn listen_input_access() -> bool {
    unsafe { CGPreflightListenEventAccess() }
}

/// Shows the system prompt once while the choice is undecided; returns the current grant.
pub fn request_input_access() -> bool {
    unsafe { CGRequestListenEventAccess() }
}

struct Ctx {
    detector: Mutex<(OptionTapDetector, bool)>,
    start: Instant,
    tap: Mutex<usize>,
    on_hit: Box<dyn Fn() + Send + Sync>,
}

extern "C" fn callback(_proxy: Ref, ev_type: u32, event: Ref, user: *mut c_void) -> Ref {
    // SAFETY: `user` is the leaked Ctx from start_option_tap; it lives for the whole process.
    let ctx = unsafe { &*(user as *const Ctx) };
    if ev_type == TAP_DISABLED_BY_TIMEOUT || ev_type == TAP_DISABLED_BY_USER_INPUT {
        let tap = *ctx.tap.lock().unwrap();
        unsafe { CGEventTapEnable(tap as Ref, true) };
        return event;
    }
    let flags = unsafe { CGEventGetFlags(event) };
    let hit = {
        let mut g = ctx.detector.lock().unwrap();
        let (ev, alt) = classify(ev_type, flags, g.1);
        g.1 = alt;
        ev.is_some_and(|e| g.0.on(e, ctx.start.elapsed().as_millis() as u64))
    };
    if hit {
        (ctx.on_hit)();
    }
    event
}

/// Starts the tap on its own thread with its own run loop. Fails if access is not granted.
pub fn start_option_tap(on_hit: impl Fn() + Send + Sync + 'static) -> anyhow::Result<()> {
    if !listen_input_access() {
        anyhow::bail!("Input Monitoring not granted");
    }
    let ctx: &'static Ctx = Box::leak(Box::new(Ctx {
        detector: Mutex::new((OptionTapDetector::default(), false)),
        start: Instant::now(),
        tap: Mutex::new(0),
        on_hit: Box::new(on_hit),
    }));
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new().name("bleep-option-tap".into()).spawn(move || unsafe {
        let mask = (1u64 << KEY_DOWN) | (1u64 << FLAGS_CHANGED);
        // session tap, head insert, listen only
        let tap = CGEventTapCreate(1, 0, 1, mask, callback, ctx as *const Ctx as *mut c_void);
        if tap.is_null() {
            let _ = tx.send(false);
            return;
        }
        *ctx.tap.lock().unwrap() = tap as usize;
        let src = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
        CFRunLoopAddSource(CFRunLoopGetCurrent(), src, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);
        let _ = tx.send(true);
        CFRunLoopRun();
    })?;
    if rx.recv().unwrap_or(false) { Ok(()) } else { anyhow::bail!("could not create the event tap") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hotkey::detector::KeyEv;

    const ALT: u64 = 0x0008_0000;
    const SHIFT: u64 = 0x0002_0000;

    #[test]
    fn alt_press_and_release() {
        assert_eq!(classify(FLAGS_CHANGED, ALT, false), (Some(KeyEv::OptionDown), true));
        assert_eq!(classify(FLAGS_CHANGED, 0, true), (Some(KeyEv::OptionUp), false));
    }

    #[test]
    fn other_modifier_is_other_key() {
        assert_eq!(classify(FLAGS_CHANGED, ALT | SHIFT, true), (Some(KeyEv::OtherKey), true));
        assert_eq!(classify(FLAGS_CHANGED, SHIFT, false), (Some(KeyEv::OtherKey), false));
    }

    #[test]
    fn key_down_is_other_key() {
        assert_eq!(classify(KEY_DOWN, ALT, true), (Some(KeyEv::OtherKey), true));
    }

    #[test]
    fn unknown_event_is_ignored() {
        assert_eq!(classify(99, 0, false), (None, false));
    }
}
