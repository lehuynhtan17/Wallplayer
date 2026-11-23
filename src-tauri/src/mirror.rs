//! Companion windows for the other monitors.
//!
//! The main skin lives on one monitor. Every other monitor gets a "mirror"
//! window: the same `ui/index.html` in mirror mode (never plays sound; its
//! playlist, cover flow and TikTok grid can be clicked, which asks the main
//! window to play: `mirror-cmd`). It reads the same saved settings, and the
//! main window broadcasts what plays with the `mirror-state` event.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::accounts::{BROWSER_ARGS, UA as USER_AGENT};
use crate::{desktop, AppState};


/// Where each mirror window is (its monitor's rectangle, in the same units as `desktop::target_rect`),
/// so the desktop mouse forwarding (Windows, macOS under Finder's icons) can reach the other displays.
static TARGETS: std::sync::Mutex<Vec<(String, i32, i32, i32, i32)>> = std::sync::Mutex::new(Vec::new());

/// The mirror window under a desktop point: its label and the point inside its monitor. (Linux needs no
/// forwarding: the skin windows take the mouse themselves.)
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub fn target_at(x: i32, y: i32) -> Option<(String, i32, i32)> {
    let v = TARGETS.lock().unwrap_or_else(|e| e.into_inner());
    v.iter()
        .find(|(_, mx, my, w, h)| x >= *mx && x < mx + w && y >= *my && y < my + h)
        .map(|(l, mx, my, _, _)| (l.clone(), x - mx, y - my))
}

fn label_for(monitor_id: &str) -> String {
    let clean: String = monitor_id.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    format!("mirror-{clean}")
}

/// Make the set of mirror windows match the monitors: one per monitor other
/// than the main skin's, none when turned off. Safe to call from anywhere.
pub fn sync(app: &AppHandle) {
    let app = app.clone();
    // Windows are built off the main thread (building inside a menu handler or
    // a sync command deadlocks WebView2 on Windows).
    tauri::async_runtime::spawn(async move {
        let off = app.state::<AppState>().settings.lock().unwrap().no_mirror;
        let main_id = desktop::target_id();
        let wanted: Vec<desktop::Monitor> = if off {
            vec![]
        } else {
            desktop::monitors().into_iter().filter(|m| m.id != main_id).collect()
        };
        let wanted_labels: Vec<String> = wanted.iter().map(|m| label_for(&m.id)).collect();
        *TARGETS.lock().unwrap_or_else(|e| e.into_inner()) = wanted.iter().map(|m| (label_for(&m.id), m.x, m.y, m.w, m.h)).collect();

        // put away mirrors that are no longer needed: hidden and emptied, not destroyed (tearing a
        // window down can crash the WebView2 engine all Wallplayer windows share); reused if the
        // display comes back
        for (label, w) in app.webview_windows() {
            if label.starts_with("mirror-") && !wanted_labels.contains(&label) {
                let raw = crate::raw_of(&w);
                if raw != 0 {
                    desktop::remove_mirror(raw);
                }
                let _ = w.hide();
                if let Ok(u) = tauri::Url::parse("about:blank") {
                    let _ = w.navigate(u);
                }
            }
        }
        let page = app.get_webview_window("main").and_then(|m| m.url().ok());

        // open the missing ones
        for m in wanted {
            let label = label_for(&m.id);
            let win = match app.get_webview_window(&label) {
                Some(w) => {
                    // put away earlier: load the skin again
                    if w.url().map(|u| u.as_str() == "about:blank").unwrap_or(false) {
                        if let Some(p) = page.clone() {
                            let _ = w.navigate(p);
                        }
                    }
                    w
                }
                None => match WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("index.html".into()))
                    .initialization_script("window.__WP_MIRROR__ = true;")
                    .title("Wallplayer")
                    .decorations(false)
                    .resizable(false)
                    .skip_taskbar(true)
                    .visible(false)
                    .focused(false)
                    .shadow(false)
                    .use_https_scheme(true)
                    .user_agent(USER_AGENT)
                    .additional_browser_args(BROWSER_ARGS)
                    .position(m.x as f64, m.y as f64)
                    .build()
                {
                    Ok(w) => {
                        crate::guard::watch(&w, false);
                        w
                    }
                    Err(e) => {
                        eprintln!("mirror window: {e}");
                        continue;
                    }
                },
            };
            let raw = crate::raw_of(&win);
            if raw == 0 {
                continue;
            }
            desktop::add_mirror(raw, &m.id);
            let first = !app.state::<AppState>().mirrors_installed.lock().unwrap().contains(&raw);
            if first {
                app.state::<AppState>().mirrors_installed.lock().unwrap().push(raw);
            }
            // Window subclassing has to happen on the thread that owns the window.
            let _ = app.run_on_main_thread(move || {
                if first {
                    desktop::install(raw);
                }
                desktop::place_mirrors(desktop::current_mode());
            });
        }
    });
}
