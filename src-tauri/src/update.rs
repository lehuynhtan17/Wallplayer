//! Self-update for the portable app: no installer.
//!
//! Once per start (when "check when Wallplayer starts" is on) the app asks GitHub
//! quietly for the latest release; a newer one only changes the tray menu item to
//! "Update available: vX" (unless the user skipped that version). Clicking it, or
//! "Check for updates", shows the release notes with Skip / Update. Update
//! downloads `Wallplayer-Windows-x64.zip`, takes the files out and swaps them in
//! next to the running exe: Windows lets a running exe be renamed (not
//! overwritten), so the running one becomes `Wallplayer.exe.old` and the new one
//! takes its name, and the new version is started; `.old` files are removed on the next start.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;

const REPO: &str = "lehuynhtan17/Wallplayer";
const ASSET: &str = "Wallplayer-Windows-x64.zip";
const UA: &str = concat!("Wallplayer/", env!("CARGO_PKG_VERSION"));

#[derive(Serialize, Clone, Default, Debug)]
pub struct UpdateStatus {
    /// version running now
    pub current: String,
    /// automatic updates on
    pub auto: bool,
    /// "idle" | "checking" | "latest" | "available" | "downloading" | "ready" | "error"
    pub state: String,
    /// the newer version (when found)
    pub version: String,
    /// its release notes
    pub notes: String,
    /// its release page
    pub url: String,
    pub error: String,
    /// this start is the first one after an update
    pub just_updated: bool,
}

static STATUS: Mutex<Option<UpdateStatus>> = Mutex::new(None);
/// The newer release found by the last check, waiting for the user's yes.
static PENDING: Mutex<Option<Release>> = Mutex::new(None);
/// Path of the exe as started, read before anything is renamed.
static EXE: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();

fn exe() -> Option<PathBuf> {
    EXE.get_or_init(|| std::env::current_exe().ok()).clone()
}

pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn ver(s: &str) -> (u32, u32, u32) {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let mut it = s.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).map(|p| p.parse::<u32>().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(0), it.next().unwrap_or(0))
}

fn newer(tag: &str) -> bool {
    ver(tag) > ver(current())
}

pub fn status(app: &AppHandle) -> UpdateStatus {
    let mut st = STATUS.lock().unwrap().clone().unwrap_or_else(|| UpdateStatus { state: "idle".into(), ..Default::default() });
    st.current = current().to_string();
    st.auto = app.state::<AppState>().settings.lock().unwrap().auto_update;
    st
}

fn set(app: &AppHandle, f: impl FnOnce(&mut UpdateStatus)) {
    {
        let mut g = STATUS.lock().unwrap();
        let st = g.get_or_insert_with(|| UpdateStatus { state: "idle".into(), ..Default::default() });
        f(st);
    }
    let _ = app.emit_to("main", "update-state", status(app));
    refresh_tray(app);
}

/// Tray menu item + tooltip: "Update available: vX" while a newer, not skipped, version waits.
pub fn refresh_tray(app: &AppHandle) {
    let st = status(app);
    let skip = app.state::<AppState>().settings.lock().unwrap().skip_version.clone();
    let waiting = matches!(st.state.as_str(), "available" | "downloading" | "ready") && !st.version.is_empty() && st.version != skip;
    let vi = crate::UI_VI.load(std::sync::atomic::Ordering::Relaxed);
    let label = match (waiting, vi) {
        (true, false) => format!("Update available: v{}", st.version),
        (true, true) => format!("Có bản mới: v{}", st.version),
        (false, false) => format!("Check for updates   (v{})", current()),
        (false, true) => format!("Kiểm tra cập nhật   (v{})", current()),
    };
    if let Some(item) = app.state::<AppState>().update_item.lock().unwrap().as_ref() {
        let _ = item.set_text(label);
    }
    if let Some(tray) = app.tray_by_id("wallplayer") {
        let tip = match (waiting, vi) {
            (true, false) => format!("Wallplayer · update available (v{})", st.version),
            (true, true) => format!("Wallplayer · có bản mới (v{})", st.version),
            _ => "Wallplayer".to_string(),
        };
        let _ = tray.set_tooltip(Some(tip));
    }
}

#[derive(Clone)]
struct Release {
    tag: String,
    notes: String,
    page: String,
    zip: String,
    size: u64,
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder().user_agent(UA).timeout(Duration::from_secs(60)).build().map_err(|e| e.to_string())
}

/// The newest release. GitHub's API allows only 60 questions an hour per internet address without an
/// account, and many people can share one address (a VPN such as Cloudflare WARP, a mobile network,
/// an office): then it answers 403. The release page itself has no such limit, so ask it instead.
async fn latest() -> Result<Release, String> {
    match latest_api().await {
        Ok(r) => Ok(r),
        Err(e) if e.starts_with("No public release") => Err(e),
        Err(e) => latest_web().await.map_err(|e2| {
            if e.contains("403") || e.contains("429") {
                format!("GitHub is limiting update checks from your network right now. Try again later ({e2})")
            } else {
                e
            }
        }),
    }
}

/// Without the API: /releases/latest redirects to /releases/tag/<tag>; the notes come from that tag's
/// CHANGELOG.md, the size from the download itself.
async fn latest_web() -> Result<Release, String> {
    let c = reqwest::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let r = c.get(format!("https://github.com/{REPO}/releases/latest")).send().await.map_err(|e| e.to_string())?;
    let page = r.headers().get(reqwest::header::LOCATION).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let tag = page.split("/releases/tag/").nth(1).unwrap_or("").to_string();
    if tag.is_empty() || !tag.chars().all(|ch| ch.is_ascii_alphanumeric() || ".-_".contains(ch)) {
        return Err(format!("the release page answered {}", r.status()));
    }
    let ver = tag.trim_start_matches(['v', 'V']);
    let notes = match client()?.get(format!("https://raw.githubusercontent.com/{REPO}/{tag}/CHANGELOG.md")).send().await {
        Ok(r) if r.status().is_success() => {
            let md = r.text().await.unwrap_or_default();
            md.split(&format!("## {ver}\n")).nth(1).map(|t| t.split("\n## ").next().unwrap_or("").trim().to_string()).unwrap_or_default()
        }
        _ => String::new(),
    };
    Ok(Release { zip: format!("https://github.com/{REPO}/releases/download/{tag}/{ASSET}"), tag, notes, page, size: 0 })
}

async fn latest_api() -> Result<Release, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let r = client()?.get(url).header("Accept", "application/vnd.github+json").send().await.map_err(|e| e.to_string())?;
    if r.status().as_u16() == 404 {
        return Err("No public release found".into());
    }
    if !r.status().is_success() {
        return Err(format!("GitHub answered {}", r.status()));
    }
    let v: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
    let asset = v["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["name"].as_str() == Some(ASSET)))
        .ok_or("The release has no Windows zip")?;
    Ok(Release {
        tag: v["tag_name"].as_str().unwrap_or("").to_string(),
        notes: v["body"].as_str().unwrap_or("").to_string(),
        page: v["html_url"].as_str().unwrap_or("").to_string(),
        zip: asset["browser_download_url"].as_str().unwrap_or("").to_string(),
        size: asset["size"].as_u64().unwrap_or(0),
    })
}

fn side(path: &Path, ext: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(ext);
    PathBuf::from(s)
}

/// Put `data` in place of `dest`, which may be in use (exe, dll): rename the old one aside first.
fn swap_in(dest: &Path, data: &[u8]) -> Result<(), String> {
    if std::fs::read(dest).map(|d| d == data).unwrap_or(false) {
        return Ok(()); // unchanged
    }
    let new = side(dest, ".new");
    let old = side(dest, ".old");
    std::fs::write(&new, data).map_err(|e| format!("Can't write in the app folder ({e})"))?;
    if dest.exists() {
        let _ = std::fs::remove_file(&old);
        if let Err(e) = std::fs::rename(dest, &old) {
            let _ = std::fs::remove_file(&new);
            return Err(format!("Can't replace {} ({e})", dest.display()));
        }
    }
    if let Err(e) = std::fs::rename(&new, dest) {
        let _ = std::fs::rename(&old, dest); // roll back
        return Err(format!("Can't replace {} ({e})", dest.display()));
    }
    Ok(())
}

async fn download_and_stage(rel: &Release) -> Result<(), String> {
    // only this repo's own release files
    let prefix = format!("https://github.com/{REPO}/releases/download/");
    if !rel.zip.starts_with(&prefix) {
        return Err("Unexpected download address".into());
    }
    let r = client()?.get(&rel.zip).send().await.map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("Download failed ({})", r.status()));
    }
    // size from the API, or (found through the release page) from the download itself
    let size = if rel.size > 0 { rel.size } else { r.content_length().unwrap_or(0) };
    if size == 0 || size > 80 * 1024 * 1024 {
        return Err("Unexpected download size".into());
    }
    let bytes = r.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() as u64 != size {
        return Err("Download was incomplete".into());
    }

    // take the files out of the zip (they sit in a "Wallplayer/" folder)
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.as_ref())).map_err(|e| format!("Bad zip ({e})"))?;
    let mut new_exe: Option<Vec<u8>> = None;
    let mut others: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
        if f.is_dir() {
            continue;
        }
        let name = f.name().rsplit(['/', '\\']).next().unwrap_or("").to_string();
        let lower = name.to_ascii_lowercase();
        if !(lower == "wallplayer.exe" || lower == "webview2loader.dll" || lower == "huong-dan.txt" || lower == "guide.txt") {
            continue;
        }
        let mut buf = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        if lower == "wallplayer.exe" {
            new_exe = Some(buf);
        } else {
            others.push((name, buf));
        }
    }
    let new_exe = new_exe.ok_or("The zip has no Wallplayer.exe")?;
    if new_exe.len() < 1024 * 1024 || &new_exe[..2] != b"MZ" {
        return Err("The new Wallplayer.exe looks broken".into());
    }

    let me = exe().ok_or("No app path")?;
    let dir = me.parent().ok_or("No app folder")?.to_path_buf();
    swap_in(&me, &new_exe)?;
    for (name, data) in others {
        let _ = swap_in(&dir.join(name), &data); // the exe is what matters
    }
    Ok(())
}

fn busy() -> bool {
    STATUS.lock().unwrap().as_ref().map(|s| s.state == "checking" || s.state == "downloading").unwrap_or(false)
}

/// Ask GitHub whether there is a newer release. Downloads nothing.
pub async fn check(app: AppHandle) -> UpdateStatus {
    {
        let g = STATUS.lock().unwrap();
        if let Some(st) = g.as_ref() {
            // already found and downloaded, or busy: just report
            if st.state == "ready" || st.state == "checking" || st.state == "downloading" {
                drop(g);
                return status(&app);
            }
        }
    }
    set(&app, |s| {
        s.state = "checking".into();
        s.error.clear();
    });
    match latest().await {
        Err(e) => set(&app, |s| {
            s.state = "error".into();
            s.error = e;
        }),
        Ok(rel) if !newer(&rel.tag) => set(&app, |s| s.state = "latest".into()),
        Ok(rel) => {
            let version = rel.tag.trim_start_matches(['v', 'V']).to_string();
            let (notes, url) = (rel.notes.clone(), rel.page.clone());
            *PENDING.lock().unwrap() = Some(rel);
            set(&app, |s| {
                s.state = "available".into();
                s.version = version;
                s.notes = notes;
                s.url = url;
            });
        }
    }
    status(&app)
}

/// The user said yes: download the release found by `check` and swap it in.
pub async fn download(app: AppHandle) -> UpdateStatus {
    if busy() {
        return status(&app);
    }
    if PENDING.lock().unwrap().is_none() {
        check(app.clone()).await;
    }
    let Some(rel) = PENDING.lock().unwrap().clone() else { return status(&app) };
    set(&app, |s| {
        s.state = "downloading".into();
        s.error.clear();
    });
    match download_and_stage(&rel).await {
        Ok(()) => set(&app, |s| s.state = "ready".into()),
        Err(e) => set(&app, |s| {
            s.state = "error".into();
            s.error = e;
        }),
    }
    status(&app)
}

/// A quiet check a little after start (when turned on), then tidy up after the
/// previous version, which has surely exited by now.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(12)).await;
        let auto = app.state::<AppState>().settings.lock().unwrap().auto_update;
        if auto {
            check(app.clone()).await;
        }
        tokio::time::sleep(Duration::from_secs(8)).await;
        cleanup();
    });
}

/// Remove what the last update left behind.
pub fn cleanup() {
    if let Some(me) = exe() {
        let _ = std::fs::remove_file(side(&me, ".old"));
        let _ = std::fs::remove_file(side(&me, ".new"));
        if let Some(dir) = me.parent() {
            for n in ["WebView2Loader.dll", "HUONG-DAN.txt", "GUIDE.txt"] {
                let p = dir.join(n);
                let _ = std::fs::remove_file(side(&p, ".old"));
                let _ = std::fs::remove_file(side(&p, ".new"));
            }
        }
    }
}

/// Start the new exe (it waits for this one to exit) and quit.
pub fn restart(app: &AppHandle) -> Result<(), String> {
    let me = exe().ok_or("No app path")?;
    std::process::Command::new(&me)
        .arg("--wait-pid")
        .arg(std::process::id().to_string())
        .spawn()
        .map_err(|e| e.to_string())?;
    crate::quit(app);
    Ok(())
}

/// Started by `restart`: wait (up to 10 s) for the old process to be gone, so the
/// two never fight over the desktop, the tray or the shortcut.
pub fn wait_for_previous() {
    let _ = exe(); // remember our path now
    let args: Vec<String> = std::env::args().collect();
    let Some(i) = args.iter().position(|a| a == "--wait-pid") else { return };
    let Some(pid) = args.get(i + 1).and_then(|p| p.parse::<u32>().ok()) else { return };
    unsafe {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
        if let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            let _ = WaitForSingleObject(h, 10_000);
            let _ = CloseHandle(h);
        }
    }
    cleanup();
}

/// First start after an update? (remembers the version it last ran as)
pub fn mark_started(app: &AppHandle) {
    let st = app.state::<AppState>();
    let mut s = st.settings.lock().unwrap();
    let was = s.last_version.clone();
    if was != current() {
        s.last_version = current().to_string();
        crate::settings::save(app, &s);
        if !was.is_empty() {
            drop(s);
            let mut g = STATUS.lock().unwrap();
            g.get_or_insert_with(|| UpdateStatus { state: "idle".into(), ..Default::default() }).just_updated = true;
        }
    }
}
