//! "Start at login" on Linux and macOS (Windows uses the Run list / Task Scheduler, see settings.rs).
//!
//! * Linux: an XDG autostart entry, `~/.config/autostart/wallplayer.desktop` (GNOME, KDE, Xfce,
//!   Cinnamon, MATE… all read it).
//! * macOS: a LaunchAgent, `~/Library/LaunchAgents/com.tan.wallplayer.plist`, loaded when you log in.
//!
//! Both start Wallplayer with `--autostart`, like the Windows Run entry. "High priority" (a Windows
//! Task Scheduler task) does not exist here.

use std::path::PathBuf;

use super::AUTOSTART_ARG;

/// The program to start: the AppImage itself when running from one (its inside is mounted under
/// /tmp at a new path each time), otherwise this executable.
fn program() -> Option<String> {
    #[cfg(target_os = "linux")]
    if let Ok(p) = std::env::var("APPIMAGE") {
        if !p.is_empty() {
            return Some(p);
        }
    }
    std::env::current_exe().ok().map(|p| p.display().to_string())
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)
}

// ---------------------------------------------------------------- Linux

#[cfg(target_os = "linux")]
fn entry_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".config")))?;
    Some(base.join("autostart").join("wallplayer.desktop"))
}

/// One argument of an `Exec=` line (Desktop Entry spec: quoted, with \ " ` $ escaped, % doubled).
#[cfg(target_os = "linux")]
fn exec_arg(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' | '"' | '`' | '$' => {
                out.push('\\');
                out.push(c);
            }
            '%' => out.push_str("%%"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The program an `Exec=` line starts (first argument, quotes and escapes undone).
#[cfg(target_os = "linux")]
fn exec_program(line: &str) -> String {
    let l = line.trim();
    if let Some(rest) = l.strip_prefix('"') {
        let mut out = String::new();
        let mut it = rest.chars();
        while let Some(c) = it.next() {
            match c {
                '\\' => {
                    if let Some(n) = it.next() {
                        out.push(n);
                    }
                }
                '"' => break,
                _ => out.push(c),
            }
        }
        out.replace("%%", "%")
    } else {
        l.split_whitespace().next().unwrap_or("").replace("%%", "%")
    }
}

#[cfg(target_os = "linux")]
fn entry_text(prog: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Wallplayer\nComment=Desktop wallpaper music player\nExec={} {AUTOSTART_ARG}\nTerminal=false\nNoDisplay=false\nHidden=false\nX-GNOME-Autostart-enabled=true\nX-GNOME-Autostart-Delay=2\n",
        exec_arg(prog)
    )
}

/// (switched on, program it starts) of an autostart entry.
#[cfg(target_os = "linux")]
fn read_entry(text: &str) -> (bool, String) {
    let mut on = true;
    let mut prog = String::new();
    for l in text.lines() {
        let l = l.trim();
        if let Some(v) = l.strip_prefix("Exec=") {
            prog = exec_program(v);
        } else if l.eq_ignore_ascii_case("Hidden=true") || l.eq_ignore_ascii_case("X-GNOME-Autostart-enabled=false") {
            on = false;
        }
    }
    (on, prog)
}

// ---------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
const AGENT: &str = "com.tan.wallplayer";

#[cfg(target_os = "macos")]
fn entry_path() -> Option<PathBuf> {
    Some(home()?.join("Library").join("LaunchAgents").join(format!("{AGENT}.plist")))
}

#[cfg(target_os = "macos")]
fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(target_os = "macos")]
fn entry_text(prog: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{AGENT}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>{AUTOSTART_ARG}</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><false/>
  <key>ProcessType</key><string>Interactive</string>
  <key>LimitLoadToSessionType</key><string>Aqua</string>
</dict>
</plist>
"#,
        xml(prog)
    )
}

#[cfg(target_os = "macos")]
fn read_entry(text: &str) -> (bool, String) {
    let unxml = |s: &str| s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&");
    // the first <string> after ProgramArguments
    let prog = text
        .split("<key>ProgramArguments</key>")
        .nth(1)
        .and_then(|t| t.split("<string>").nth(1))
        .and_then(|t| t.split("</string>").next())
        .map(|t| unxml(t.trim()))
        .unwrap_or_default();
    let off = text.split("<key>Disabled</key>").nth(1).map(|t| t.trim_start().starts_with("<true/>")).unwrap_or(false);
    (!off, prog)
}

// ---------------------------------------------------------------- both

fn same(a: &str, b: &str) -> bool {
    let canon = |s: &str| std::fs::canonicalize(s).map(|p| p.display().to_string()).unwrap_or_else(|_| s.to_string());
    a == b || canon(a) == canon(b)
}

fn write_entry() -> std::io::Result<()> {
    let p = entry_path().ok_or_else(|| std::io::Error::other("no home folder"))?;
    let prog = program().ok_or_else(|| std::io::Error::other("no app path"))?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&p, entry_text(&prog))
}

fn remove_entry() -> std::io::Result<()> {
    let Some(p) = entry_path() else { return Ok(()) };
    match std::fs::remove_file(p) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// On only when the login start is switched on and starts THIS copy of Wallplayer.
pub fn autostart_enabled() -> bool {
    let (Some(p), Some(me)) = (entry_path(), program()) else { return false };
    let Ok(text) = std::fs::read_to_string(p) else { return false };
    let (on, prog) = read_entry(&text);
    on && same(&prog, &me)
}

pub fn set_autostart(on: bool) -> std::io::Result<()> {
    if on {
        write_entry()
    } else {
        remove_entry()
    }
}

/// "High priority" is a Windows Task Scheduler start: not here. Turning it off keeps the normal start.
pub fn set_autostart_high(on: bool) -> Result<(), String> {
    if on {
        return Err("High priority is only available on Windows".into());
    }
    write_entry().map_err(|e| e.to_string())
}

/// At start: the login start points to another copy or folder (the app was moved) → this copy.
pub fn heal_autostart() -> Option<String> {
    let p = entry_path()?;
    let text = std::fs::read_to_string(&p).ok()?;
    let (on, old) = read_entry(&text);
    let me = program()?;
    if !on || (same(&old, &me) && text.contains(AUTOSTART_ARG)) {
        return None;
    }
    write_entry().ok()?;
    Some(format!("Start at login pointed to {old}; now points to this copy"))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn exec_line_round_trip() {
        for p in ["/opt/Wallplayer/wallplayer", "/home/a b/My \"Apps\"/wall$player 100%/wallplayer"] {
            let text = entry_text(p);
            let (on, prog) = read_entry(&text);
            assert!(on);
            assert_eq!(prog, p, "{text}");
        }
        assert_eq!(read_entry("Exec=/usr/bin/wallplayer --autostart\nHidden=true").1, "/usr/bin/wallplayer");
        assert!(!read_entry("Exec=x\nX-GNOME-Autostart-enabled=false").0);
    }
}
