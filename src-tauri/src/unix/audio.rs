//! System audio analysis for the ambient light (Windows: ../audio.rs, WASAPI loopback).
//!
//! * Linux: the "monitor" of the default output (what you hear), read from PulseAudio / PipeWire
//!   with `parec` (or PipeWire's `pw-record`) as 48 kHz float samples. Same analysis as on Windows:
//!   a 4096-point FFT every 512 samples, `audio-spectrum` ≈60×/s as `[rms, band0 … band31]`.
//! * macOS: apps can't hear the system's sound without the Screen Recording permission (or an extra
//!   audio driver), so nothing is sent and the light follows the page's own animation.

use std::sync::atomic::AtomicBool;

/// Set while every window is hidden behind another app: skip the analysis.
pub static PAUSED: AtomicBool = AtomicBool::new(false);

#[cfg(target_os = "macos")]
pub fn spawn(_app: tauri::AppHandle) {}

#[cfg(target_os = "linux")]
pub fn spawn(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut fails = 0u32;
        loop {
            let started = std::time::Instant::now();
            let _ = linux::run(&app);
            // a recorder that dies at once again and again (no sound server): try less often
            if started.elapsed() < std::time::Duration::from_secs(5) {
                fails += 1;
            } else {
                fails = 0;
            }
            std::thread::sleep(std::time::Duration::from_secs(if fails > 5 { 60 } else { 4 }));
        }
    });
}

#[cfg(target_os = "linux")]
mod linux {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use rustfft::num_complex::Complex;
    use rustfft::FftPlanner;
    use tauri::{AppHandle, Emitter};

    use super::PAUSED;
    use std::sync::atomic::Ordering;

    const N: usize = 4096;
    const HOP: usize = 512;
    const BANDS: usize = 32;
    const RATE: u32 = 48_000;

    fn band_edges(sample_rate: f32) -> Vec<usize> {
        let (lo, hi) = (40.0f32, 16_000.0f32.min(sample_rate * 0.45));
        let mut e: Vec<usize> = (0..=BANDS)
            .map(|k| {
                let f = lo * (hi / lo).powf(k as f32 / BANDS as f32);
                ((f / sample_rate * N as f32).round() as usize).clamp(1, N / 2 - 1)
            })
            .collect();
        for k in 1..e.len() {
            if e[k] <= e[k - 1] {
                e[k] = (e[k - 1] + 1).min(N / 2);
            }
        }
        e
    }

    /// The recorder: stereo float32 little-endian samples of the default output's monitor on stdout.
    fn recorder() -> Option<std::process::Child> {
        let rate = RATE.to_string();
        let tries: [(&str, Vec<&str>); 2] = [
            ("parec", vec!["--device=@DEFAULT_MONITOR@", "--format=float32le", "--rate", &rate, "--channels=2", "--latency-msec=20", "--raw"]),
            ("pw-record", vec!["-P", "{ stream.capture.sink=true }", "--format", "f32", "--rate", &rate, "--channels", "2", "-"]),
        ];
        for (cmd, args) in tries {
            if let Ok(child) = Command::new(cmd).args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() {
                return Some(child);
            }
        }
        None
    }

    fn default_sink() -> Option<String> {
        let o = Command::new("pactl").arg("get-default-sink").stderr(Stdio::null()).output().ok()?;
        o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
    }

    pub fn run(app: &AppHandle) -> Result<(), String> {
        let mut child = recorder().ok_or("no recorder (parec / pw-record)")?;
        let mut out_pipe = child.stdout.take().ok_or("no output")?;
        let pid = child.id();
        // start again on the new output when the default one changes (headphones…)
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        if let Some(sink) = default_sink() {
            let done2 = done.clone();
            std::thread::spawn(move || {
                while !done2.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_secs(3));
                    if default_sink().is_some_and(|s| s != sink) && !done2.load(Ordering::Relaxed) {
                        unsafe {
                            libc::kill(pid as i32, libc::SIGTERM);
                        }
                        break;
                    }
                }
            });
        }
        let r = analyse(app, &mut out_pipe);
        done.store(true, Ordering::Relaxed);
        let _ = child.kill();
        let _ = child.wait();
        r
    }

    fn analyse(app: &AppHandle, input: &mut impl Read) -> Result<(), String> {
        let sr = RATE as f32;
        let fft = FftPlanner::<f32>::new().plan_fft_forward(N);
        let window: Vec<f32> = (0..N).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (N - 1) as f32).cos()).collect();
        let edges = band_edges(sr);
        let mut ring = vec![0f32; N];
        let mut wpos = 0usize;
        let mut since = 0usize;
        let mut buf = vec![Complex { re: 0.0f32, im: 0.0 }; N];
        let mut scratch = vec![Complex { re: 0.0f32, im: 0.0 }; fft.get_inplace_scratch_len()];
        let mut out = vec![0f32; BANDS + 1];
        let (mut acc, mut nacc) = (0f32, 0usize);
        let mut last_emit = Instant::now();
        let mut peak = 0f32;
        let mut sent_silence = false;
        let mut bytes = vec![0u8; 8 * 256]; // 256 stereo frames
        loop {
            input.read_exact(&mut bytes).map_err(|e| e.to_string())?;
            if PAUSED.load(Ordering::Relaxed) {
                continue; // keep the pipe flowing, skip the work
            }
            for frame in bytes.chunks_exact(8) {
                let l = f32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]);
                let r = f32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
                let s = if l.is_finite() && r.is_finite() { (l + r) * 0.5 } else { 0.0 };
                ring[wpos] = s;
                wpos = (wpos + 1) % N;
                since += 1;
                acc += s * s;
                nacc += 1;
                if s.abs() > peak {
                    peak = s.abs();
                }
                if since < HOP {
                    continue;
                }
                since = 0;
                if peak < 1e-6 {
                    if !sent_silence && last_emit.elapsed() >= Duration::from_millis(16) {
                        out.iter_mut().for_each(|v| *v = 0.0);
                        let _ = app.emit("audio-spectrum", &out);
                        sent_silence = true;
                        acc = 0.0;
                        nacc = 0;
                        last_emit = Instant::now();
                    }
                    continue;
                }
                peak = 0.0;
                for i in 0..N {
                    buf[i] = Complex { re: ring[(wpos + i) % N] * window[i], im: 0.0 };
                }
                fft.process_with_scratch(&mut buf, &mut scratch);
                for b in 0..BANDS {
                    let (lo, hi) = (edges[b], edges[b + 1].max(edges[b] + 1));
                    let mut m = 0f32;
                    for k in lo..hi.min(N / 2) {
                        let v = buf[k].norm();
                        if v > m {
                            m = v;
                        }
                    }
                    let v = m * 2.0 / N as f32;
                    if v > out[b + 1] {
                        out[b + 1] = v;
                    }
                }
                let every = if crate::power::LOW.load(Ordering::Relaxed) { 33 } else { 16 };
                if last_emit.elapsed() >= Duration::from_millis(every) {
                    sent_silence = false;
                    out[0] = if nacc > 0 { (acc / nacc as f32).sqrt() } else { 0.0 };
                    let _ = app.emit("audio-spectrum", &out);
                    out.iter_mut().for_each(|v| *v = 0.0);
                    acc = 0.0;
                    nacc = 0;
                    last_emit = Instant::now();
                }
            }
        }
    }
}
