//! System volume (default output) for the volume slider when music plays in another app.
//!
//! * Linux: PipeWire's `wpctl`, else PulseAudio's `pactl` (PipeWire answers it too), else ALSA's `amixer`.
//! * macOS: `osascript` (Standard Additions: no permission prompt).

use std::process::Command;

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

// ---------------------------------------------------------------- parsing (shared by the tests)

/// `wpctl get-volume`: "Volume: 0.45" or "Volume: 0.45 [MUTED]".
#[allow(dead_code)]
fn parse_wpctl(s: &str) -> Option<(f32, bool)> {
    let v = s.split("Volume:").nth(1)?.split_whitespace().next()?.parse::<f32>().ok()?;
    Some((v.clamp(0.0, 1.5), s.contains("[MUTED]")))
}

/// `pactl get-sink-volume`: "Volume: front-left: 29491 /  45% / -20.81 dB, …" (first percentage).
#[allow(dead_code)]
fn parse_pactl_volume(s: &str) -> Option<f32> {
    let pct = s.split('%').next()?.rsplit(|c: char| !c.is_ascii_digit()).next()?;
    Some(pct.parse::<f32>().ok()? / 100.0)
}

/// `amixer get Master`: "… [45%] [on]".
#[allow(dead_code)]
fn parse_amixer(s: &str) -> Option<(f32, bool)> {
    let line = s.lines().find(|l| l.contains('%'))?;
    let pct = line.split('[').nth(1)?.split('%').next()?.parse::<f32>().ok()?;
    Some((pct / 100.0, line.contains("[off]")))
}

// ---------------------------------------------------------------- Linux

#[cfg(target_os = "linux")]
pub fn get() -> Result<(f32, bool), String> {
    if let Some(v) = run("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"]).as_deref().and_then(parse_wpctl) {
        return Ok((v.0.min(1.0), v.1));
    }
    if let Some(v) = run("pactl", &["get-sink-volume", "@DEFAULT_SINK@"]).as_deref().and_then(parse_pactl_volume) {
        let muted = run("pactl", &["get-sink-mute", "@DEFAULT_SINK@"]).map(|m| m.contains("yes")).unwrap_or(false);
        return Ok((v.min(1.0), muted));
    }
    if let Some(v) = run("amixer", &["get", "Master"]).as_deref().and_then(parse_amixer) {
        return Ok(v);
    }
    Err("no sound system found (wpctl / pactl / amixer)".into())
}

#[cfg(target_os = "linux")]
pub fn set(v: f32) -> Result<(), String> {
    let v = v.clamp(0.0, 1.0);
    if run("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{v:.3}")]).is_some() {
        let _ = run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", "0"]);
        return Ok(());
    }
    let pct = format!("{}%", (v * 100.0).round() as u32);
    if run("pactl", &["set-sink-volume", "@DEFAULT_SINK@", &pct]).is_some() {
        let _ = run("pactl", &["set-sink-mute", "@DEFAULT_SINK@", "0"]);
        return Ok(());
    }
    run("amixer", &["-q", "set", "Master", &pct, "unmute"]).map(|_| ()).ok_or_else(|| "couldn't set the volume".into())
}

#[cfg(target_os = "linux")]
pub fn mute(on: bool) -> Result<(), String> {
    let flag = if on { "1" } else { "0" };
    if run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", flag]).is_some() || run("pactl", &["set-sink-mute", "@DEFAULT_SINK@", flag]).is_some() {
        return Ok(());
    }
    run("amixer", &["-q", "set", "Master", if on { "mute" } else { "unmute" }]).map(|_| ()).ok_or_else(|| "couldn't mute".into())
}

// ---------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
fn osa(script: &str) -> Option<String> {
    run("osascript", &["-e", script]).map(|s| s.trim().to_string())
}

#[cfg(target_os = "macos")]
pub fn get() -> Result<(f32, bool), String> {
    // "45, true" (output volume 0…100 or "missing value" for outputs without a volume)
    let r = osa("set s to (get volume settings)\nreturn ((output volume of s) as text) & \",\" & ((output muted of s) as text)").ok_or("couldn't read the volume")?;
    let mut it = r.split(',');
    let v = it.next().and_then(|x| x.trim().parse::<f32>().ok()).ok_or("this output has no volume")?;
    let muted = it.next().map(|x| x.trim() == "true").unwrap_or(false);
    Ok(((v / 100.0).clamp(0.0, 1.0), muted))
}

#[cfg(target_os = "macos")]
pub fn set(v: f32) -> Result<(), String> {
    let pct = (v.clamp(0.0, 1.0) * 100.0).round() as u32;
    osa(&format!("set volume output volume {pct} without output muted")).map(|_| ()).ok_or_else(|| "couldn't set the volume".into())
}

#[cfg(target_os = "macos")]
pub fn mute(on: bool) -> Result<(), String> {
    osa(&format!("set volume output muted {on}")).map(|_| ()).ok_or_else(|| "couldn't mute".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tools() {
        assert_eq!(parse_wpctl("Volume: 0.45\n"), Some((0.45, false)));
        assert_eq!(parse_wpctl("Volume: 0.30 [MUTED]\n"), Some((0.30, true)));
        assert_eq!(parse_pactl_volume("Volume: front-left: 29491 /  45% / -20.81 dB,   front-right: 29491 /  45% / -20.81 dB\n"), Some(0.45));
        assert_eq!(parse_amixer("Simple mixer control 'Master',0\n  Front Left: Playback 40 [62%] [-12.00dB] [on]\n"), Some((0.62, false)));
    }
}
