//! Window placement on the Linux and macOS desktop (Windows: ../desktop.rs, same functions).
//!
//! Three modes, as on Windows:
//! * `Wallpaper` – the skin is part of the desktop: under every app window, on every workspace /
//!                 Space, and it stays when you show the desktop.
//!   - Linux: an X11 window of type DESKTOP (what file managers draw their desktop with). The window
//!     manager keeps it in the desktop layer, below all apps. It covers the desktop icons of the file
//!     manager, if there are any, and takes the mouse itself (no forwarding needed).
//!   - macOS: a window at the desktop picture's level, just under Finder's icons (they stay usable)
//!     — or, with "Hide desktop icons" on, just above the icons, which it then covers and so takes the
//!     mouse itself.
//! * `Desktop`   – Linux: a normal window kept below the others. macOS: just above the desktop icons.
//! * `Peek`      – raised above everything and focused (the "Search from anywhere" shortcut).
//!
//! The window handle Windows calls HWND is, here, a small number this module hands out
//! (`raw_of`) and turns back into the Tauri window.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewWindow};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Desktop = 0,
    Wallpaper = 1,
    Peek = 2,
}

impl Mode {
    fn from_u8(v: u8) -> Mode {
        match v {
            1 => Mode::Wallpaper,
            2 => Mode::Peek,
            _ => Mode::Desktop,
        }
    }
}

static MODE: AtomicU8 = AtomicU8::new(Mode::Desktop as u8);
/// "Hide desktop icons" (macOS: the skin goes over Finder's icons; Linux: the skin always covers them).
static ICONS_HIDE: AtomicBool = AtomicBool::new(false);
/// Handle of the skin's own window.
pub static MAIN: AtomicIsize = AtomicIsize::new(0);

static APP: OnceLock<AppHandle> = OnceLock::new();
/// Window labels; a window's handle is its place in this list + 1.
static LABELS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The handle of one of our windows (see the module notes).
pub fn raw_of(w: &WebviewWindow) -> isize {
    let _ = APP.set(w.app_handle().clone());
    let mut v = LABELS.lock().unwrap();
    let label = w.label().to_string();
    let i = match v.iter().position(|l| *l == label) {
        Some(i) => i,
        None => {
            v.push(label);
            v.len() - 1
        }
    };
    (i + 1) as isize
}

fn window(raw: isize) -> Option<WebviewWindow> {
    let label = LABELS.lock().unwrap().get((raw as usize).checked_sub(1)?)?.clone();
    APP.get()?.get_webview_window(&label)
}

fn on_main_thread() -> bool {
    std::thread::current().name() == Some("main")
}

/// Run `f` on the main thread (GTK / AppKit live there) and wait for its answer.
fn on_main<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    if on_main_thread() {
        return Some(f());
    }
    let app = APP.get()?;
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .ok()?;
    rx.recv_timeout(Duration::from_secs(5)).ok()
}

/// The desktop is ready as soon as we run (nothing to wait for, unlike Explorer at sign-in).
pub fn shell_ready() -> bool {
    true
}

/// Hide / show the desktop icons. Linux: the skin covers the file manager's desktop in any case.
/// macOS: the skin moves over Finder's icons (on) or under them (off).
pub fn set_icons_hidden(hide: bool) {
    let was = ICONS_HIDE.swap(hide, Ordering::Relaxed);
    if cfg!(target_os = "macos") && was != hide && current_mode() == Mode::Wallpaper {
        let main = MAIN.load(Ordering::Relaxed);
        if main != 0 {
            let _ = set_mode(main, Mode::Wallpaper);
        }
        place_mirrors(Mode::Wallpaper);
    }
}

/// macOS: the skin sits under Finder's icons (Wallpaper mode with the icons shown), so the mouse
/// reaches it only through deskmouse.rs.
#[allow(dead_code)]
pub fn behind_icons() -> bool {
    cfg!(target_os = "macos") && is_wallpaper() && !ICONS_HIDE.load(Ordering::Relaxed)
}

pub fn is_wallpaper() -> bool {
    MODE.load(Ordering::Relaxed) == Mode::Wallpaper as u8
}

/// Monitor id to cover ("" = primary).
static TARGET: Mutex<String> = Mutex::new(String::new());

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Monitor {
    pub id: String,
    pub primary: bool,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

static MONS: Mutex<Option<(Instant, Vec<Monitor>)>> = Mutex::new(None);

#[cfg(target_os = "linux")]
fn native_monitors() -> Vec<Monitor> {
    let mut v: Vec<Monitor> = crate::x11::monitors().into_iter().map(|(id, primary, x, y, w, h)| Monitor { id, primary, x, y, w, h }).collect();
    if v.is_empty() {
        // no X server (a pure Wayland session): what GTK knows
        if let Some(app) = APP.get() {
            let primary = app.primary_monitor().ok().flatten().map(|m| *m.position());
            v = app
                .available_monitors()
                .unwrap_or_default()
                .iter()
                .map(|m| Monitor {
                    id: m.name().cloned().unwrap_or_else(|| format!("{},{}", m.position().x, m.position().y)),
                    primary: Some(*m.position()) == primary,
                    x: m.position().x,
                    y: m.position().y,
                    w: m.size().width as i32,
                    h: m.size().height as i32,
                })
                .collect();
        }
    }
    v
}

/// macOS: in points (the unit windows are placed in), top-left origin like CoreGraphics.
#[cfg(target_os = "macos")]
fn native_monitors() -> Vec<Monitor> {
    crate::mac::displays()
        .into_iter()
        .map(|(id, primary, r)| Monitor {
            id,
            primary,
            x: r.origin.x.round() as i32,
            y: r.origin.y.round() as i32,
            w: r.size.width.round() as i32,
            h: r.size.height.round() as i32,
        })
        .collect()
}

/// All monitors, left to right (asked again at most once a second).
pub fn monitors() -> Vec<Monitor> {
    if let Some((t, v)) = MONS.lock().unwrap().as_ref() {
        if t.elapsed() < Duration::from_millis(1000) && !v.is_empty() {
            return v.clone();
        }
    }
    // asked without holding the lock (the Wayland fallback waits for the main thread)
    let mut v = native_monitors();
    v.sort_by_key(|m| (m.x, m.y));
    *MONS.lock().unwrap() = Some((Instant::now(), v.clone()));
    v
}

pub fn set_target(id: &str) {
    *TARGET.lock().unwrap() = id.to_string();
}

fn target_monitor() -> Option<Monitor> {
    let want = TARGET.lock().unwrap().clone();
    let list = monitors();
    list.iter()
        .find(|m| !want.is_empty() && m.id == want)
        .or_else(|| list.iter().find(|m| m.primary))
        .or_else(|| list.first())
        .cloned()
}

/// Id of the monitor the main skin is on (chosen one, else primary).
pub fn target_id() -> String {
    target_monitor().map(|m| m.id).unwrap_or_default()
}

/// Rect of the chosen monitor (falls back to the primary one).
pub fn target_rect() -> (i32, i32, i32, i32) {
    target_monitor().map(|m| (m.x, m.y, m.w, m.h)).unwrap_or((0, 0, 1920, 1080))
}

/// Companion windows on the other monitors: (handle, monitor id).
static MIRRORS: Mutex<Vec<(isize, String)>> = Mutex::new(Vec::new());

pub fn add_mirror(raw: isize, monitor: &str) {
    let mut v = MIRRORS.lock().unwrap();
    v.retain(|(h, _)| *h != raw);
    v.push((raw, monitor.to_string()));
}

pub fn remove_mirror(raw: isize) {
    MIRRORS.lock().unwrap().retain(|(h, _)| *h != raw);
}

pub fn mirror_monitor(raw: isize) -> Option<String> {
    MIRRORS.lock().unwrap().iter().find(|(h, _)| *h == raw).map(|(_, id)| id.clone())
}

fn mirror_handles() -> Vec<isize> {
    MIRRORS.lock().unwrap().iter().map(|(h, _)| *h).collect()
}

fn is_mirror(raw: isize) -> bool {
    MIRRORS.lock().unwrap().iter().any(|(m, _)| *m == raw)
}

/// Rect a window should cover: its own monitor for a mirror, else the target.
fn rect_for(raw: isize) -> (i32, i32, i32, i32) {
    if let Some(id) = mirror_monitor(raw) {
        if let Some(m) = monitors().into_iter().find(|m| m.id == id) {
            return (m.x, m.y, m.w, m.h);
        }
    }
    target_rect()
}

#[allow(dead_code)]
pub const APP_ID: &str = "com.tan.wallplayer";

/// Before the windows exist. Linux: on a Wayland session GTK cannot put a window in the desktop layer
/// (or keep it under the others), so Wallplayer uses X11 through XWayland, as on an X11 session.
pub fn set_app_id() {
    #[cfg(target_os = "linux")]
    {
        let set = |k: &str| std::env::var_os(k).map(|v| !v.is_empty()).unwrap_or(false);
        if !set("GDK_BACKEND") && set("DISPLAY") && set("WAYLAND_DISPLAY") {
            std::env::set_var("GDK_BACKEND", "x11");
        }
    }
}

/// Prepare a window of ours (main skin or mirror): out of the task bar / window switcher, on
/// every workspace. Main thread.
pub fn install(raw: isize) {
    let _ = MAIN.compare_exchange(0, raw, Ordering::Relaxed, Ordering::Relaxed);
    let Some(w) = window(raw) else { return };
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::*;
        // GTK holds a non-resizable window at its first size or bigger, and Tauri makes the window
        // non-resizable (as the config says) when it first appears: keep it resizable, and fit it
        // again then, so it can match a monitor smaller than that first size
        let _ = w.set_resizable(true);
        if let Ok(gw) = w.gtk_window() {
            gw.set_skip_taskbar_hint(true);
            gw.set_skip_pager_hint(true);
            gw.stick();
            gw.connect_configure_event(move |gw, _| {
                if !gw.is_resizable() {
                    gw.set_resizable(true);
                    glib::idle_add_local_once(move || {
                        let mode = if is_mirror(raw) && current_mode() == Mode::Peek { Mode::Desktop } else { current_mode() };
                        let _ = place(raw, mode);
                    });
                }
                false
            });
            // a mode change hides the window and shows it again (the window manager reads the type
            // only then); the window manager may clear the old window's states (above the others,
            // on every workspace) after we set the new ones: set them once more when it is on screen
            gw.connect_map_event(move |gw, _| {
                let mode = if is_mirror(raw) && current_mode() == Mode::Peek { Mode::Desktop } else { current_mode() };
                gw.set_keep_above(mode == Mode::Peek);
                gw.set_keep_below(mode == Mode::Desktop);
                gw.stick();
                if mode == Mode::Peek {
                    gw.present();
                }
                glib::Propagation::Proceed
            });
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = &w;
    }
}

pub fn set_mode(raw: isize, mode: Mode) -> Result<(), String> {
    place(raw, mode)
}

/// Place every mirror window in the given mode (Peek is never used for them).
pub fn place_mirrors(mode: Mode) {
    let mode = if mode == Mode::Peek { Mode::Desktop } else { mode };
    for raw in mirror_handles() {
        if place(raw, mode).is_err() {
            let _ = place(raw, Mode::Desktop);
        }
    }
}

pub fn current_mode() -> Mode {
    Mode::from_u8(MODE.load(Ordering::Relaxed))
}

fn place(raw: isize, mode: Mode) -> Result<(), String> {
    let w = window(raw).ok_or_else(|| "window not found".to_string())?;
    if !is_mirror(raw) {
        MODE.store(mode as u8, Ordering::SeqCst);
    }
    let rect = rect_for(raw);
    let icons = ICONS_HIDE.load(Ordering::Relaxed);
    on_main(move || apply(&w, mode, rect, icons)).unwrap_or_else(|| Err("the main thread did not answer".into()))
}

#[cfg(target_os = "linux")]
fn apply(w: &WebviewWindow, mode: Mode, (x, y, ww, hh): (i32, i32, i32, i32), _icons: bool) -> Result<(), String> {
    use gtk::gdk::WindowTypeHint;
    use gtk::prelude::*;
    let gw = w.gtk_window().map_err(|e| e.to_string())?;
    let hint = if mode == Mode::Wallpaper { WindowTypeHint::Desktop } else { WindowTypeHint::Normal };
    if gw.type_hint() != hint {
        // the window manager reads the type when the window appears: hide, change, show again
        if gw.is_visible() {
            gw.hide();
        }
        gw.set_type_hint(hint);
    }
    // Tauri makes the window non-resizable again when it first appears (the config says so), and
    // GTK then never makes it smaller than its first size: smaller monitors need it resizable
    gw.set_resizable(true);
    gw.set_keep_above(mode == Mode::Peek);
    gw.set_keep_below(mode == Mode::Desktop);
    gw.set_skip_taskbar_hint(true);
    gw.set_skip_pager_hint(true);
    gw.set_decorated(false);
    let pos = tauri::PhysicalPosition::new(x, y);
    let size = tauri::PhysicalSize::new(ww.max(1) as u32, hh.max(1) as u32);
    let _ = w.set_position(pos);
    let _ = w.set_size(size);
    if !gw.is_visible() {
        gw.show_all();
    }
    gw.stick();
    // a window manager may place a window its own way when it appears: once more after that
    let _ = w.set_position(pos);
    let _ = w.set_size(size);
    if mode == Mode::Peek {
        gw.present();
    }
    Ok(())
}

#[cfg(target_os = "macos")]
#[allow(deprecated)]
fn apply(w: &WebviewWindow, mode: Mode, (x, y, ww, hh): (i32, i32, i32, i32), icons_hidden: bool) -> Result<(), String> {
    use crate::mac::{kCGDesktopIconWindowLevelKey, kCGDesktopWindowLevelKey, kCGFloatingWindowLevelKey, level};
    use objc2_app_kit::{NSApplication, NSWindow, NSWindowCollectionBehavior};
    use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};

    let ptr = w.ns_window().map_err(|e| e.to_string())?;
    if ptr.is_null() {
        return Err("no window".into());
    }
    let ns: &NSWindow = unsafe { &*(ptr as *const NSWindow) };
    let behind_icons = mode == Mode::Wallpaper && !icons_hidden;
    let lvl = match mode {
        Mode::Peek => level(kCGFloatingWindowLevelKey),
        _ if behind_icons => level(kCGDesktopWindowLevelKey),
        _ => level(kCGDesktopIconWindowLevelKey) + 1,
    };
    ns.setLevel(lvl);
    let mut cb = NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::IgnoresCycle;
    cb |= if mode == Mode::Peek { NSWindowCollectionBehavior::FullScreenAuxiliary } else { NSWindowCollectionBehavior::Stationary };
    ns.setCollectionBehavior(cb);
    ns.setHasShadow(false);
    // CoreGraphics rect (top-left origin) → Cocoa frame (bottom-left origin of the main display)
    let top = crate::mac::main_height();
    let frame = NSRect::new(NSPoint::new(x as f64, top - (y + hh) as f64), NSSize::new(ww as f64, hh as f64));
    ns.setFrame_display(frame, true);
    if mode == Mode::Peek {
        if let Some(mtm) = MainThreadMarker::new() {
            NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        }
        ns.makeKeyAndOrderFront(None);
    } else {
        ns.orderFrontRegardless();
    }
    Ok(())
}

/// Open a URL or URI (https:, spotify:, …) with the default app.
pub fn open_external(url: &str) {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    if let Ok(mut child) = std::process::Command::new(opener).arg(url).spawn() {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

/// Windows brings its taskbar up when the mouse touches the bottom edge; panels and the Dock here
/// do that themselves.
pub fn reveal_taskbar(_raw: isize) {}

/// App windows that can be seen on the target monitor: (title, rect).
fn blocking_windows() -> Vec<String> {
    let (mx, my, sw, sh) = target_rect();
    let overlaps = |x: i32, y: i32, w: i32, h: i32| x < mx + sw && x + w > mx && y < my + sh && y + h > my;
    #[cfg(target_os = "linux")]
    {
        let me = std::process::id();
        crate::x11::clients()
            .into_iter()
            .filter(|c| !c.desktop && c.pid != me && c.w >= 120 && c.h >= 80 && overlaps(c.x, c.y, c.w, c.h))
            .map(|c| if c.title.is_empty() { format!("0x{:x}", c.win) } else { c.title })
            .collect()
    }
    #[cfg(target_os = "macos")]
    {
        crate::mac::app_windows()
            .into_iter()
            .filter(|(_, r)| overlaps(r.origin.x as i32, r.origin.y as i32, r.size.width as i32, r.size.height as i32))
            .map(|(o, _)| o)
            .collect()
    }
}

/// Watches the desktop: when monitors change, everything is fitted again. Linux: in Wallpaper mode,
/// keep the skin above another desktop window that appeared later (a file manager's desktop
/// started after Wallplayer at login would otherwise hide it).
pub fn spawn_watcher(raw: isize) {
    std::thread::spawn(move || {
        let key = || monitors().iter().map(|m| format!("{}@{},{},{}x{}", m.id, m.x, m.y, m.w, m.h)).collect::<Vec<_>>().join("|");
        let mut last = key();
        loop {
            std::thread::sleep(Duration::from_millis(1500));
            let now = key();
            if now != last {
                last = now;
                let _ = place(raw, current_mode());
                place_mirrors(current_mode());
            }
            #[cfg(target_os = "linux")]
            if current_mode() == Mode::Wallpaper {
                let me = std::process::id();
                let cl = crate::x11::clients();
                let lowest_mine = cl.iter().position(|c| c.pid == me && c.desktop);
                let other_above = lowest_mine.map(|i| cl[i + 1..].iter().any(|c| c.desktop && c.pid != me)).unwrap_or(false);
                if other_above {
                    crate::guard::note("another desktop window covered the skin: raising the skin again");
                    let mut all = vec![raw];
                    all.extend(mirror_handles());
                    let _ = on_main(move || {
                        use gtk::prelude::*;
                        for r in all {
                            if let Some(gw) = window(r).and_then(|w| w.gtk_window().ok()) {
                                if let Some(gdk) = gw.window() {
                                    gdk.raise();
                                }
                            }
                        }
                    });
                }
            }
        }
    });
}

/// Average colours of the picture shown on the target monitor (the playing music video), in two quiet
/// zones left and right of the middle — the same zones as on Windows. [left, right] as RGB.
/// Linux: read from the X server. macOS: reading the screen needs the Screen Recording permission, so
/// the page works the colours out by itself there (None).
pub fn screen_colors() -> Option<[[u8; 3]; 2]> {
    #[cfg(target_os = "linux")]
    {
        if !blocking_windows().is_empty() {
            return None; // an app window covers the screen: its colours are not the video's
        }
        let (mx, my, sw, sh) = target_rect();
        let zone = |gx0: i32, gx1: i32| -> Option<[u8; 3]> {
            // the zones of a 64 × 36 grid: x 12–22 / 42–52, y 8–22
            let (x0, x1) = (mx + sw * gx0 / 64, mx + sw * gx1 / 64);
            let (y0, y1) = (my + sh * 8 / 36, my + sh * 22 / 36);
            let (w, h) = (x1 - x0, y1 - y0);
            let px = crate::x11::grab(x0, y0, w, h)?;
            let (mut r, mut g, mut b, mut c) = (0u64, 0u64, 0u64, 0u64);
            for yy in (0..h).step_by(4) {
                for xx in (0..w).step_by(4) {
                    let i = ((yy * w + xx) * 4) as usize;
                    b += px[i] as u64;
                    g += px[i + 1] as u64;
                    r += px[i + 2] as u64;
                    c += 1;
                }
            }
            let c = c.max(1);
            Some([(r / c) as u8, (g / c) as u8, (b / c) as u8])
        };
        Some([zone(12, 22)?, zone(42, 52)?])
    }
    #[cfg(target_os = "macos")]
    {
        None
    }
}

/// Path of the desktop picture of the monitor a window is on. None for a plain colour.
pub fn wallpaper_for(raw: isize) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let _ = raw;
        linux_wallpaper()
    }
    #[cfg(target_os = "macos")]
    {
        let w = window(raw)?;
        on_main(move || {
            use objc2_app_kit::{NSScreen, NSWindow, NSWorkspace};
            use objc2_foundation::MainThreadMarker;
            let mtm = MainThreadMarker::new()?;
            let screen = w
                .ns_window()
                .ok()
                .filter(|p| !p.is_null())
                .and_then(|p| unsafe { &*(p as *const NSWindow) }.screen())
                .or_else(|| NSScreen::mainScreen(mtm))?;
            let url = NSWorkspace::sharedWorkspace().desktopImageURLForScreen(&screen)?;
            url.path().map(|p| p.to_string())
        })
        .flatten()
        .filter(|p| std::path::Path::new(p).is_file())
    }
}

/// The desktop picture on Linux: GNOME / Ubuntu / Budgie (dark style first when on), Cinnamon, MATE,
/// Xfce, KDE Plasma.
#[cfg(target_os = "linux")]
fn linux_wallpaper() -> Option<String> {
    use gio::prelude::*;
    let setting = |schema: &str, key: &str| -> Option<String> {
        let src = gio::SettingsSchemaSource::default()?;
        let sc = src.lookup(schema, true)?;
        if !sc.has_key(key) {
            return None;
        }
        let s = gio::Settings::new_full(&sc, None::<&gio::SettingsBackend>, None);
        let v: String = s.string(key).into();
        Some(v).filter(|v| !v.is_empty())
    };
    let path_of = |v: String| -> Option<String> {
        let p = if v.starts_with("file://") { glib::filename_from_uri(&v).ok()?.0 } else { std::path::PathBuf::from(v) };
        p.is_file().then(|| p.display().to_string())
    };
    let dark = setting("org.gnome.desktop.interface", "color-scheme").map(|v| v == "prefer-dark").unwrap_or(false);
    let mut tries: Vec<(&str, &str)> = Vec::new();
    if dark {
        tries.push(("org.gnome.desktop.background", "picture-uri-dark"));
    }
    tries.extend([
        ("org.gnome.desktop.background", "picture-uri"),
        ("org.cinnamon.desktop.background", "picture-uri"),
        ("org.mate.background", "picture-filename"),
    ]);
    // KDE and Xfce first when they are the desktop in use (GNOME's settings may linger there)
    let de = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
    let kde = || -> Option<String> {
        let home = std::env::var("HOME").ok()?;
        let txt = std::fs::read_to_string(format!("{home}/.config/plasma-org.kde.plasma.desktop-appletsrc")).ok()?;
        txt.lines().filter_map(|l| l.trim().strip_prefix("Image=")).map(|v| v.to_string()).find_map(path_of)
    };
    let xfce = || -> Option<String> {
        let out = std::process::Command::new("xfconf-query").args(["-c", "xfce4-desktop", "-l", "-v"]).output().ok()?;
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| l.contains("/last-image"))
            .filter_map(|l| l.split_whitespace().nth(1).map(String::from))
            .find_map(path_of)
    };
    if de.contains("kde") {
        if let Some(p) = kde() {
            return Some(p);
        }
    }
    if de.contains("xfce") {
        if let Some(p) = xfce() {
            return Some(p);
        }
    }
    tries.into_iter().filter_map(|(s, k)| setting(s, k)).find_map(path_of).or_else(kde)
}

/// Self-test text: the placement, and which app windows count as covering the skin.
pub fn debug_state() -> String {
    let v = blocking_windows();
    format!(
        "mode={} icons_hidden={} monitors={} blocking_windows={}{}",
        MODE.load(Ordering::Relaxed),
        ICONS_HIDE.load(Ordering::Relaxed),
        monitors().len(),
        v.len(),
        if v.is_empty() { String::new() } else { format!(" → {}", v.join(" | ")) }
    )
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    /// Needs a desktop session: `cargo test -- --ignored wallpaper`
    #[test]
    #[ignore]
    fn wallpaper() {
        let p = super::linux_wallpaper();
        println!("desktop picture: {p:?}");
        assert!(p.is_some());
    }

    #[test]
    #[ignore]
    fn monitors_and_windows() {
        println!("monitors: {:?}", super::monitors());
        println!("windows: {:?}", crate::x11::clients().iter().map(|c| (&c.title, c.x, c.y, c.w, c.h, c.desktop)).collect::<Vec<_>>());
        assert!(!super::monitors().is_empty());
    }
}
