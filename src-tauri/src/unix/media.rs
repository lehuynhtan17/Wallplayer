//! System "Now Playing" on Linux and macOS (Windows: ../media.rs, the media sessions).
//!
//! * Linux: MPRIS over D-Bus — what Spotify, Firefox, Chrome, VLC, Rhythmbox, Elisa… publish
//!   (the same source as the desktop's media controls).
//! * macOS: the system's Now Playing list is closed to other apps, so Wallplayer asks Spotify and
//!   Apple Music themselves (AppleScript). macOS asks once whether Wallplayer may control them.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone, Default, PartialEq, Debug)]
pub struct MediaState {
    pub has: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub app: String,
    pub playing: bool,
    pub position_ms: i64,
    pub duration_ms: i64,
    /// Unix ms at which `position_ms` was sampled.
    pub updated_ms: i64,
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// The cover as a data: URL (local file or web address).
fn art_data(url: &str) -> Option<String> {
    if url.is_empty() {
        return None;
    }
    if url.starts_with("https://") || url.starts_with("http://") {
        let u = url.to_string();
        return tauri::async_runtime::block_on(async move { crate::net::fetch_image(&u).await.ok() });
    }
    let path = url.strip_prefix("file://").map(|p| urlencoding::decode(p).map(|c| c.into_owned()).unwrap_or_else(|_| p.to_string())).unwrap_or_else(|| url.to_string());
    let bytes = std::fs::read(&path).ok().filter(|b| !b.is_empty() && b.len() < 20_000_000)?;
    let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) { "image/png" } else { "image/jpeg" };
    Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes)))
}

/// Background poller: emits `media` (MediaState) and `media-art` (data URL).
pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last = MediaState::default();
        let mut last_key = String::new();
        loop {
            let (st, art) = sys::read();
            if st.has {
                let key = format!("{}\u{1}{}\u{1}{}", st.title, st.artist, st.album);
                if key != last_key {
                    last_key = key;
                    let _ = app.emit("media-art", art.as_deref().and_then(art_data));
                }
            }
            // the position moves by itself while playing: only a real change is news
            let mut cmp = st.clone();
            cmp.updated_ms = last.updated_ms;
            if st.playing && last.playing && cmp.title == last.title {
                let expected = last.position_ms + (st.updated_ms - last.updated_ms);
                if (st.position_ms - expected).abs() < 1500 {
                    cmp.position_ms = last.position_ms;
                }
            }
            if cmp != last {
                let _ = app.emit("media", &st);
                last = st;
            }
            std::thread::sleep(Duration::from_millis(if cfg!(target_os = "macos") { 1500 } else { 500 }));
        }
    });
}

/// Control the app that plays now.
pub fn control(action: String, value: Option<f64>) -> Result<(), String> {
    sys::control(&action, value)
}

// ---------------------------------------------------------------- Linux: MPRIS

#[cfg(target_os = "linux")]
mod sys {
    use gio::prelude::*;
    use glib::{Variant, VariantDict};

    use super::{now_ms, MediaState};

    const PREFIX: &str = "org.mpris.MediaPlayer2.";
    const PATH: &str = "/org/mpris/MediaPlayer2";
    const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

    fn bus() -> Option<gio::DBusConnection> {
        thread_local!(static BUS: Option<gio::DBusConnection> = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok());
        BUS.with(|b| b.clone())
    }

    fn call(c: &gio::DBusConnection, dest: &str, path: &str, iface: &str, method: &str, args: Option<&Variant>) -> Result<Variant, String> {
        c.call_sync(Some(dest), path, iface, method, args, None, gio::DBusCallFlags::NONE, 1500, gio::Cancellable::NONE).map_err(|e| e.to_string())
    }

    /// MPRIS players other than ours: bus name.
    fn players(c: &gio::DBusConnection) -> Vec<String> {
        let me = std::process::id();
        let Ok(r) = call(c, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "ListNames", None) else { return vec![] };
        let names: Vec<String> = r.child_value(0).get().unwrap_or_default();
        names
            .into_iter()
            .filter(|n| n.starts_with(PREFIX))
            .filter(|n| {
                let pid = call(c, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "GetConnectionUnixProcessID", Some(&(n.as_str(),).to_variant()))
                    .ok()
                    .and_then(|v| v.child_value(0).get::<u32>());
                pid != Some(me) && !n.to_lowercase().contains("wallplayer")
            })
            .collect()
    }

    fn all(c: &gio::DBusConnection, name: &str, iface: &str) -> Option<VariantDict> {
        let r = call(c, name, PATH, "org.freedesktop.DBus.Properties", "GetAll", Some(&(iface,).to_variant())).ok()?;
        Some(VariantDict::new(Some(&r.child_value(0))))
    }

    fn int(v: &Variant) -> Option<i64> {
        v.get::<i64>().or_else(|| v.get::<u64>().map(|x| x as i64)).or_else(|| v.get::<i32>().map(|x| x as i64)).or_else(|| v.get::<f64>().map(|x| x as i64))
    }

    struct Player {
        name: String,
        st: MediaState,
        art: Option<String>,
        track: Option<String>,
        shuffle: Option<bool>,
    }

    fn read_one(c: &gio::DBusConnection, name: &str) -> Option<Player> {
        let p = all(c, name, PLAYER)?;
        let status: String = p.lookup_value("PlaybackStatus", None).and_then(|v| v.get()).unwrap_or_default();
        let meta = p.lookup_value("Metadata", None).map(|v| VariantDict::new(Some(&v)))?;
        let s = |k: &str| -> String { meta.lookup_value(k, None).and_then(|v| v.get::<String>()).unwrap_or_default() };
        let artist = meta
            .lookup_value("xesam:artist", None)
            .and_then(|v| v.get::<Vec<String>>().map(|a| a.join(", ")).or_else(|| v.get::<String>()))
            .unwrap_or_default();
        let identity: String = all(c, name, "org.mpris.MediaPlayer2").and_then(|d| d.lookup_value("Identity", None)).and_then(|v| v.get()).unwrap_or_default();
        let short = name.trim_start_matches(PREFIX).split('.').next().unwrap_or("").to_string();
        let st = MediaState {
            has: !s("xesam:title").is_empty(),
            title: s("xesam:title"),
            artist,
            album: s("xesam:album"),
            app: if identity.is_empty() { short } else { identity },
            playing: status == "Playing",
            position_ms: p.lookup_value("Position", None).as_ref().and_then(int).unwrap_or(0).max(0) / 1000,
            duration_ms: meta.lookup_value("mpris:length", None).as_ref().and_then(int).unwrap_or(0).max(0) / 1000,
            updated_ms: now_ms(),
        };
        let track = meta.lookup_value("mpris:trackid", None).and_then(|v| v.str().map(String::from));
        let shuffle = p.lookup_value("Shuffle", None).and_then(|v| v.get::<bool>());
        Some(Player { name: name.to_string(), st, art: Some(s("mpris:artUrl")).filter(|a| !a.is_empty()), track, shuffle })
    }

    /// A playing player first, else the first one with a song.
    fn pick() -> Option<Player> {
        let c = bus()?;
        let mut fallback = None;
        for n in players(&c) {
            if let Some(p) = read_one(&c, &n) {
                if !p.st.has {
                    continue;
                }
                if p.st.playing {
                    return Some(p);
                }
                fallback.get_or_insert(p);
            }
        }
        fallback
    }

    pub fn read() -> (MediaState, Option<String>) {
        match pick() {
            Some(p) => (p.st, p.art),
            None => (MediaState::default(), None),
        }
    }

    pub fn control(action: &str, value: Option<f64>) -> Result<(), String> {
        let c = bus().ok_or("no session bus")?;
        let p = pick().ok_or("nothing is playing")?;
        let simple = |m: &str| call(&c, &p.name, PATH, PLAYER, m, None).map(|_| ());
        match action {
            "toggle" => simple("PlayPause"),
            "play" => simple("Play"),
            "pause" => simple("Pause"),
            "next" => simple("Next"),
            "prev" => simple("Previous"),
            "seek" => {
                let us = (value.unwrap_or(0.0).max(0.0) * 1_000_000.0) as i64;
                match p.track.as_deref().and_then(|t| glib::variant::ObjectPath::try_from(t.to_string()).ok()) {
                    Some(path) => call(&c, &p.name, PATH, PLAYER, "SetPosition", Some(&(path, us).to_variant())).map(|_| ()),
                    None => call(&c, &p.name, PATH, PLAYER, "Seek", Some(&(us - p.st.position_ms * 1000,).to_variant())).map(|_| ()),
                }
            }
            "shuffle" => {
                let on = !p.shuffle.unwrap_or(false);
                call(&c, &p.name, PATH, "org.freedesktop.DBus.Properties", "Set", Some(&(PLAYER, "Shuffle", on.to_variant()).to_variant())).map(|_| ())
            }
            _ => Err(format!("unknown action {action}")),
        }
    }
}

// ---------------------------------------------------------------- macOS: Spotify / Apple Music

#[cfg(target_os = "macos")]
mod sys {
    use std::process::Command;
    use std::sync::Mutex;

    use super::{now_ms, MediaState};

    /// (process name, AppleScript application name, duration unit in ms)
    const APPS: [(&str, &str, f64); 2] = [("Spotify", "Spotify", 1.0), ("Music", "Music", 1000.0)];
    /// apps macOS refused to let us control (asked once; the answer is in System Settings)
    static DENIED: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    static LAST: Mutex<&'static str> = Mutex::new("");

    fn running(proc_name: &str) -> bool {
        Command::new("pgrep").args(["-x", proc_name]).output().map(|o| o.status.success()).unwrap_or(false)
    }

    fn osa(script: &str) -> Result<String, String> {
        let out = Command::new("osascript").args(["-e", script]).output().map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).into_owned())
        }
    }

    fn num(s: &str) -> f64 {
        s.trim().replace(',', ".").parse::<f64>().unwrap_or(0.0)
    }

    fn query(app: &'static str, unit: f64) -> Option<(MediaState, Option<String>)> {
        let (art, shuffle) = if app == "Spotify" { ("(artwork url of t)", "shuffling") } else { ("\"\"", "shuffle enabled") };
        let script = format!(
            "tell application \"{app}\"\nset st to (player state as text)\nif st is \"stopped\" then return st\nset t to current track\nreturn st & linefeed & (name of t) & linefeed & (artist of t) & linefeed & (album of t) & linefeed & ((player position) as text) & linefeed & ((duration of t) as text) & linefeed & {art} & linefeed & (({shuffle}) as text)\nend tell"
        );
        match osa(&script) {
            Ok(r) => {
                let l: Vec<&str> = r.split('\n').collect();
                if l.len() < 6 {
                    return Some((MediaState::default(), None));
                }
                let st = MediaState {
                    has: !l[1].is_empty(),
                    title: l[1].to_string(),
                    artist: l[2].to_string(),
                    album: l[3].to_string(),
                    app: app.to_string(),
                    playing: l[0] == "playing",
                    position_ms: (num(l[4]) * 1000.0) as i64,
                    duration_ms: (num(l[5]) * unit) as i64,
                    updated_ms: now_ms(),
                };
                Some((st, l.get(6).map(|s| s.to_string()).filter(|s| s.starts_with("http"))))
            }
            Err(e) => {
                // -1743: not allowed to control it (the user said no)
                if e.contains("-1743") {
                    DENIED.lock().unwrap().push(app);
                }
                None
            }
        }
    }

    pub fn read() -> (MediaState, Option<String>) {
        let mut fallback: Option<(MediaState, Option<String>, &'static str)> = None;
        for (proc_name, app, unit) in APPS {
            if DENIED.lock().unwrap().contains(&app) || !running(proc_name) {
                continue;
            }
            if let Some((st, art)) = query(app, unit) {
                if st.has && st.playing {
                    *LAST.lock().unwrap() = app;
                    return (st, art);
                }
                if st.has && fallback.is_none() {
                    fallback = Some((st, art, app));
                }
            }
        }
        match fallback {
            Some((st, art, app)) => {
                *LAST.lock().unwrap() = app;
                (st, art)
            }
            None => {
                *LAST.lock().unwrap() = "";
                (MediaState::default(), None)
            }
        }
    }

    pub fn control(action: &str, value: Option<f64>) -> Result<(), String> {
        let app = *LAST.lock().unwrap();
        if app.is_empty() {
            return Err("nothing is playing".into());
        }
        let cmd = match action {
            "toggle" => "playpause".to_string(),
            "play" => "play".to_string(),
            "pause" => "pause".to_string(),
            "next" => "next track".to_string(),
            "prev" => "previous track".to_string(),
            "seek" => format!("set player position to {}", value.unwrap_or(0.0).max(0.0)),
            "shuffle" if app == "Spotify" => "set shuffling to not shuffling".to_string(),
            "shuffle" => "set shuffle enabled to not shuffle enabled".to_string(),
            _ => return Err(format!("unknown action {action}")),
        };
        osa(&format!("tell application \"{app}\" to {cmd}")).map(|_| ())
    }
}
