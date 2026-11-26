//! The mouse on Linux and macOS (Windows: ../deskmouse.rs).
//!
//! * "Search from anywhere" bound to a mouse button (middle, back, forward, with or without
//!   Ctrl / Alt / Shift / Super).
//!   - Linux: a passive grab of that button on the X11 root window (the press is taken, like on
//!     Windows).
//!   - macOS: an event monitor (the press also reaches the app under the mouse).
//! * Wallpaper mode forwarding. Linux: not needed, the skin takes the mouse itself (see desktop.rs).
//!   macOS: off since 1.0.21 (see FORWARD): with "Hide desktop icons" off the icons alone take the
//!   mouse; with it on the skin covers Finder's icons and takes the mouse itself.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use tauri::{AppHandle, Emitter};

// "Search from anywhere" bound to a mouse button (1 = middle, 2 = X1/back, 3 = X2/forward; 0 = none)
static BIND_BTN: AtomicU32 = AtomicU32::new(0);
static BIND_MODS: AtomicU32 = AtomicU32::new(0); // 1 ctrl · 2 alt · 4 shift · 8 super
static CAPTURE: AtomicBool = AtomicBool::new(false);
/// macOS: send the desktop's mouse to the skin under Finder's icons (off: the icons keep it)
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const FORWARD: bool = false;

pub fn set_bind(btn: u32, mods: u32) {
    BIND_BTN.store(btn, Ordering::Relaxed);
    BIND_MODS.store(mods, Ordering::Relaxed);
    #[cfg(target_os = "linux")]
    linux::regrab();
}

pub fn set_capture(on: bool) {
    CAPTURE.store(on, Ordering::Relaxed);
    #[cfg(target_os = "linux")]
    linux::regrab();
}

/// A bindable button was pressed (btn 1/2/3, mods as above): search, or report the new binding.
fn pressed(app: &AppHandle, btn: u32, mods: u32) -> bool {
    if CAPTURE.swap(false, Ordering::Relaxed) {
        let mut spec = String::new();
        for (bit, name) in [(1u32, "Ctrl+"), (2, "Alt+"), (4, "Shift+"), (8, "Super+")] {
            if mods & bit != 0 {
                spec.push_str(name);
            }
        }
        spec.push_str(match btn {
            1 => "Mouse:Middle",
            2 => "Mouse:X1",
            _ => "Mouse:X2",
        });
        let _ = app.emit("bind-captured", spec);
        #[cfg(target_os = "linux")]
        linux::regrab();
        return true;
    }
    if btn != 0 && BIND_BTN.load(Ordering::Relaxed) == btn && BIND_MODS.load(Ordering::Relaxed) == mods {
        let app = app.clone();
        std::thread::spawn(move || crate::open_search(&app));
        return true;
    }
    false
}

pub fn start(app: AppHandle) {
    #[cfg(target_os = "linux")]
    linux::start(app);
    #[cfg(target_os = "macos")]
    mac::start(app);
}

// ---------------------------------------------------------------- Linux (X11)

#[cfg(target_os = "linux")]
mod linux {
    use std::sync::OnceLock;

    use tauri::AppHandle;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ButtonIndex, ConnectionExt as _, EventMask, GrabMode, ModMask, Window};
    use x11rb::protocol::Event;
    use x11rb::rust_connection::RustConnection;

    use super::{pressed, BIND_BTN, BIND_MODS, CAPTURE};
    use std::sync::atomic::Ordering;

    /// A connection of our own: grabs belong to the connection that waits for their events.
    static CONN: OnceLock<Option<(RustConnection, Window)>> = OnceLock::new();

    fn conn() -> Option<&'static (RustConnection, Window)> {
        CONN.get_or_init(|| {
            let (c, n) = x11rb::connect(None).ok()?;
            let root = c.setup().roots.get(n)?.root;
            Some((c, root))
        })
        .as_ref()
    }

    /// X buttons: 2 middle, 8 back, 9 forward.
    fn x_button(btn: u32) -> Option<u8> {
        match btn {
            1 => Some(2),
            2 => Some(8),
            3 => Some(9),
            _ => None,
        }
    }

    fn x_mods(mods: u32) -> u16 {
        let mut m = 0u16;
        if mods & 1 != 0 {
            m |= u16::from(ModMask::CONTROL);
        }
        if mods & 2 != 0 {
            m |= u16::from(ModMask::M1);
        }
        if mods & 4 != 0 {
            m |= u16::from(ModMask::SHIFT);
        }
        if mods & 8 != 0 {
            m |= u16::from(ModMask::M4);
        }
        m
    }

    fn our_mods(state: u16) -> u32 {
        let has = |m: ModMask| state & u16::from(m) != 0;
        (has(ModMask::CONTROL) as u32) | ((has(ModMask::M1) as u32) << 1) | ((has(ModMask::SHIFT) as u32) << 2) | ((has(ModMask::M4) as u32) << 3)
    }

    /// Grab what is needed now: every bindable button while a new binding is being recorded, else the
    /// bound button with its modifiers (also with Num Lock / Caps Lock on).
    pub fn regrab() {
        let Some((c, root)) = conn() else { return };
        let _ = c.ungrab_button(ButtonIndex::ANY, *root, ModMask::ANY);
        let mask = EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE;
        let grab = |b: u8, m: ModMask| {
            let _ = c.grab_button(false, *root, mask, GrabMode::ASYNC, GrabMode::ASYNC, x11rb::NONE, x11rb::NONE, ButtonIndex::from(b), m);
        };
        if CAPTURE.load(Ordering::Relaxed) {
            for b in [2u8, 8, 9] {
                grab(b, ModMask::ANY);
            }
        } else if let Some(b) = x_button(BIND_BTN.load(Ordering::Relaxed)) {
            let m = x_mods(BIND_MODS.load(Ordering::Relaxed));
            for extra in [0u16, u16::from(ModMask::M2), u16::from(ModMask::LOCK), u16::from(ModMask::M2) | u16::from(ModMask::LOCK)] {
                grab(b, ModMask::from(m | extra));
            }
        }
        let _ = c.flush();
    }

    pub fn start(app: AppHandle) {
        if conn().is_none() {
            return;
        }
        regrab();
        std::thread::spawn(move || {
            let Some((c, _)) = conn() else { return };
            while let Ok(ev) = c.wait_for_event() {
                if let Event::ButtonPress(e) = ev {
                    let btn = match e.detail {
                        2 => 1,
                        8 => 2,
                        9 => 3,
                        _ => 0,
                    };
                    pressed(&app, btn, our_mods(u16::from(e.state)));
                }
            }
        });
    }
}

// ---------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
mod mac {
    use std::ptr::NonNull;
    use std::sync::atomic::Ordering;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use block2::RcBlock;
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSEventType, NSScreen};
    use objc2_foundation::MainThreadMarker;
    use serde_json::json;
    use tauri::{AppHandle, Emitter};

    use super::pressed;
    use crate::desktop;

    /// the window holding a press (label, "" = main skin) and the one the mouse hovers
    static DOWN: Mutex<Option<String>> = Mutex::new(None);
    static OVER: Mutex<Option<String>> = Mutex::new(None);
    static LAST_MOVE: Mutex<Option<Instant>> = Mutex::new(None);
    /// (when, app windows) — the window list is asked at most 4 times a second
    static APPS: Mutex<Option<(Instant, Vec<crate::mac::CGRect>)>> = Mutex::new(None);
    static SCALE: Mutex<f64> = Mutex::new(1.0);

    fn mods(f: NSEventModifierFlags) -> u32 {
        (f.contains(NSEventModifierFlags::Control) as u32)
            | ((f.contains(NSEventModifierFlags::Option) as u32) << 1)
            | ((f.contains(NSEventModifierFlags::Shift) as u32) << 2)
            | ((f.contains(NSEventModifierFlags::Command) as u32) << 3)
    }

    fn button(n: isize) -> u32 {
        match n {
            2 => 1,
            3 => 2,
            4 => 3,
            _ => 0,
        }
    }

    /// The mouse (CoreGraphics points, top-left origin) is over the bare desktop: no app window,
    /// menu bar or Dock there.
    fn over_desktop(x: f64, y: f64) -> bool {
        let mut g = APPS.lock().unwrap_or_else(|e| e.into_inner());
        let fresh = g.as_ref().map(|(t, _)| t.elapsed() < Duration::from_millis(250)).unwrap_or(false);
        if !fresh {
            let me = std::process::id() as i64;
            let list = crate::mac::windows().into_iter().filter(|w| w.layer >= 0 && w.pid != me && w.alpha > 0.0).map(|w| w.bounds).collect();
            *g = Some((Instant::now(), list));
        }
        let wins = &g.as_ref().unwrap().1;
        !wins.iter().any(|r| x >= r.origin.x && x < r.origin.x + r.size.width && y >= r.origin.y && y < r.origin.y + r.size.height)
    }

    fn forward(app: &AppHandle, k: &str, d: i32) {
        // Since 1.0.21 the desktop icons shown means the icons alone take the mouse; hidden, the skin
        // sits over Finder's icons and takes it itself. So nothing is forwarded any more.
        if !FORWARD || !desktop::is_wallpaper() || !desktop::behind_icons() {
            return;
        }
        let p = NSEvent::mouseLocation();
        let (x, y) = (p.x, crate::mac::main_height() - p.y);
        let (mx, my, sw, sh) = desktop::target_rect();
        let inside = x >= mx as f64 && x < (mx + sw) as f64 && y >= my as f64 && y < (my + sh) as f64;
        let s = *SCALE.lock().unwrap_or_else(|e| e.into_inner());
        // the window under the mouse: the main skin ("": pixels, the page divides by its devicePixelRatio
        // like the pixels Windows sends) or the mirror on another display (points = CSS pixels, "c")
        let at: Option<(String, i32, i32, bool)> = if inside {
            Some((String::new(), ((x - mx as f64) * s).round() as i32, ((y - my as f64) * s).round() as i32, false))
        } else {
            crate::mirror::target_at(x.round() as i32, y.round() as i32).map(|(l, lx, ly)| (l, lx, ly, true))
        };
        let emit = |to: &str, k: &str, px: i32, py: i32, css: bool| {
            let ev = if to.is_empty() { "desk-mouse".to_string() } else { format!("desk-mouse-{to}") };
            let _ = app.emit(&ev, json!({"k": k, "x": px, "y": py, "d": d, "c": css}));
        };
        let held = DOWN.lock().unwrap_or_else(|e| e.into_inner()).clone();
        match k {
            "move" => {
                if let Some(to) = held {
                    // a drag stays with the window it started on (coordinates may run past its edge)
                    if let Some((t, px, py, css)) = at.clone().filter(|a| a.0 == to) {
                        emit(&t, "move", px, py, css);
                    }
                } else if let Some((to, px, py, css)) = at.filter(|_| over_desktop(x, y)) {
                    let mut over = OVER.lock().unwrap_or_else(|e| e.into_inner());
                    if over.as_deref() != Some(to.as_str()) {
                        if let Some(old) = over.replace(to.clone()) {
                            emit(&old, "leave", 0, 0, false);
                        }
                    }
                    let mut last = LAST_MOVE.lock().unwrap_or_else(|e| e.into_inner());
                    if last.map(|t| t.elapsed() >= Duration::from_millis(14)).unwrap_or(true) {
                        *last = Some(Instant::now());
                        emit(&to, "move", px, py, css);
                    }
                } else if let Some(old) = OVER.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    emit(&old, "leave", 0, 0, false);
                }
            }
            "down" => {
                if let Some((to, px, py, css)) = at.filter(|_| over_desktop(x, y)) {
                    *DOWN.lock().unwrap_or_else(|e| e.into_inner()) = Some(to.clone());
                    emit(&to, "down", px, py, css);
                }
            }
            "up" => {
                if let Some(to) = DOWN.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    let (px, py, css) = at.filter(|a| a.0 == to).map(|a| (a.1, a.2, a.3)).unwrap_or((-1, -1, to.as_str() != ""));
                    emit(&to, "up", px, py, css);
                }
            }
            "wheel" => {
                if let Some((to, px, py, css)) = at.filter(|_| over_desktop(x, y)) {
                    emit(&to, "wheel", px, py, css);
                }
            }
            _ => {}
        }
    }

    /// Main thread (called from the app's setup).
    pub fn start(app: AppHandle) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        if let Some(s) = NSScreen::mainScreen(mtm) {
            *SCALE.lock().unwrap() = s.backingScaleFactor();
        }
        let a1 = app.clone();
        let global = RcBlock::new(move |ev: NonNull<NSEvent>| {
            let ev = unsafe { ev.as_ref() };
            match ev.r#type() {
                NSEventType::OtherMouseDown => {
                    pressed(&a1, button(ev.buttonNumber()), mods(ev.modifierFlags()));
                }
                NSEventType::MouseMoved | NSEventType::LeftMouseDragged => forward(&a1, "move", 0),
                NSEventType::LeftMouseDown => forward(&a1, "down", 0),
                NSEventType::LeftMouseUp => forward(&a1, "up", 0),
                NSEventType::ScrollWheel => {
                    let dy = ev.scrollingDeltaY();
                    let d = if ev.hasPreciseScrollingDeltas() { dy } else { dy * 40.0 };
                    forward(&a1, "wheel", d.round() as i32);
                }
                _ => {}
            }
        });
        let mask = NSEventMask::OtherMouseDown
            | NSEventMask::MouseMoved
            | NSEventMask::LeftMouseDragged
            | NSEventMask::LeftMouseDown
            | NSEventMask::LeftMouseUp
            | NSEventMask::ScrollWheel;
        if let Some(token) = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &global) {
            std::mem::forget(token); // for the whole life of the app
        }
        std::mem::forget(global);
        // the same buttons pressed over our own windows (the global monitor sees other apps' events only)
        let a2 = app.clone();
        let local = RcBlock::new(move |ev: NonNull<NSEvent>| -> *mut NSEvent {
            let e = unsafe { ev.as_ref() };
            if pressed(&a2, button(e.buttonNumber()), mods(e.modifierFlags())) {
                return std::ptr::null_mut(); // taken
            }
            ev.as_ptr()
        });
        if let Some(token) = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::OtherMouseDown, &local) } {
            std::mem::forget(token);
        }
        std::mem::forget(local);
    }
}
