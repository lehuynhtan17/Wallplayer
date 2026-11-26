#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accounts;
// Windows: the files next to this one. Linux / macOS: the same modules (same functions) in `unix/`.
#[cfg_attr(not(windows), path = "unix/audio.rs")]
mod audio;
#[cfg_attr(not(windows), path = "unix/boot.rs")]
mod boot;
#[cfg_attr(not(windows), path = "unix/deskmouse.rs")]
mod deskmouse;
#[cfg_attr(not(windows), path = "unix/desktop.rs")]
mod desktop;
mod github;
mod guard;
mod island;
mod library;
#[cfg_attr(not(windows), path = "unix/media.rs")]
mod media;
mod mirror;
mod net;
#[cfg_attr(not(windows), path = "unix/power.rs")]
mod power;
mod settings;
mod tiktok;
mod update;
#[cfg_attr(not(windows), path = "unix/volume.rs")]
mod volume;
#[cfg(target_os = "linux")]
#[path = "unix/x11.rs"]
mod x11;
#[cfg(target_os = "macos")]
#[path = "unix/mac.rs"]
mod mac;

/// The native handle of one of our windows as a number: the HWND on Windows; on Linux / macOS a
/// small id the `desktop` module hands out (and turns back into the window).
#[cfg(windows)]
pub(crate) fn raw_of(w: &tauri::WebviewWindow) -> isize {
    w.hwnd().map(|h| h.0 as isize).unwrap_or(0)
}
#[cfg(not(windows))]
pub(crate) fn raw_of(w: &tauri::WebviewWindow) -> isize {
    desktop::raw_of(w)
}

/// Which system this build runs on, as the interface names it.
pub(crate) const OS: &str = if cfg!(windows) { "windows" } else if cfg!(target_os = "macos") { "macos" } else { "linux" };

use std::sync::Mutex;

use serde::Serialize;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub(crate) struct AppState {
    pub(crate) settings: Mutex<settings::Settings>,
    peek: Mutex<bool>,
    hwnd: Mutex<isize>,
    wall_item: Mutex<Option<CheckMenuItem<tauri::Wry>>>,
    icons_item: Mutex<Option<CheckMenuItem<tauri::Wry>>>,
    auto_item: Mutex<Option<CheckMenuItem<tauri::Wry>>>,
    pub(crate) update_item: Mutex<Option<MenuItem<tauri::Wry>>>,
    /// tray menu items whose text follows the interface language: (id, set_text)
    tray_texts: Mutex<Vec<(&'static str, Box<dyn Fn(&str) + Send + Sync>)>>,
    pub(crate) mirrors_installed: Mutex<Vec<isize>>,
}

#[derive(Serialize, Clone)]
struct UiState {
    wallpaper_mode: bool,
    /// desktop icons hidden: the player takes the mouse (shown: only the icons do)
    hide_icons: bool,
    autostart: bool,
    /// "Start with Windows" → "High priority" (the sign-in task)
    autostart_high: bool,
    peek: bool,
}

fn ui_state(app: &AppHandle) -> UiState {
    let st = app.state::<AppState>();
    let (wallpaper_mode, hide_icons) = {
        let s = st.settings.lock().unwrap();
        (s.wallpaper_mode, s.hide_icons)
    };
    let peek = *st.peek.lock().unwrap();
    UiState { wallpaper_mode, hide_icons, autostart: settings::autostart_enabled(), autostart_high: boot::high_on(), peek }
}

/// Apply the wanted placement (peek > wallpaper > desktop).
fn apply_mode(app: &AppHandle) {
    let st = app.state::<AppState>();
    let h = *st.hwnd.lock().unwrap();
    if h == 0 {
        return;
    }
    let peek = *st.peek.lock().unwrap();
    let wall = st.settings.lock().unwrap().wallpaper_mode;
    let mode = if peek {
        desktop::Mode::Peek
    } else if wall {
        desktop::Mode::Wallpaper
    } else {
        desktop::Mode::Desktop
    };
    if let Err(e) = desktop::set_mode(h, mode) {
        let _ = desktop::set_mode(h, desktop::Mode::Desktop);
        let _ = app.emit("toast", format!("Couldn't go behind icons: {e}"));
    }
    // Peek only lifts the main window; mirrors stay where they are.
    if !peek {
        desktop::place_mirrors(desktop::current_mode());
    }
    if peek {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.set_focus();
        }
    }
    let _ = app.emit("mode-changed", ui_state(app));
}

fn set_peek(app: &AppHandle, on: bool) {
    {
        let st = app.state::<AppState>();
        let mut p = st.peek.lock().unwrap();
        if *p == on {
            return;
        }
        *p = on;
    }
    apply_mode(app);
}

fn set_wallpaper_mode_inner(app: &AppHandle, on: bool) {
    {
        let st = app.state::<AppState>();
        let mut s = st.settings.lock().unwrap();
        s.wallpaper_mode = on;
        settings::save(app, &s);
        let item = st.wall_item.lock().unwrap().clone();
        if let Some(item) = item {
            let _ = item.set_checked(on);
        }
    }
    apply_mode(app);
}

/// "Hide desktop icons": on, the icons are hidden and the player takes the mouse; off, the icons show
/// and only they take the mouse (the player stays a moving wallpaper). Linux: the skin always covers
/// the file manager's icons, so this does nothing there.
fn set_icons_hidden_inner(app: &AppHandle, on: bool) {
    {
        let st = app.state::<AppState>();
        let mut s = st.settings.lock().unwrap();
        s.hide_icons = on;
        settings::save(app, &s);
        let item = st.icons_item.lock().unwrap().clone();
        if let Some(item) = item {
            let _ = item.set_checked(on);
        }
    }
    desktop::set_icons_hidden(on);
    let _ = app.emit("mode-changed", ui_state(app));
}

fn toggle_icons(app: &AppHandle) {
    if cfg!(target_os = "linux") {
        return;
    }
    let on = !app.state::<AppState>().settings.lock().unwrap().hide_icons;
    set_icons_hidden_inner(app, on);
    let _ = app.emit("icons-changed", on);
}

#[tauri::command]
fn set_icons_hidden(app: AppHandle, on: bool) -> UiState {
    if !cfg!(target_os = "linux") {
        set_icons_hidden_inner(&app, on);
    }
    ui_state(&app)
}

pub(crate) fn end_peek(app: &AppHandle) {
    set_peek(app, false);
    let _ = app.emit("peek-ended", ());
}

pub(crate) fn open_search(app: &AppHandle) {
    set_peek(app, true);
    let _ = app.emit("hotkey-search", ());
}

// ───────────────────────── commands ─────────────────────────

#[tauri::command]
fn get_state(app: AppHandle) -> UiState {
    ui_state(&app)
}

/// Settings → General → "Start with Windows" (the tray menu has the same switch).
#[tauri::command]
fn set_autostart(app: AppHandle, on: bool) -> UiState {
    let _ = settings::set_autostart(on);
    let item = app.state::<AppState>().auto_item.lock().unwrap().clone();
    if let Some(item) = item {
        let _ = item.set_checked(settings::autostart_enabled());
    }
    ui_state(&app)
}

/// Settings → Performance → "High priority": start as soon as you sign in, before the other startup apps.
#[tauri::command]
fn set_autostart_high(app: AppHandle, on: bool) -> Result<UiState, String> {
    let r = settings::set_autostart_high(on);
    if r.is_err() {
        let _ = settings::set_autostart(true); // at least the normal start keeps working
    }
    let item = app.state::<AppState>().auto_item.lock().unwrap().clone();
    if let Some(item) = item {
        let _ = item.set_checked(settings::autostart_enabled());
    }
    r.map(|_| ui_state(&app))
}

#[tauri::command]
fn set_wallpaper_mode(app: AppHandle, on: bool) -> UiState {
    set_wallpaper_mode_inner(&app, on);
    ui_state(&app)
}

#[tauri::command]
fn peek(app: AppHandle, on: bool) {
    set_peek(&app, on);
}

#[tauri::command]
async fn get_volume() -> Result<(f32, bool), String> {
    tauri::async_runtime::spawn_blocking(volume::get).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn set_volume(value: f32) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || volume::set(value)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn set_mute(on: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || volume::mute(on)).await.map_err(|e| e.to_string())?
}

#[derive(Serialize)]
struct Wallpaper {
    path: String,
    /// data: URL, only when the path differs from `known`
    data: Option<String>,
}

/// The Windows wallpaper of the monitor the calling window is on.
#[tauri::command]
async fn desktop_wallpaper(window: tauri::WebviewWindow, known: Option<String>) -> Result<Option<Wallpaper>, String> {
    let raw = raw_of(&window);
    if raw == 0 {
        return Err("no window handle".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = desktop::wallpaper_for(raw) else { return Ok(None) };
        if known.as_deref() == Some(path.as_str()) {
            return Ok(Some(Wallpaper { path, data: None }));
        }
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        let mime = match ext.as_str() {
            "png" => "image/png",
            "heic" | "heif" => "image/heic",
            "svg" => "image/svg+xml",
            "bmp" | "dib" => "image/bmp",
            "gif" => "image/gif",
            "webp" => "image/webp",
            _ => "image/jpeg",
        };
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        Ok(Some(Wallpaper { path, data: Some(format!("data:{mime};base64,{b64}")) }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn screen_colors() -> Result<Vec<[u8; 3]>, String> {
    tauri::async_runtime::spawn_blocking(|| desktop::screen_colors().map(|c| c.to_vec()).ok_or_else(|| "capture failed".to_string()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn debug_state() -> String {
    desktop::debug_state()
}

#[tauri::command]
fn reveal_taskbar(app: AppHandle) {
    let h = *app.state::<AppState>().hwnd.lock().unwrap();
    desktop::reveal_taskbar(h);
}

#[derive(Serialize)]
struct MonitorsOut {
    monitors: Vec<desktop::Monitor>,
    current: String,
}

#[tauri::command]
fn list_monitors(app: AppHandle) -> MonitorsOut {
    let current = app.state::<AppState>().settings.lock().unwrap().monitor.clone();
    MonitorsOut { monitors: desktop::monitors(), current }
}

fn set_monitor_inner(app: &AppHandle, id: String) {
    {
        let st = app.state::<AppState>();
        let mut s = st.settings.lock().unwrap();
        s.monitor = id.clone();
        settings::save(app, &s);
    }
    desktop::set_target(&id);
    apply_mode(app);
    mirror::sync(app);
}

fn set_mirror_inner(app: &AppHandle, on: bool) {
    {
        let st = app.state::<AppState>();
        let mut s = st.settings.lock().unwrap();
        s.no_mirror = !on;
        settings::save(app, &s);
    }
    mirror::sync(app);
}

#[tauri::command]
fn get_mirror(app: AppHandle) -> bool {
    !app.state::<AppState>().settings.lock().unwrap().no_mirror
}

#[tauri::command]
fn set_mirror(app: AppHandle, on: bool) {
    set_mirror_inner(&app, on);
}

#[tauri::command]
fn set_monitor(app: AppHandle, id: String) {
    set_monitor_inner(&app, id);
    island::follow(&app);
}

/// Dynamic Island: the main window says when it shows; the island asks to grow on hover.
#[tauri::command]
fn island_show(app: AppHandle, on: bool) {
    island::show(&app, on);
}

#[tauri::command]
fn island_size(app: AppHandle, open: bool) {
    island::set_open(&app, open);
}

#[tauri::command]
fn open_external(url: String) {
    desktop::open_external(&url);
}

#[tauri::command]
async fn media_control(action: String, value: Option<f64>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || media::control(action, value))
        .await
        .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
struct SearchOut {
    source: String,
    needs_login: bool,
    error: Option<String>,
    tracks: Vec<net::Track>,
}

/// Search on the chosen platform.
#[tauri::command]
async fn search(app: AppHandle, platform: String, query: String) -> SearchOut {
    let ok = |source: &str, r: Result<Vec<net::Track>, String>| match r {
        Ok(tracks) => SearchOut { source: source.into(), needs_login: false, error: None, tracks },
        Err(e) => SearchOut { source: source.into(), needs_login: false, error: Some(e), tracks: vec![] },
    };
    match platform.as_str() {
        "ytmusic" | "auto" => ok("ytmusic", net::ytm_search(&query).await),
        "youtube" => ok(
            "youtube",
            net::yt_search(&query).await.map(|v| {
                v.into_iter()
                    .map(|mut t| {
                        t.url = format!("https://www.youtube.com/watch?v={}", t.id);
                        t.source = "youtube".into();
                        t
                    })
                    .collect()
            }),
        ),
        "soundcloud" => ok("soundcloud", net::sc_search(&query).await),
        "spotify" | "spotifyweb" => match accounts::spotify_search(&app, &query).await {
            Ok(tracks) => SearchOut { source: "spotify".into(), needs_login: false, error: None, tracks },
            Err(e) => {
                let mut out = ok("apple", net::search_music(&query).await);
                out.needs_login = e == "not_connected";
                if !out.needs_login {
                    out.error = Some(e);
                }
                out
            }
        },
        _ => ok("apple", net::search_music(&query).await),
    }
}

/// Playlists and albums for the search panel (empty where the platform has none for us).
#[tauri::command]
async fn search_collections(app: AppHandle, platform: String, query: String) -> Vec<net::Collection> {
    match platform.as_str() {
        "ytmusic" | "auto" | "youtube" => net::ytm_collections(&query).await.unwrap_or_default(),
        "soundcloud" => net::sc_collections(&query).await.unwrap_or_default(),
        "spotify" | "spotifyweb" => accounts::spotify_collections(&app, &query).await.unwrap_or_default(),
        _ => vec![],
    }
}

/// The songs of a playlist / album found by `search_collections`.
#[tauri::command]
async fn collection_tracks(source: String, id: String) -> Result<Vec<net::Track>, String> {
    match source.as_str() {
        "ytmusic" => net::ytm_playlist(&id).await,
        "soundcloud" => net::sc_playlist(&id).await,
        _ => Err("Open this one in its app".into()),
    }
}

/// Songs to play after a song ("radio"), from the platform it came from.
#[derive(serde::Serialize)]
struct Suggest {
    tracks: Vec<net::Track>,
    personal: bool,
}

/// Save a YouTube Music playlist / album to the signed-in library.
#[tauri::command]
async fn save_playlist(app: AppHandle, id: String) -> Result<(), String> {
    net::set_yt_auth(accounts::yt_auth(&app));
    net::ytm_save_playlist(&id).await
}

/// Save the playing list (YouTube songs) as a new private playlist in the signed-in library.
#[tauri::command]
async fn create_playlist(app: AppHandle, title: String, ids: Vec<String>) -> Result<String, String> {
    net::set_yt_auth(accounts::yt_auth(&app));
    net::ytm_create_playlist(&title, &ids).await
}

/// Radio + the user's YouTube Music picks for one song.
#[tauri::command]
async fn suggest(app: AppHandle, id: String) -> Result<Suggest, String> {
    net::set_yt_auth(accounts::yt_auth(&app));
    let (tracks, personal) = net::ytm_suggest(&id).await?;
    Ok(Suggest { tracks, personal })
}

/// TikTok channel: open For You ("foryou"), Following, Friends or a search ("search") in the off-screen TikTok page.
#[tauri::command]
async fn tiktok_feed(app: AppHandle, mode: String, query: String) -> Result<(), String> {
    tiktok::feed(&app, &mode, &query)
}

/// The TikTok videos collected since the last call (page "user": from the profile page).
#[tauri::command]
async fn tiktok_take(app: AppHandle, page: Option<String>) -> Result<tiktok::Take, String> {
    tiktok::take(&app, page.as_deref()).await
}

#[tauri::command]
async fn tiktok_more(app: AppHandle, page: Option<String>) {
    tiktok::more(&app, page.as_deref())
}

/// Open someone's TikTok profile in its off-screen page ("" empties it).
#[tauri::command]
async fn tiktok_user(app: AppHandle, name: String) -> Result<(), String> {
    tiktok::user(&app, &name)
}

#[tauri::command]
async fn tiktok_stop(app: AppHandle) {
    tiktok::stop(&app)
}

/// Like / save a TikTok video with your TikTok account (flips it).
#[tauri::command]
async fn tiktok_act(app: AppHandle, author: String, id: String, kind: String) -> Result<bool, String> {
    tiktok::act(&app, &author, &id, &kind).await
}

/// The first comments under a TikTok video.
#[tauri::command]
async fn tiktok_comments(app: AppHandle, author: String, id: String) -> Result<Vec<tiktok::Comment>, String> {
    tiktok::comments(&app, &author, &id).await
}

/// Show the TikTok page (to answer TikTok's check), or put it away.
#[tauri::command]
async fn tiktok_show(app: AppHandle, on: bool, video: Option<bool>, page: Option<String>) {
    let p = page.unwrap_or_else(|| if video.unwrap_or(false) { "video".into() } else { String::new() });
    tiktok::show(&app, on, &p)
}

/// YouTube's Mix for one song ("Danh sách kết hợp"), or its next part when `cont` is given.
#[derive(serde::Serialize)]
struct Mix {
    tracks: Vec<net::Track>,
    cont: Option<String>,
}

#[tauri::command]
async fn mix(app: AppHandle, id: String, cont: Option<String>) -> Result<Mix, String> {
    net::set_yt_auth(accounts::yt_auth(&app));
    let (tracks, cont) = net::ytm_mix(&id, cont.as_deref()).await?;
    Ok(Mix { tracks, cont })
}

#[tauri::command]
async fn yt_music_feed(app: AppHandle, hl: String) -> Result<Vec<net::FeedShelf>, String> {
    net::set_yt_auth(accounts::yt_auth(&app));
    net::yt_music_feed(&hl).await
}

#[tauri::command]
async fn yt_list(app: AppHandle, video: String, list: String) -> Result<Vec<net::Track>, String> {
    net::set_yt_auth(accounts::yt_auth(&app));
    net::yt_list(&video, &list).await
}

#[tauri::command]
async fn radio(app: AppHandle, source: String, id: String) -> Result<Vec<net::Track>, String> {
    net::set_yt_auth(accounts::yt_auth(&app));
    match source.as_str() {
        "soundcloud" => net::sc_related(&id).await,
        "ytmusic" | "youtube" => net::ytm_radio(&id).await,
        _ => Err("no radio for this platform".into()),
    }
}

#[tauri::command]
async fn search_rich(query: String) -> Result<net::Rich, String> {
    net::ytm_rich(&query).await
}

#[tauri::command]
async fn browse_artist(id: String, title: String, art: String) -> Result<Vec<net::Track>, String> {
    net::ytm_artist(&id, &title, &art).await
}

/// Settings → Support: signed in to GitHub? did this account star Wallplayer?
#[tauri::command]
async fn github_star_state(app: AppHandle) -> github::Star {
    github::state(&app).await
}

/// Time for the "Star on GitHub" card? (30 days after the last one, never once starred.)
#[tauri::command]
async fn github_star_due(app: AppHandle) -> bool {
    github::check_due(&app).await
}

/// The card was shown: the next one comes in 30 days.
#[tauri::command]
fn github_star_asked(app: AppHandle) {
    github::asked(&app);
}

/// Press GitHub's Star button for the signed-in account. Err("signin") = sign in first.
#[tauri::command]
async fn github_star(app: AppHandle) -> Result<bool, String> {
    github::star(&app).await
}

#[tauri::command]
async fn account_status(app: AppHandle) -> accounts::Accounts {
    accounts::status(&app).await
}

#[tauri::command]
async fn account_login(app: AppHandle, platform: String) -> Result<(), String> {
    accounts::login(app, platform).await
}

#[tauri::command]
async fn account_logout(app: AppHandle, platform: String) -> Result<(), String> {
    accounts::logout(app, platform).await
}

#[tauri::command]
async fn spotify_connect(app: AppHandle, client_id: String) -> Result<(), String> {
    accounts::spotify_connect(app, client_id).await
}

#[tauri::command]
async fn spotify_play(app: AppHandle, uri: String) -> Result<String, String> {
    accounts::spotify_play(&app, &uri).await
}

#[tauri::command]
async fn youtube_search(query: String) -> Result<Vec<String>, String> {
    net::youtube_search(&query).await
}

#[tauri::command]
async fn yt_captions(video_id: String) -> Result<Vec<net::Cue>, String> {
    net::yt_captions(&video_id).await
}

#[tauri::command]
async fn lyrics(artist: String, title: String, duration: Option<f64>) -> Result<Option<net::Lyrics>, String> {
    net::lyrics(&artist, &title, duration).await
}

#[tauri::command]
async fn lyrics_words(artist: String, title: String, duration: Option<f64>, id: Option<i64>) -> Result<Option<String>, String> {
    net::lyrics_words(&artist, &title, duration, id).await
}

#[tauri::command]
async fn lyrics_search(query: String) -> Result<Vec<net::Lyrics>, String> {
    net::lyrics_search(&query).await
}

#[tauri::command]
async fn lyrics_page(url: String) -> Result<String, String> {
    net::genius_page(&url).await
}

#[tauri::command]
async fn translate(lines: Vec<String>, target: String) -> Result<net::Translated, String> {
    net::translate(lines, &target).await
}

#[tauri::command]
async fn fetch_image(url: String) -> Result<String, String> {
    net::fetch_image(&url).await
}

/// Interface language of the tray menu (the UI tells us which one).
pub(crate) static UI_VI: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn tray_label(id: &str) -> String {
    let vi = UI_VI.load(std::sync::atomic::Ordering::Relaxed);
    #[cfg(not(windows))]
    {
        // the shortcut shown is this system's default one; "Start with …" names this system
        let key = settings::default_key_label();
        let at_login = if cfg!(target_os = "macos") { ("Open at login", "Mở khi đăng nhập") } else { ("Start at login", "Mở khi đăng nhập") };
        match (id, vi) {
            ("search", false) => return format!("Search music   {key}"),
            ("search", true) => return format!("Tìm nhạc   {key}"),
            ("autostart", false) => return at_login.0.to_string(),
            ("autostart", true) => return at_login.1.to_string(),
            _ => {}
        }
    }
    match (id, vi) {
        ("search", false) => "Search music   Ctrl+Alt+Space",
        ("search", true) => "Tìm nhạc   Ctrl+Alt+Space",
        ("wallpaper", false) => "Behind desktop icons",
        ("wallpaper", true) => "Nằm sau icon desktop",
        ("icons", false) => "Hide desktop icons",
        ("icons", true) => "Ẩn icon desktop",
        ("display", false) => "Move to next display",
        ("display", true) => "Chuyển sang màn hình kế",
        ("mirror", false) => "Fill other displays",
        ("mirror", true) => "Phủ các màn hình khác",
        ("autostart", false) => "Start with Windows",
        ("autostart", true) => "Mở cùng Windows",
        ("reload", false) => "Reload skin",
        ("reload", true) => "Tải lại giao diện",
        ("quit", false) => "Quit Wallplayer",
        ("quit", true) => "Thoát Wallplayer",
        _ => "",
    }
    .to_string()
}

#[tauri::command]
fn set_ui_lang(app: AppHandle, lang: String) {
    UI_VI.store(lang == "vi", std::sync::atomic::Ordering::Relaxed);
    for (id, set) in app.state::<AppState>().tray_texts.lock().unwrap().iter() {
        set(&tray_label(id));
    }
    update::refresh_tray(&app);
}

pub(crate) fn quit(app: &AppHandle) {
    guard::QUITTING.store(true, std::sync::atomic::Ordering::Relaxed);
    desktop::set_icons_hidden(false);   // give the desktop icons back
    app.exit(0);
}

/// Folders / files you add or drop: audio is read, pictures and videos come back for the wallpaper.
#[tauri::command]
async fn music_import(app: AppHandle, paths: Vec<String>) -> library::Imported {
    tauri::async_runtime::spawn_blocking(move || library::import(&app, &paths)).await.unwrap_or_default()
}

/// Re-open access to remembered music folders at start.
#[tauri::command]
fn music_allow(app: AppHandle, paths: Vec<String>) {
    library::allow_all(&app, &paths);
}

#[tauri::command]
async fn music_art(path: String) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || library::art(&path)).await.ok().flatten()
}

/// "Add a folder…" (runs off the main thread: the picker blocks).
#[tauri::command]
async fn music_pick_folder(app: AppHandle) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || library::pick_folder(&app)).await.ok().flatten()
}

/// Tray icon = the album cover drawn by the UI (32×32 RGBA, base64); None = Wallplayer's own icon.
#[tauri::command]
fn set_tray_art(app: AppHandle, rgba: Option<String>) {
    use base64::Engine;
    let Some(tray) = app.tray_by_id("wallplayer") else { return };
    let icon = rgba
        .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
        .filter(|px| px.len() == 32 * 32 * 4)
        .map(|px| tauri::image::Image::new_owned(px, 32, 32))
        .or_else(|| app.default_window_icon().cloned());
    let _ = tray.set_icon(icon);
}

#[tauri::command]
fn get_power(app: AppHandle, window: tauri::WebviewWindow) -> power::PowerState {
    power::current(&app, window.label())
}

#[tauri::command]
fn set_power_prefs(cover_pause: bool, low: bool) {
    power::set_prefs(cover_pause, low);
}

#[tauri::command]
fn update_status(app: AppHandle) -> update::UpdateStatus {
    update::status(&app)
}

#[tauri::command]
async fn update_check(app: AppHandle) -> update::UpdateStatus {
    update::check(app).await
}

#[tauri::command]
async fn update_download(app: AppHandle) -> update::UpdateStatus {
    update::download(app).await
}

#[tauri::command]
fn update_set_auto(app: AppHandle, on: bool) -> update::UpdateStatus {
    {
        let st = app.state::<AppState>();
        let mut s = st.settings.lock().unwrap();
        s.auto_update = on;
        settings::save(&app, &s);
    }
    update::status(&app)
}

#[tauri::command]
fn update_skip(app: AppHandle, version: String) -> update::UpdateStatus {
    {
        let st = app.state::<AppState>();
        let mut s = st.settings.lock().unwrap();
        s.skip_version = version;
        settings::save(&app, &s);
    }
    update::refresh_tray(&app);
    update::status(&app)
}

#[tauri::command]
fn update_restart(app: AppHandle) -> Result<(), String> {
    update::restart(&app)
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    quit(&app);
}

// ───────────────────────── "Search from anywhere" binding ─────────────────────────

/// Register the binding: a keyboard shortcut (global-shortcut plugin) or a mouse button (low-level hook).
/// The two global shortcuts now registered: search, and show / hide the desktop icons.
static SEARCH_SC: Mutex<Option<Shortcut>> = Mutex::new(None);
static ICONS_SC: Mutex<Option<Shortcut>> = Mutex::new(None);

fn is_icons_key(sc: &Shortcut) -> bool {
    ICONS_SC.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|i| i.id() == sc.id())
}

/// Show / hide the desktop icons with a key ("Ctrl+Alt+D"…; "" = none). Not on Linux.
fn apply_icons_key(app: &AppHandle, spec: &str) -> Result<(), String> {
    if let Some(old) = ICONS_SC.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = app.global_shortcut().unregister(old);
    }
    let spec = spec.trim();
    if spec.is_empty() || cfg!(target_os = "linux") {
        return Ok(());
    }
    if spec.contains("Mouse:") {
        return Err("use a key here".into());
    }
    let sc: Shortcut = spec.parse().map_err(|e| format!("{e}"))?;
    if SEARCH_SC.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|s| s.id() == sc.id()) {
        return Err("that is the search shortcut".into());
    }
    app.global_shortcut().register(sc).map_err(|e| e.to_string())?;
    *ICONS_SC.lock().unwrap_or_else(|e| e.into_inner()) = Some(sc);
    Ok(())
}

#[tauri::command]
fn get_icons_key(app: AppHandle) -> String {
    app.state::<AppState>().settings.lock().unwrap().icons_key.clone()
}

#[tauri::command]
fn set_icons_key(app: AppHandle, spec: String) -> Result<String, String> {
    if let Err(e) = apply_icons_key(&app, &spec) {
        let old = get_icons_key(app.clone());
        let _ = apply_icons_key(&app, &old);
        return Err(e);
    }
    let st = app.state::<AppState>();
    let mut s = st.settings.lock().unwrap();
    s.icons_key = spec.clone();
    settings::save(&app, &s);
    Ok(spec)
}

fn apply_search_key(app: &AppHandle, spec: &str) -> Result<(), String> {
    if let Some(old) = SEARCH_SC.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = app.global_shortcut().unregister(old);
    }
    deskmouse::set_bind(0, 0);
    let spec = spec.trim();
    if let Some(i) = spec.find("Mouse:") {
        let mut mods = 0u32;
        for part in spec[..i].split('+').filter(|p| !p.is_empty()) {
            mods |= match part {
                "Ctrl" => 1,
                "Alt" => 2,
                "Shift" => 4,
                "Super" => 8,
                _ => return Err(format!("unknown modifier {part}")),
            };
        }
        let btn = match &spec[i + 6..] {
            "Middle" => 1,
            "X1" => 2,
            "X2" => 3,
            _ => return Err("this mouse button can't be used".into()),
        };
        deskmouse::set_bind(btn, mods);
        return Ok(());
    }
    let sc: Shortcut = spec.parse().map_err(|e| format!("{e}"))?;
    if is_icons_key(&sc) {
        return Err("that is the desktop icons shortcut".into());
    }
    app.global_shortcut().register(sc).map_err(|e| e.to_string())?;
    *SEARCH_SC.lock().unwrap_or_else(|e| e.into_inner()) = Some(sc);
    Ok(())
}

#[tauri::command]
fn get_search_key(app: AppHandle) -> String {
    app.state::<AppState>().settings.lock().unwrap().search_key.clone()
}

#[tauri::command]
fn set_search_key(app: AppHandle, spec: String) -> Result<String, String> {
    if let Err(e) = apply_search_key(&app, &spec) {
        let old = get_search_key(app.clone());
        let _ = apply_search_key(&app, &old);
        return Err(e);
    }
    let st = app.state::<AppState>();
    let mut s = st.settings.lock().unwrap();
    s.search_key = spec.clone();
    settings::save(&app, &s);
    Ok(spec)
}

/// While true, the next middle / side mouse button press (anywhere) is reported as `bind-captured`.
#[tauri::command]
fn capture_mouse_bind(on: bool) {
    deskmouse::set_capture(on);
}

// ───────────────────────── main ─────────────────────────

fn main() {
    guard::panic_hook();
    update::wait_for_previous();   // started by "Restart to update": let the old one exit first
    desktop::set_app_id();

    tauri::Builder::default()
        // tells the interface which system it runs on (names, the default shortcut) before it starts
        .plugin(
            tauri::plugin::Builder::<tauri::Wry>::new("wp-os")
                .js_init_script(format!(
                    "window.__WP_OS__ = '{OS}'; window.__WP_DEFAULT_KEY__ = '{}'; window.__WP_ICONS_KEY__ = '{}';",
                    settings::default_search_key(),
                    settings::default_icons_key()
                ))
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, sc, ev| {
                    if ev.state() == ShortcutState::Pressed {
                        if is_icons_key(sc) {
                            toggle_icons(app);
                        } else {
                            open_search(app);
                        }
                    }
                })
                .build(),
        )
        .manage(AppState {
            settings: Mutex::new(settings::Settings::default()),
            peek: Mutex::new(false),
            hwnd: Mutex::new(0),
            wall_item: Mutex::new(None),
            icons_item: Mutex::new(None),
            auto_item: Mutex::new(None),
            update_item: Mutex::new(None),
            tray_texts: Mutex::new(Vec::new()),
            mirrors_installed: Mutex::new(Vec::new()),
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            set_wallpaper_mode,
            set_icons_hidden,
            get_icons_key,
            set_icons_key,
            set_autostart,
            set_autostart_high,
            save_playlist,
            create_playlist,
            peek,
            open_external,
            reveal_taskbar,
            screen_colors,
            desktop_wallpaper,
            yt_captions,
            debug_state,
            get_volume,
            set_volume,
            set_mute,
            list_monitors,
            set_monitor,
            island_show,
            island_size,
            get_mirror,
            set_mirror,
            media_control,
            search,
            search_collections,
            collection_tracks,
            yt_music_feed,
            yt_list,
            radio,
            mix,
            tiktok_feed,
            tiktok_take,
            tiktok_more,
            tiktok_user,
            tiktok_stop,
            tiktok_show,
            tiktok_act,
            tiktok_comments,
            account_status,
            account_login,
            github_star_state,
            github_star_due,
            github_star_asked,
            github_star,
            account_logout,
            spotify_connect,
            spotify_play,
            youtube_search,
            get_search_key,
            set_search_key,
            capture_mouse_bind,
            search_rich,
            browse_artist,
            suggest,
            lyrics,
            lyrics_search,
            lyrics_words,
            fetch_image,
            lyrics_page,
            translate,
            get_power,
            music_import,
            music_allow,
            music_art,
            music_pick_folder,
            set_tray_art,
            set_ui_lang,
            set_power_prefs,
            update_status,
            update_check,
            update_download,
            update_skip,
            update_set_auto,
            update_restart,
            quit_app
        ])
        .setup(move |app| {
            let handle = app.handle().clone();

            // settings
            let s = settings::load(&handle);
            let hide_on = s.hide_icons;
            desktop::set_target(&s.monitor);
            *app.state::<AppState>().settings.lock().unwrap() = s;
            github::note_use(&handle);

            // window → desktop layer
            let win = app.get_webview_window("main").expect("main window");
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory); // a tray app: no Dock icon
            let raw = raw_of(&win);
            *app.state::<AppState>().hwnd.lock().unwrap() = raw;
            if raw != 0 {
                desktop::install(raw);
                desktop::spawn_watcher(raw);
                deskmouse::start(handle.clone());
            }
            let at_signin = std::env::args().any(|a| a == settings::AUTOSTART_ARG);
            if at_signin {
                boot::boost_start();
            }
            // at sign-in the desktop may not exist yet: stay hidden until it does (below), instead of
            // showing up as a plain window over the sign-in screen
            if !at_signin || desktop::shell_ready() {
                apply_mode(&handle);
            }
            desktop::set_icons_hidden(hide_on);
            // the web engine dies → start again instead of sitting empty in the tray
            // Started by Windows at sign-in: Explorer may not have built the desktop yet. Wait for it
            // (up to 2 minutes), then go behind the icons again — otherwise the skin could stay a plain
            // window behind everything, looking as if Wallplayer never opened.
            guard::log(&handle, if at_signin { "started with Windows" } else { "started" });
            if let Some(fix) = settings::heal_autostart() {
                guard::log(&handle, &fix);
            }
            if at_signin {
                let h3 = handle.clone();
                std::thread::spawn(move || {
                    let t0 = std::time::Instant::now();
                    while !desktop::shell_ready() && t0.elapsed() < std::time::Duration::from_secs(120) {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                    // Explorer keeps rebuilding the desktop layer for a moment after it appears
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    guard::log(&h3, &format!("desktop ready after {:.1}s: placing the skin", t0.elapsed().as_secs_f32()));
                    let h4 = h3.clone();
                    let _ = h3.run_on_main_thread(move || apply_mode(&h4));
                    // the skin is up: back to normal priority once it has settled
                    boot::boost_end_after(std::time::Duration::from_secs(20));
                });
            }
            guard::watch(&win, true);

            // leave peek when focus goes elsewhere
            let h2 = handle.clone();
            win.on_window_event(move |e| {
                // Alt+F4 while the skin is in front must not tear it down (quit from the tray)
                if let WindowEvent::CloseRequested { api, .. } = e {
                    api.prevent_close();
                    return;
                }
                // The skin lives inside the desktop's WorkerW layer. When Windows rebuilds that layer
                // (wallpaper change, display sleep / wake, a full-screen game switching modes,
                // Explorer restarting…) the skin window is destroyed with it while Wallplayer keeps
                // running in the tray: start again so it comes back by itself.
                if let WindowEvent::Destroyed = e {
                    if !guard::QUITTING.load(std::sync::atomic::Ordering::Relaxed) {
                        guard::log(&h2, "the skin window was destroyed (the desktop layer was rebuilt): starting again");
                        guard::relaunch(h2.clone());
                    }
                    return;
                }
                if let WindowEvent::Focused(false) = e {
                    // Linux hides the skin for a moment to bring it to the front: not the user leaving
                    #[cfg(not(windows))]
                    if desktop::remapping() {
                        return;
                    }
                    if *h2.state::<AppState>().peek.lock().unwrap() {
                        set_peek(&h2, false);
                        let _ = h2.emit("peek-ended", ());
                    }
                }
            });

            // tray
            let search_i = MenuItem::with_id(app, "search", tray_label("search"), true, None::<&str>)?;
            let auto_i = CheckMenuItem::with_id(
                app,
                "autostart",
                tray_label("autostart"),
                true,
                settings::autostart_enabled(),
                None::<&str>,
            )?;
            let icons_i = CheckMenuItem::with_id(app, "icons", "Hide desktop icons", true, hide_on, None::<&str>)?;
            let display_i = MenuItem::with_id(app, "display", "Move to next display", true, None::<&str>)?;
            let mirror_on = !app.state::<AppState>().settings.lock().unwrap().no_mirror;
            let mirror_i = CheckMenuItem::with_id(app, "mirror", "Fill other displays", true, mirror_on, None::<&str>)?;
            let reload_i = MenuItem::with_id(app, "reload", "Reload skin", true, None::<&str>)?;
            let update_i = MenuItem::with_id(app, "update", format!("Check for updates   (v{})", update::current()), true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit Wallplayer", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            // Linux: the skin always covers the file manager's desktop icons, so that switch has no use there
            #[cfg(not(target_os = "linux"))]
            let menu = Menu::with_items(app, &[&search_i, &icons_i, &display_i, &mirror_i, &auto_i, &sep, &update_i, &reload_i, &quit_i])?;
            #[cfg(target_os = "linux")]
            let menu = Menu::with_items(app, &[&search_i, &display_i, &mirror_i, &auto_i, &sep, &update_i, &reload_i, &quit_i])?;
            let mirror_c = mirror_i.clone();
            *app.state::<AppState>().icons_item.lock().unwrap() = Some(icons_i.clone());
            *app.state::<AppState>().auto_item.lock().unwrap() = Some(auto_i.clone());
            *app.state::<AppState>().update_item.lock().unwrap() = Some(update_i.clone());
            {
                let st = app.state::<AppState>();
                let mut tt = st.tray_texts.lock().unwrap();
                macro_rules! keep {
                    ($id:expr, $item:expr) => {{
                        let it = $item.clone();
                        tt.push(($id, Box::new(move |t: &str| {
                            let _ = it.set_text(t);
                        })));
                    }};
                }
                keep!("search", search_i);
                keep!("icons", icons_i);
                keep!("display", display_i);
                keep!("mirror", mirror_i);
                keep!("autostart", auto_i);
                keep!("reload", reload_i);
                keep!("quit", quit_i);
            }

            let auto_c = auto_i.clone();
            let mut tray = TrayIconBuilder::with_id("wallplayer")
                .tooltip("Wallplayer")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, ev| match ev.id().as_ref() {
                    "search" => open_search(app),
                    // bring the skin up: it shows the release notes (Skip / Update), checking first if needed
                    "update" => {
                        set_peek(app, true);
                        let _ = app.emit_to("main", "update-check-request", ());
                    }
                    "autostart" => {
                        let on = !settings::autostart_enabled();
                        let _ = settings::set_autostart(on);
                        let _ = auto_c.set_checked(settings::autostart_enabled());
                        let _ = app.emit("mode-changed", ui_state(app));
                    }
                    "display" => {
                        let list = desktop::monitors();
                        if list.len() > 1 {
                            let cur = app.state::<AppState>().settings.lock().unwrap().monitor.clone();
                            let i = list.iter().position(|m| m.id == cur).or_else(|| list.iter().position(|m| m.primary)).unwrap_or(0);
                            let next = list[(i + 1) % list.len()].id.clone();
                            set_monitor_inner(app, next);
                            let _ = app.emit("monitor-changed", ());
                        }
                    }
                    "mirror" => {
                        let on = app.state::<AppState>().settings.lock().unwrap().no_mirror;
                        set_mirror_inner(app, on);
                        let _ = mirror_c.set_checked(on);
                        let _ = app.emit("mirror-changed", on);
                    }
                    "reload" => {
                        for (label, w) in app.webview_windows() {
                            if label == "main" || label.starts_with("mirror-") {
                                let _ = w.eval("location.reload()");
                            }
                        }
                    }
                    "quit" => quit(app),
                    "icons" => {
                        let on = !app.state::<AppState>().settings.lock().unwrap().hide_icons;
                        set_icons_hidden_inner(app, on);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, ev| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = ev
                    {
                        open_search(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            // hotkey
            let key = app.state::<AppState>().settings.lock().unwrap().search_key.clone();
            if let Err(e) = apply_search_key(&handle, &key) {
                eprintln!("hotkey: {e}");
                let _ = apply_search_key(&handle, &settings::default_search_key());
            }
            let ikey = app.state::<AppState>().settings.lock().unwrap().icons_key.clone();
            if let Err(e) = apply_icons_key(&handle, &ikey) {
                guard::log(&handle, &format!("desktop icons shortcut {ikey}: {e}"));
            }

            // companion windows on the other monitors; re-check when monitors come and go
            mirror::sync(&handle);
            let h3 = handle.clone();
            std::thread::spawn(move || {
                let key = || desktop::monitors().iter().map(|m| format!("{}@{},{},{}x{}", m.id, m.x, m.y, m.w, m.h)).collect::<Vec<_>>().join("|");
                let mut last = key();
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    let now = key();
                    if now != last {
                        last = now;
                        mirror::sync(&h3);
                    }
                }
            });

            // native feeds
            media::spawn(handle.clone());
            audio::spawn(handle.clone());
            power::spawn(handle.clone());
            update::mark_started(&handle);
            update::spawn(handle.clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while running Wallplayer")
        .run(|_app, event| {
            // Wallplayer lives in the tray: it quits only from there (or to update / restart itself),
            // never because its windows went away — the skin then comes back (see the main window's
            // Destroyed event).
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() && !guard::QUITTING.load(std::sync::atomic::Ordering::Relaxed) {
                    api.prevent_exit();
                }
            }
        });
}
