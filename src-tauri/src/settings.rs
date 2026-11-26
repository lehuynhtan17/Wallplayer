use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
#[cfg(windows)]
use winreg::enums::HKEY_CURRENT_USER;
#[cfg(windows)]
use winreg::RegKey;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Settings {
    /// true = behind desktop icons (WorkerW), false = interactive desktop layer. Always true since
    /// 1.0.21 (the switch is gone: "Hide desktop icons" decides who takes the mouse)
    #[serde(default)]
    pub wallpaper_mode: bool,
    /// settings layout version (2 = Wallpaper Engine style placement is the default, 3 = always)
    #[serde(default)]
    pub mode_ver: u8,
    /// hide all desktop icons (incl. Recycle Bin) while Wallplayer runs, and the player takes the mouse;
    /// off: the icons show and only they take the mouse. Default on
    #[serde(default = "yes")]
    pub hide_icons: bool,
    /// shortcut that shows / hides the desktop icons (not on Linux)
    #[serde(default = "default_icons_key")]
    pub icons_key: String,
    #[serde(default)]
    pub spotify_client_id: String,
    #[serde(default)]
    pub spotify_refresh: String,
    /// monitor device name ("" = primary)
    #[serde(default)]
    pub monitor: String,
    /// true = leave the other monitors alone (no companion "mirror" windows)
    #[serde(default)]
    pub no_mirror: bool,
    /// "Search from anywhere" binding: "Ctrl+Alt+Space", "Mouse:Middle", "Ctrl+Mouse:X1"…
    #[serde(default = "default_search_key")]
    pub search_key: String,
    /// look for a new version on GitHub once when the app starts; default on
    #[serde(default = "yes")]
    pub auto_update: bool,
    /// version that ran last time (to say "updated" once)
    #[serde(default)]
    pub last_version: String,
    /// a version the user chose to skip: the tray menu does not offer it
    #[serde(default)]
    pub skip_version: String,
}

pub fn default_search_key() -> String {
    // macOS keeps Control+Option+Space for switching keyboard languages (Vietnamese ⇄ English):
    // Shift+Command+Space is free there
    if cfg!(target_os = "macos") { "Shift+Super+Space".into() } else { "Ctrl+Alt+Space".into() }
}

/// Show / hide the desktop icons: Ctrl+Alt+D (macOS: Control+Option+D), "D" as in Win+D.
pub fn default_icons_key() -> String {
    "Ctrl+Alt+D".into()
}

/// The default shortcut as this system writes it (tray menu).
#[cfg(not(windows))]
pub fn default_key_label() -> &'static str {
    if cfg!(target_os = "macos") { "⇧⌘Space" } else { "Ctrl+Alt+Space" }
}

fn yes() -> bool {
    true
}

fn path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("settings.json"))
}

pub fn load(app: &AppHandle) -> Settings {
    path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
        .map(|mut s| {
            if s.mode_ver < 3 {
                s.wallpaper_mode = true;
                s.mode_ver = 3;
            }
            s
        })
        .unwrap_or_else(|| Settings {
            wallpaper_mode: true,
            mode_ver: 3,
            hide_icons: true,
            search_key: default_search_key(),
            icons_key: default_icons_key(),
            auto_update: true,
            ..Default::default()
        })
}

pub fn save(app: &AppHandle, s: &Settings) {
    if let Some(p) = path(app) {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(p, serde_json::to_string_pretty(s).unwrap_or_default());
    }
}

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const APPROVED_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
#[cfg(windows)]
const RUN_NAME: &str = "Wallplayer";
/// Marks a start by Windows at sign-in (the app then waits for the desktop before going behind the icons).
pub const AUTOSTART_ARG: &str = "--autostart";

#[cfg(windows)]
fn run_value() -> Option<String> {
    RegKey::predef(HKEY_CURRENT_USER).open_subkey(RUN_KEY).ok()?.get_value::<String, _>(RUN_NAME).ok()
}

/// The exe path inside a Run command line (`"C:\…\Wallplayer.exe" --autostart` → `C:\…\Wallplayer.exe`).
#[cfg(windows)]
fn exe_of(cmd: &str) -> String {
    let c = cmd.trim();
    if let Some(rest) = c.strip_prefix('"') {
        rest.split('"').next().unwrap_or("").to_string()
    } else {
        c.split(" --").next().unwrap_or(c).trim().to_string()
    }
}

#[cfg(windows)]
pub(crate) fn same_path(a: &str, b: &std::path::Path) -> bool {
    let norm = |s: &str| s.trim_start_matches(r"\\?\").replace('/', "\\").to_lowercase();
    norm(a) == norm(&b.display().to_string())
}

#[cfg(windows)]
fn run_command() -> std::io::Result<String> {
    let exe = std::env::current_exe()?;
    let exe = exe.display().to_string();
    Ok(format!("\"{}\" {AUTOSTART_ARG}", exe.trim_start_matches(r"\\?\")))
}

/// Windows keeps its own on/off switch for each sign-in app (Task Manager → Startup apps, Settings →
/// Apps → Startup). First byte 2/6 = on, 3/7 = off; no value = on.
#[cfg(windows)]
fn approved() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(APPROVED_KEY)
        .and_then(|k| k.get_raw_value(RUN_NAME))
        .map(|v| v.bytes.first().map(|b| b & 1 == 0).unwrap_or(true))
        .unwrap_or(true)
}

/// On only when Windows will really start THIS copy of Wallplayer at sign-in (Run list, or the
/// High priority task).
#[cfg(windows)]
pub fn autostart_enabled() -> bool {
    let run = match (run_value(), std::env::current_exe()) {
        (Some(v), Ok(me)) => same_path(&exe_of(&v), &me) && approved(),
        _ => false,
    };
    run || crate::boot::high_on()
}

/// Takes Wallplayer out of Windows' Run list (and its Startup apps switch).
#[cfg(windows)]
fn remove_run() -> std::io::Result<()> {
    if let Ok(k) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(APPROVED_KEY, winreg::enums::KEY_ALL_ACCESS) {
        let _ = k.delete_value(RUN_NAME);
    }
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY)?;
    match key.delete_value(RUN_NAME) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(windows)]
pub fn set_autostart(on: bool) -> std::io::Result<()> {
    if !on {
        let _ = crate::boot::set_high(false);
        return remove_run();
    }
    if crate::boot::task_exists() {
        // High priority is on: point its task at this copy rather than adding a second start
        if crate::boot::high_on() || crate::boot::set_high(true).is_ok() {
            return remove_run();
        }
    }
    write_run()
}

/// "Start with Windows" → "High priority": the sign-in task instead of the Run list. Turning it off
/// keeps Start with Windows on, the normal way.
#[cfg(windows)]
pub fn set_autostart_high(on: bool) -> Result<(), String> {
    if on {
        crate::boot::set_high(true)?;
        remove_run().map_err(|e| e.to_string())
    } else {
        crate::boot::set_high(false)?;
        write_run().map_err(|e| e.to_string())
    }
}

#[cfg(windows)]
fn write_run() -> std::io::Result<()> {
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY)?;
    {
        key.set_value(RUN_NAME, &run_command()?)?;
        // turning it on here also turns Windows' own switch back on (it may have been switched off
        // in Task Manager or Settings, which silently keeps the app from starting)
        let (ok, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(APPROVED_KEY)?;
        let on_bytes = winreg::RegValue { bytes: vec![2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], vtype: winreg::enums::RegType::REG_BINARY };
        let _ = ok.set_raw_value(RUN_NAME, &on_bytes);
        Ok(())
    }
}

/// At start: if "Start with Windows" is on but points to another copy or folder (the folder was moved,
/// or an older copy turned it on), point it at this copy. Returns what was fixed, for the log.
#[cfg(windows)]
pub fn heal_autostart() -> Option<String> {
    if crate::boot::task_exists() {
        // started by the High priority task: never also from the Run list (that would open it twice)
        let twice = run_value().is_some() && remove_run().is_ok();
        let fix = crate::boot::heal();
        return match (fix, twice) {
            (Some(f), _) => Some(f),
            (None, true) => Some("removed the normal start: High priority already starts Wallplayer".into()),
            _ => None,
        };
    }
    let v = run_value()?;
    let me = std::env::current_exe().ok()?;
    let old = exe_of(&v);
    if same_path(&old, &me) && v.contains(AUTOSTART_ARG) {
        return None;
    }
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY).ok()?;
    key.set_value(RUN_NAME, &run_command().ok()?).ok()?;
    Some(format!("Start with Windows pointed to {old}; now points to this copy"))
}

// Linux / macOS: the sign-in start is a file (an autostart entry / a LaunchAgent), see unix/autostart.rs
#[cfg(not(windows))]
#[path = "unix/autostart.rs"]
mod unix_autostart;
#[cfg(not(windows))]
pub use unix_autostart::{autostart_enabled, heal_autostart, set_autostart, set_autostart_high};
