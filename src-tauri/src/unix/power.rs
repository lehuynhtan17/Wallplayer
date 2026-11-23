//! Power awareness on Linux and macOS (Windows: ../power.rs): battery / power saver, and whether
//! each of our windows is hidden behind a full-screen or maximised app.
//!
//! * Linux: the battery from /sys/class/power_supply, power saver from power-profiles-daemon,
//!   the windows from the X server (x11.rs).
//! * macOS: `pmset` (battery, Low Power Mode), the windows from the window server (mac.rs).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::desktop;

/// UI preference: rest while a window covers the screen (on by default).
static COVER_PAUSE: AtomicBool = AtomicBool::new(true);
/// UI preference: lighter mode is in effect (battery / saver / chosen).
pub static LOW: AtomicBool = AtomicBool::new(false);

pub fn set_prefs(cover_pause: bool, low: bool) {
    COVER_PAUSE.store(cover_pause, Ordering::Relaxed);
    LOW.store(low, Ordering::Relaxed);
}

#[derive(Serialize, Clone, PartialEq, Default, Debug)]
pub struct PowerState {
    pub battery: bool,
    pub saver: bool,
    pub full: bool,
    pub max: bool,
    pub monitor: String,
}

#[derive(Default, Clone, Copy)]
struct Cover {
    full: bool,
    max: bool,
}

/// What covers each monitor right now (monitor id → cover).
fn scan() -> HashMap<String, Cover> {
    let mons = desktop::monitors();
    let mut out: HashMap<String, Cover> = HashMap::new();
    // (x, y, w, h, full-screen state, maximised state) of each visible app window
    #[cfg(target_os = "linux")]
    let wins: Vec<(i32, i32, i32, i32, bool, bool)> = {
        let me = std::process::id();
        crate::x11::clients().into_iter().filter(|c| !c.desktop && c.pid != me).map(|c| (c.x, c.y, c.w, c.h, c.full, c.max)).collect()
    };
    #[cfg(target_os = "macos")]
    let wins: Vec<(i32, i32, i32, i32, bool, bool)> = crate::mac::app_windows()
        .into_iter()
        .map(|(_, r)| (r.origin.x.round() as i32, r.origin.y.round() as i32, r.size.width.round() as i32, r.size.height.round() as i32, false, false))
        .collect();
    for (x, y, w, h, full, max) in wins {
        // the monitor holding the window's centre
        let (cx, cy) = (x + w / 2, y + h / 2);
        let Some(m) = mons.iter().find(|m| cx >= m.x && cx < m.x + m.w && cy >= m.y && cy < m.y + m.h) else { continue };
        let c = out.entry(m.id.clone()).or_default();
        // a few pixels of slack: frames can sit 1–2 px inside the edge
        let covers_all = x <= m.x + 2 && y <= m.y + 2 && x + w >= m.x + m.w - 2 && y + h >= m.y + m.h - 2;
        // maximised: the whole width and most of the height (panels / menu bar / Dock take the rest)
        let covers_work = x <= m.x + 2 && x + w >= m.x + m.w - 2 && h >= m.h * 4 / 5;
        if full || covers_all {
            c.full = true;
        } else if max || covers_work {
            c.max = true;
        }
    }
    out
}

/// (on battery, power saver on)
#[cfg(target_os = "linux")]
fn battery() -> (bool, bool) {
    let mut ac_online = false;
    let mut has_battery = false;
    let mut discharging = false;
    if let Ok(rd) = std::fs::read_dir("/sys/class/power_supply") {
        for e in rd.flatten() {
            let p = e.path();
            let read = |n: &str| std::fs::read_to_string(p.join(n)).map(|s| s.trim().to_string()).unwrap_or_default();
            match read("type").as_str() {
                "Mains" | "USB" => ac_online |= read("online") == "1",
                // the laptop's own battery, not a mouse's or a headset's
                "Battery" if read("scope") != "Device" => {
                    has_battery = true;
                    discharging |= read("status") == "Discharging";
                }
                _ => {}
            }
        }
    }
    let on_battery = has_battery && (discharging || !ac_online);
    (on_battery, power_saver())
}

/// power-profiles-daemon (GNOME, KDE, Fedora, Ubuntu…): the "Power Saver" profile.
#[cfg(target_os = "linux")]
fn power_saver() -> bool {
    use gio::prelude::*;
    let Ok(c) = gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE) else { return false };
    for (name, path) in [("org.freedesktop.UPower.PowerProfiles", "/org/freedesktop/UPower/PowerProfiles"), ("net.hadess.PowerProfiles", "/net/hadess/PowerProfiles")] {
        let r = c.call_sync(
            Some(name),
            path,
            "org.freedesktop.DBus.Properties",
            "Get",
            Some(&(name, "ActiveProfile").to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            800,
            gio::Cancellable::NONE,
        );
        if let Ok(v) = r {
            let p: Option<String> = v.child_value(0).as_variant().and_then(|x| x.get());
            return p.as_deref() == Some("power-saver");
        }
    }
    false
}

#[cfg(target_os = "macos")]
fn battery() -> (bool, bool) {
    let run = |args: &[&str]| std::process::Command::new("pmset").args(args).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    // "Now drawing from 'Battery Power'"
    let on_battery = run(&["-g", "batt"]).contains("'Battery Power'");
    // " lowpowermode         1"
    let saver = run(&["-g"]).lines().any(|l| {
        let mut it = l.split_whitespace();
        it.next() == Some("lowpowermode") && it.next() == Some("1")
    });
    (on_battery, saver)
}

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last: HashMap<String, PowerState> = HashMap::new();
        let mut tick = 0u32;
        let (mut bat, mut saver) = battery();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(400));
            tick += 1;
            if tick % 12 == 0 {
                (bat, saver) = battery(); // every ~5 s is plenty
            }
            let cover = scan();
            let peek = desktop::current_mode() == desktop::Mode::Peek;
            let main_mon = desktop::target_id();
            let mut all_hidden = true;
            let mut any = false;
            for (label, w) in app.webview_windows() {
                let mon = if label == "main" {
                    Some(main_mon.clone())
                } else if label.starts_with("mirror-") {
                    desktop::mirror_monitor(crate::raw_of(&w))
                } else {
                    None
                };
                let Some(mon) = mon else { continue };
                any = true;
                let c = cover.get(&mon).copied().unwrap_or_default();
                let lifted = peek && label == "main";
                let ps = PowerState { battery: bat, saver, full: c.full && !lifted, max: c.max && !lifted, monitor: mon.clone() };
                if !(ps.full || ps.max) {
                    all_hidden = false;
                }
                if last.get(&label) != Some(&ps) {
                    let _ = app.emit_to(label.as_str(), "power-state", &ps);
                    last.insert(label, ps);
                }
            }
            crate::audio::PAUSED.store(any && all_hidden && COVER_PAUSE.load(Ordering::Relaxed), Ordering::Relaxed);
        }
    });
}

/// The state right now for one of our windows (asked for when a page loads).
pub fn current(app: &AppHandle, label: &str) -> PowerState {
    let (bat, saver) = battery();
    let cover = scan();
    let mon = if label == "main" { Some(desktop::target_id()) } else { app.get_webview_window(label).and_then(|w| desktop::mirror_monitor(crate::raw_of(&w))) };
    let c = mon.as_ref().and_then(|m| cover.get(m).copied()).unwrap_or_default();
    let lifted = label == "main" && desktop::current_mode() == desktop::Mode::Peek;
    PowerState { battery: bat, saver, full: c.full && !lifted, max: c.max && !lifted, monitor: mon.unwrap_or_default() }
}
