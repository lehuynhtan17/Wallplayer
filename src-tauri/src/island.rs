//! Dynamic Island: a small capsule that stays on top of every window, at the top
//! of the display the player lives on. It shows the song (and the line being sung)
//! while the wallpaper player can't be seen: a maximised window covers it, or the
//! player was collapsed with the collapse button.
//!
//! The page is `ui/island.html`. The main window decides when it shows
//! (`island_show`); the island asks to grow when the mouse points at it
//! (`island_size`). It reads what plays from the same `mirror-state` broadcast
//! the other monitors use, and sends its buttons back as `island-cmd`.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
#[cfg(windows)]
use windows::Win32::Foundation::HWND;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW};

use crate::desktop;

const LABEL: &str = "island";
/// window size (logical px), a little larger than the capsule for its soft shadow
const SMALL: (f64, f64) = (316.0, 52.0);
const BIG: (f64, f64) = (404.0, 172.0);

static WANT: AtomicBool = AtomicBool::new(false);
static OPEN: AtomicBool = AtomicBool::new(false);
static GEN: AtomicU64 = AtomicU64::new(0);

fn build(app: &AppHandle) -> Option<WebviewWindow> {
    let w = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("island.html".into()))
        .title("Wallplayer")
        .decorations(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .transparent(true)
        .shadow(false)
        .focused(false)
        .visible(false)
        .inner_size(SMALL.0, SMALL.1)
        .use_https_scheme(true)
        .build()
        .map_err(|e| eprintln!("island window: {e}"))
        .ok()?;
    #[cfg(windows)]
    if let Ok(h) = w.hwnd() {
        // not in Alt+Tab, and clicking it does not take the focus from the app you work in
        unsafe {
            let hwnd = HWND(h.0 as *mut core::ffi::c_void);
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize);
        }
    }
    Some(w)
}

/// Size it (small or open) and put it at the top centre of the player's display.
fn place(w: &WebviewWindow) {
    let (lw, lh) = if OPEN.load(Ordering::Relaxed) { BIG } else { SMALL };
    let id = desktop::target_id();
    let mons = desktop::monitors();
    let Some(m) = mons.iter().find(|m| m.id == id).or_else(|| mons.iter().find(|m| m.primary)).or(mons.first()) else {
        return;
    };
    let scale = w
        .available_monitors()
        .ok()
        .and_then(|v| v.into_iter().find(|t| t.position().x == m.x && t.position().y == m.y).map(|t| t.scale_factor()))
        .unwrap_or(1.0);
    let (pw, ph) = ((lw * scale).round() as i32, (lh * scale).round() as i32);
    let at = PhysicalPosition::new(m.x + (m.w - pw) / 2, m.y);
    // move first: crossing to a display with another scale makes Windows resize it
    let _ = w.set_position(at);
    let _ = w.set_size(PhysicalSize::new(pw as u32, ph as u32));
    let _ = w.set_position(at);
}

/// Show or hide the island. Hiding lets it play its little exit first.
pub fn show(app: &AppHandle, on: bool) {
    WANT.store(on, Ordering::Relaxed);
    let gen = GEN.fetch_add(1, Ordering::Relaxed) + 1;
    let app = app.clone();
    // windows are built off the main thread (building inside a command deadlocks WebView2)
    tauri::async_runtime::spawn(async move {
        if on {
            let w = match app.get_webview_window(LABEL) {
                Some(w) => w,
                None => match build(&app) {
                    Some(w) => w,
                    None => return,
                },
            };
            if GEN.load(Ordering::Relaxed) != gen {
                return;
            }
            OPEN.store(false, Ordering::Relaxed);
            place(&w);
            let _ = w.show();
            let _ = app.emit_to(LABEL, "island-vis", true);
        } else if let Some(w) = app.get_webview_window(LABEL) {
            let _ = app.emit_to(LABEL, "island-vis", false);
            tokio::time::sleep(Duration::from_millis(340)).await;
            if GEN.load(Ordering::Relaxed) == gen && !WANT.load(Ordering::Relaxed) {
                let _ = w.hide();
            }
        }
    });
}

/// The island grows while the mouse points at it, and shrinks back after.
pub fn set_open(app: &AppHandle, open: bool) {
    OPEN.store(open, Ordering::Relaxed);
    if let Some(w) = app.get_webview_window(LABEL) {
        place(&w);
    }
}

/// The player moved to another display: follow it.
pub fn follow(app: &AppHandle) {
    if WANT.load(Ordering::Relaxed) {
        if let Some(w) = app.get_webview_window(LABEL) {
            place(&w);
        }
    }
}
