//! Companion windows for the other monitors.
//!
//! The main skin lives on one monitor. Every other monitor gets a "mirror"
//! window: the same `ui/index.html` in display-only mode (no controls, never
//! plays sound). It reads the same saved settings, and the main window
//! broadcasts what plays with the `mirror-state` event.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::accounts::{BROWSER_ARGS, UA as USER_AGENT};
use crate::{desktop, AppState};


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
