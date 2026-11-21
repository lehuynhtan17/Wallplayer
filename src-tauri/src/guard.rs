//! Keeps the skin alive when the web engine behind it dies.
//!
//! The interface runs in Microsoft Edge WebView2, in separate processes. If the
//! WebView2 browser process ends (a crash, an engine update…) every window stays
//! on screen but is empty: the wallpaper looks as if Wallplayer had quit, while
//! it still sits in the tray. When that happens Wallplayer starts itself again.
//! When only a page's process dies or hangs, that page is reloaded.
//!
//! Each failure is written to `wallplayer.log` in the app's local data folder,
//! and Wallplayer gives up relaunching after 3 relaunches within 10 minutes.

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager, WebviewWindow};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2ProcessFailedEventArgs2, COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED, COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
    COREWEBVIEW2_PROCESS_FAILED_REASON,
};
use webview2_com::ProcessFailedEventHandler;
use windows_core::Interface;

/// Set when Wallplayer quits (or restarts itself) on purpose: its windows closing then is expected.
pub static QUITTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

const MAX_RELAUNCHES: usize = 3;
const WINDOW_SECS: u64 = 600;

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn data_dir(app: &AppHandle) -> Option<PathBuf> {
    let d = app.path().app_local_data_dir().ok()?;
    let _ = std::fs::create_dir_all(&d);
    Some(d)
}

/// One line in wallplayer.log (kept small: the newest 200 lines).
pub fn log(app: &AppHandle, line: &str) {
    let Some(d) = data_dir(app) else { return };
    let p = d.join("wallplayer.log");
    let mut lines: Vec<String> = std::fs::read_to_string(&p).unwrap_or_default().lines().map(String::from).collect();
    let keep = lines.len().saturating_sub(199);
    lines.drain(..keep);
    lines.push(format!("{} v{} {}", now(), env!("CARGO_PKG_VERSION"), line));
    if let Ok(mut f) = std::fs::File::create(&p) {
        let _ = f.write_all((lines.join("\r\n") + "\r\n").as_bytes());
    }
}

/// Whether another relaunch is allowed now (and count this one).
fn may_relaunch(app: &AppHandle) -> bool {
    let Some(d) = data_dir(app) else { return true };
    let p = d.join("relaunches.txt");
    let t = now();
    let mut recent: Vec<u64> = std::fs::read_to_string(&p)
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|x| x.parse().ok())
        .filter(|x| t.saturating_sub(*x) < WINDOW_SECS)
        .collect();
    if recent.len() >= MAX_RELAUNCHES {
        return false;
    }
    recent.push(t);
    let _ = std::fs::write(&p, recent.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" "));
    true
}

pub fn relaunch(app: AppHandle) {
    // off the WebView2 callback: give the dying engine a moment, then start a fresh Wallplayer
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(800));
        if !may_relaunch(&app) {
            log(&app, "web engine stopped again: not relaunching (3 times in 10 minutes)");
            return;
        }
        log(&app, "relaunching Wallplayer");
        if let Err(e) = crate::update::restart(&app) {
            log(&app, &format!("relaunch failed: {e}"));
        }
    });
}

/// Watch one window's web engine. `main`: the browser process ending relaunches the app.
pub fn watch(win: &WebviewWindow, main: bool) {
    let app = win.app_handle().clone();
    let label = win.label().to_string();
    let _ = win.with_webview(move |wv| unsafe {
        let Ok(core) = wv.controller().CoreWebView2() else { return };
        let handler = ProcessFailedEventHandler::create(Box::new(move |sender, args| {
            let Some(args) = args else { return Ok(()) };
            let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
            args.ProcessFailedKind(&mut kind)?;
            let (mut reason, mut code) = (COREWEBVIEW2_PROCESS_FAILED_REASON::default(), 0i32);
            if let Ok(a2) = args.cast::<ICoreWebView2ProcessFailedEventArgs2>() {
                let _ = a2.Reason(&mut reason);
                let _ = a2.ExitCode(&mut code);
            }
            log(&app, &format!("{label}: web process failed, kind {} reason {} exit {:#x}", kind.0, reason.0, code));
            if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED {
                if main {
                    relaunch(app.clone());
                }
            } else if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
                || kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE
            {
                if let Some(s) = sender {
                    let _ = s.Reload();
                }
            }
            Ok(())
        }));
        let mut token = 0i64;
        let _ = core.add_ProcessFailed(&handler, &mut token);
    });
}
