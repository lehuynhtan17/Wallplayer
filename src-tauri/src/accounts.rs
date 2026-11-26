//! Platform accounts.
//!
//! * YouTube / SoundCloud: the user signs in inside an in-app browser window
//!   that shares the WebView2 profile with the wallpaper. The embedded
//!   players then run with that session (YouTube Premium → no ads, SoundCloud
//!   Go+ → full tracks). Nothing is stored by us; cookies live in WebView2.
//! * Spotify: official OAuth (Authorization Code + PKCE) with the user's own
//!   Client ID. Gives Spotify search and "play this track on my Spotify app"
//!   through Spotify Connect (playback control needs Spotify Premium).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use rand::distributions::Alphanumeric;
use rand::Rng;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

use crate::net::{client, Collection, Track};

pub const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
/// Must be identical for every webview of the app (shared WebView2 environment).
pub const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,UserAgentClientHint --autoplay-policy=no-user-gesture-required";

pub const SP_REDIRECT: &str = "http://127.0.0.1:43821/callback";
const SP_SCOPES: &str = "user-read-playback-state user-modify-playback-state user-read-currently-playing";

static SP_ACCESS: Mutex<Option<(String, Instant)>> = Mutex::new(None);
static SP_PENDING: Mutex<Option<(String, String, String)>> = Mutex::new(None);

const YT_COOKIES: &[&str] = &["SAPISID", "__Secure-3PAPISID", "LOGIN_INFO", "__Secure-1PSID"];
const SC_COOKIES: &[&str] = &["oauth_token"];
const TT_COOKIES: &[&str] = &["sessionid", "sessionid_ss"];
const GH_COOKIES: &[&str] = &["user_session"];

#[derive(Serialize, Clone)]
pub struct Accounts {
    pub youtube: bool,
    pub soundcloud: bool,
    pub spotify: bool,
    pub tiktok: bool,
    pub github: bool,
    pub spotify_client_id: String,
    pub redirect_uri: String,
}

fn enc(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

pub(crate) fn has_cookie(app: &AppHandle, url: &str, names: &[&str]) -> bool {
    let Some(w) = app.get_webview_window("main") else { return false };
    let Ok(u) = Url::parse(url) else { return false };
    w.cookies_for_url(u)
        .map(|v| v.iter().any(|c| names.contains(&c.name()) && !c.value().is_empty()))
        .unwrap_or(false)
}

fn delete_cookies(app: &AppHandle, url: &str) {
    let Some(w) = app.get_webview_window("main") else { return };
    let Ok(u) = Url::parse(url) else { return };
    if let Ok(list) = w.cookies_for_url(u) {
        for c in list {
            let _ = w.delete_cookie(c);
        }
    }
}

/// The signed-in YouTube session (cookie header + SAPISID) from the in-app browser, for personalised YouTube Music requests.
pub fn yt_auth(app: &AppHandle) -> Option<(String, String)> {
    let w = app.get_webview_window("main")?;
    let mut all: Vec<(String, String)> = Vec::new();
    for u in ["https://music.youtube.com", "https://www.youtube.com"] {
        if let Ok(url) = Url::parse(u) {
            if let Ok(list) = w.cookies_for_url(url) {
                for c in list {
                    if !c.value().is_empty() && !all.iter().any(|(n, _)| n == c.name()) {
                        all.push((c.name().to_string(), c.value().to_string()));
                    }
                }
            }
        }
    }
    let sapisid = all
        .iter()
        .find(|(n, _)| n == "SAPISID")
        .or_else(|| all.iter().find(|(n, _)| n == "__Secure-3PAPISID"))
        .map(|(_, v)| v.clone())?;
    let cookie = all.iter().map(|(n, v)| format!("{n}={v}")).collect::<Vec<_>>().join("; ");
    Some((cookie, sapisid))
}

pub async fn status(app: &AppHandle) -> Accounts {
    let youtube = has_cookie(app, "https://www.youtube.com", YT_COOKIES);
    let soundcloud = has_cookie(app, "https://soundcloud.com", SC_COOKIES);
    let tiktok = has_cookie(app, "https://www.tiktok.com", TT_COOKIES);
    let github = has_cookie(app, "https://github.com", GH_COOKIES);
    let (cid, refresh) = {
        let st = app.state::<crate::AppState>();
        let s = st.settings.lock().unwrap();
        (s.spotify_client_id.clone(), !s.spotify_refresh.is_empty())
    };
    Accounts {
        youtube,
        soundcloud,
        spotify: refresh,
        tiktok,
        github,
        spotify_client_id: cid,
        redirect_uri: SP_REDIRECT.into(),
    }
}

/// Put a sign-in window away without closing it. All Wallplayer windows share one WebView2 engine, and
/// closing a window while its page is still busy (sign-in redirects, the password bubble) can crash that
/// engine: the whole skin then goes blank. So the window is hidden and emptied, and the next sign-in
/// uses it again.
fn retire(w: &WebviewWindow) {
    let _ = w.hide();
    if let Ok(u) = Url::parse("about:blank") {
        let _ = w.navigate(u);
    }
}

/// The ✕ of a sign-in window hides it too.
fn keep_on_close(w: &WebviewWindow) {
    let w2 = w.clone();
    w.on_window_event(move |e| {
        if let WindowEvent::CloseRequested { api, .. } = e {
            api.prevent_close();
            retire(&w2);
        }
    });
}

/// Show a sign-in window on `url`: the one kept from last time, or a new one.
fn show_again(win: &WebviewWindow, url: &str) -> Result<(), String> {
    win.navigate(Url::parse(url).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let _ = win.center();
    let _ = win.show();
    let _ = win.set_focus();
    Ok(())
}

fn browser_window(app: &AppHandle, label: &str, url: &str, title: &str, w: f64, h: f64) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(label) {
        return show_again(&win, url);
    }
    let win = WebviewWindowBuilder::new(app, label, WebviewUrl::External(Url::parse(url).map_err(|e| e.to_string())?))
        .title(title)
        .inner_size(w, h)
        .center()
        .user_agent(UA)
        .additional_browser_args(BROWSER_ARGS)
        .build()
        .map_err(|e| e.to_string())?;
    keep_on_close(&win);
    Ok(())
}

/// Open the sign-in page; closes itself as soon as the session cookie appears.
pub async fn login(app: AppHandle, platform: String) -> Result<(), String> {
    let (label, url, title, check_url, names, nice): (&str, &str, &str, &str, &'static [&'static str], &str) =
        match platform.as_str() {
            "youtube" => (
                "login-youtube",
                "https://accounts.google.com/ServiceLogin?service=youtube&hl=en&continue=https%3A%2F%2Fwww.youtube.com%2Fsignin%3Faction_handle_signin%3Dtrue%26next%3Dhttps%253A%252F%252Fmusic.youtube.com%252F",
                "Sign in — YouTube / YouTube Music",
                "https://www.youtube.com",
                YT_COOKIES,
                "YouTube",
            ),
            "soundcloud" => (
                "login-soundcloud",
                "https://soundcloud.com/signin",
                "Sign in — SoundCloud",
                "https://soundcloud.com",
                SC_COOKIES,
                "SoundCloud",
            ),
            "tiktok" => (
                "login-tiktok",
                "https://www.tiktok.com/login",
                "Sign in — TikTok",
                "https://www.tiktok.com",
                TT_COOKIES,
                "TikTok",
            ),
            "github" => (
                "login-github",
                "https://github.com/login?return_to=%2Flehuynhtan17%2FWallplayer",
                "Sign in — GitHub",
                "https://github.com",
                GH_COOKIES,
                "GitHub",
            ),
            _ => return Err("unknown platform".into()),
        };
    crate::end_peek(&app);
    browser_window(&app, label, url, title, 980.0, 760.0)?;
    let app2 = app.clone();
    let label = label.to_string();
    let nice = nice.to_string();
    let check_url = check_url.to_string();
    tauri::async_runtime::spawn(async move {
        // give the page a moment so an existing session isn't mistaken for a fresh one
        tokio::time::sleep(Duration::from_millis(2500)).await;
        for _ in 0..480 {
            // put away (or closed by you): stop watching
            match app2.get_webview_window(&label) {
                Some(w) if w.is_visible().unwrap_or(false) => {}
                _ => break,
            }
            if has_cookie(&app2, &check_url, names) {
                tokio::time::sleep(Duration::from_millis(1500)).await;
                if let Some(w) = app2.get_webview_window(&label) {
                    retire(&w);
                }
                let _ = app2.emit("toast", format!("Signed in to {nice}"));
                break;
            }
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
        let _ = app2.emit("accounts-changed", ());
    });
    Ok(())
}

pub async fn logout(app: AppHandle, platform: String) -> Result<(), String> {
    match platform.as_str() {
        "youtube" => {
            for u in [
                "https://www.youtube.com",
                "https://music.youtube.com",
                "https://accounts.google.com",
                "https://www.google.com",
            ] {
                delete_cookies(&app, u);
            }
        }
        "soundcloud" => {
            for u in ["https://soundcloud.com", "https://api-auth.soundcloud.com", "https://secure.soundcloud.com"] {
                delete_cookies(&app, u);
            }
        }
        "tiktok" => {
            for u in ["https://www.tiktok.com", "https://tiktok.com", "https://www.tiktokv.com"] {
                delete_cookies(&app, u);
            }
        }
        "github" => {
            for u in ["https://github.com", "https://api.github.com"] {
                delete_cookies(&app, u);
            }
        }
        "spotify" => {
            *SP_ACCESS.lock().unwrap() = None;
            let st = app.state::<crate::AppState>();
            let mut s = st.settings.lock().unwrap();
            s.spotify_refresh.clear();
            crate::settings::save(&app, &s);
        }
        _ => return Err("unknown platform".into()),
    }
    let _ = app.emit("accounts-changed", ());
    Ok(())
}

// ───────────────────────── Spotify ─────────────────────────

pub async fn spotify_connect(app: AppHandle, client_id: String) -> Result<(), String> {
    let cid = client_id.trim().to_string();
    if cid.len() < 20 || !cid.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("That Client ID doesn't look right".into());
    }
    {
        let st = app.state::<crate::AppState>();
        let mut s = st.settings.lock().unwrap();
        s.spotify_client_id = cid.clone();
        crate::settings::save(&app, &s);
    }
    let verifier: String = rand::thread_rng().sample_iter(&Alphanumeric).take(64).map(char::from).collect();
    let state: String = rand::thread_rng().sample_iter(&Alphanumeric).take(20).map(char::from).collect();
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    *SP_PENDING.lock().unwrap() = Some((cid.clone(), verifier, state.clone()));
    let url = format!(
        "https://accounts.spotify.com/authorize?response_type=code&client_id={}&scope={}&redirect_uri={}&code_challenge_method=S256&code_challenge={}&state={}&show_dialog=false",
        enc(&cid),
        enc(SP_SCOPES),
        enc(SP_REDIRECT),
        challenge,
        state
    );
    crate::end_peek(&app);
    if let Some(w) = app.get_webview_window("login-spotify") {
        return show_again(&w, &url);   // its redirect watcher reads the new login from SP_PENDING
    }
    let app2 = app.clone();
    let win = WebviewWindowBuilder::new(&app, "login-spotify", WebviewUrl::External(Url::parse(&url).map_err(|e| e.to_string())?))
        .title("Connect Spotify")
        .inner_size(520.0, 780.0)
        .center()
        .user_agent(UA)
        .additional_browser_args(BROWSER_ARGS)
        .on_navigation(move |u| {
            if u.as_str().starts_with(SP_REDIRECT) {
                let q: HashMap<String, String> = u.query_pairs().into_owned().collect();
                let app3 = app2.clone();
                tauri::async_runtime::spawn(async move {
                    let r = finish(&app3, q).await;
                    if let Some(w) = app3.get_webview_window("login-spotify") {
                        retire(&w);
                    }
                    let _ = app3.emit(
                        "toast",
                        match r {
                            Ok(_) => "Spotify connected".to_string(),
                            Err(e) => format!("Spotify: {e}"),
                        },
                    );
                    let _ = app3.emit("accounts-changed", ());
                });
                return false;
            }
            true
        })
        .build()
        .map_err(|e| e.to_string())?;
    keep_on_close(&win);
    Ok(())
}

async fn finish(app: &AppHandle, q: HashMap<String, String>) -> Result<(), String> {
    let pending = SP_PENDING.lock().unwrap().take();
    let (cid, verifier, state) = pending.ok_or("no login in progress")?;
    if let Some(e) = q.get("error") {
        return Err(e.clone());
    }
    if q.get("state") != Some(&state) {
        return Err("login state mismatch".into());
    }
    let code = q.get("code").ok_or("no code returned")?.clone();
    let v: Value = client()
        .post("https://accounts.spotify.com/api/token")
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", SP_REDIRECT),
            ("client_id", cid.as_str()),
            ("code_verifier", verifier.as_str()),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    store_tokens(app, &v)
}

fn store_tokens(app: &AppHandle, v: &Value) -> Result<(), String> {
    let access = v
        .get("access_token")
        .and_then(|a| a.as_str())
        .ok_or_else(|| {
            v.get("error_description")
                .or_else(|| v.get("error"))
                .and_then(|e| e.as_str())
                .unwrap_or("token error")
                .to_string()
        })?;
    let exp = v.get("expires_in").and_then(|e| e.as_u64()).unwrap_or(3600);
    *SP_ACCESS.lock().unwrap() = Some((access.to_string(), Instant::now() + Duration::from_secs(exp.saturating_sub(60))));
    if let Some(r) = v.get("refresh_token").and_then(|r| r.as_str()) {
        let st = app.state::<crate::AppState>();
        let mut s = st.settings.lock().unwrap();
        s.spotify_refresh = r.to_string();
        crate::settings::save(app, &s);
    }
    Ok(())
}

async fn token(app: &AppHandle) -> Result<String, String> {
    let cached = SP_ACCESS.lock().unwrap().clone();
    if let Some((t, exp)) = cached {
        if Instant::now() < exp {
            return Ok(t);
        }
    }
    let (cid, refresh) = {
        let st = app.state::<crate::AppState>();
        let s = st.settings.lock().unwrap();
        (s.spotify_client_id.clone(), s.spotify_refresh.clone())
    };
    if refresh.is_empty() || cid.is_empty() {
        return Err("not_connected".into());
    }
    let v: Value = client()
        .post("https://accounts.spotify.com/api/token")
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh.as_str()),
            ("client_id", cid.as_str()),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    store_tokens(app, &v)?;
    SP_ACCESS
        .lock()
        .unwrap()
        .clone()
        .map(|(t, _)| t)
        .ok_or_else(|| "token error".into())
}

pub async fn spotify_search(app: &AppHandle, q: &str) -> Result<Vec<Track>, String> {
    let tok = token(app).await?;
    let v: Value = client()
        .get(format!("https://api.spotify.com/v1/search?q={}&type=track&limit=24", enc(q)))
        .bearer_auth(tok)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let items = v.pointer("/tracks/items").and_then(|i| i.as_array()).cloned().unwrap_or_default();
    Ok(items
        .iter()
        .map(|x| {
            let s = |p: &str| x.pointer(p).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let artists = x
                .get("artists")
                .and_then(|a| a.as_array())
                .map(|a| a.iter().filter_map(|n| n.get("name").and_then(|n| n.as_str())).collect::<Vec<_>>().join(", "))
                .unwrap_or_default();
            Track {
                id: s("/id"),
                title: s("/name"),
                artist: artists,
                album: s("/album/name"),
                art: s("/album/images/0/url"),
                preview: String::new(),
                url: s("/external_urls/spotify"),
                duration_ms: x.get("duration_ms").and_then(|d| d.as_i64()).unwrap_or(0),
                source: "spotify".into(),
                play: s("/uri"),
            }
        })
        .filter(|t| !t.title.is_empty())
        .collect())
}

/// Play a track URI on the user's Spotify app via Spotify Connect.
/// Returns "played" or "opened" (fallback: just open the URI in the app).
pub async fn spotify_play(app: &AppHandle, uri: &str) -> Result<String, String> {
    let tok = match token(app).await {
        Ok(t) => t,
        Err(_) => {
            crate::desktop::open_external(uri);
            return Ok("opened".into());
        }
    };
    let mut launched = false;
    for attempt in 0..5 {
        let v: Value = client()
            .get("https://api.spotify.com/v1/me/player/devices")
            .bearer_auth(&tok)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .unwrap_or(Value::Null);
        let devices = v.get("devices").and_then(|d| d.as_array()).cloned().unwrap_or_default();
        let pick = devices
            .iter()
            .find(|d| d.get("is_active").and_then(|a| a.as_bool()).unwrap_or(false))
            .or_else(|| devices.iter().find(|d| d.get("type").and_then(|t| t.as_str()) == Some("Computer")))
            .or_else(|| devices.first())
            .and_then(|d| d.get("id").and_then(|i| i.as_str()).map(String::from));
        if let Some(id) = pick {
            let r = client()
                .put(format!("https://api.spotify.com/v1/me/player/play?device_id={}", enc(&id)))
                .bearer_auth(&tok)
                // a single song plays by itself; an album / playlist plays as a whole
                .json(&if uri.starts_with("spotify:track:") { json!({ "uris": [uri] }) } else { json!({ "context_uri": uri }) })
                .send()
                .await
                .map_err(|e| e.to_string())?;
            let code = r.status().as_u16();
            if code < 300 {
                return Ok("played".into());
            }
            if code == 403 {
                // not Premium: let the app handle it
                crate::desktop::open_external(uri);
                return Ok("opened".into());
            }
        }
        if !launched {
            crate::desktop::open_external("spotify:");
            launched = true;
        }
        tokio::time::sleep(Duration::from_millis(if attempt == 0 { 3500 } else { 2000 })).await;
    }
    crate::desktop::open_external(uri);
    Ok("opened".into())
}

/// Spotify albums and playlists matching a query (needs the account connected).
pub async fn spotify_collections(app: &AppHandle, q: &str) -> Result<Vec<Collection>, String> {
    let tok = token(app).await?;
    let v: Value = client()
        .get(format!("https://api.spotify.com/v1/search?q={}&type=album,playlist&limit=8", enc(q)))
        .bearer_auth(tok)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (kind, path) in [("album", "/albums/items"), ("playlist", "/playlists/items")] {
        for x in v.pointer(path).and_then(|i| i.as_array()).cloned().unwrap_or_default() {
            if x.is_null() {
                continue; // Spotify returns nulls for playlists it hides
            }
            let s = |p: &str| x.pointer(p).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let sub = if kind == "album" {
                x.get("artists")
                    .and_then(|a| a.as_array())
                    .map(|a| a.iter().filter_map(|n| n.get("name").and_then(|n| n.as_str())).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default()
            } else {
                s("/owner/display_name")
            };
            if s("/uri").is_empty() {
                continue;
            }
            out.push(Collection { id: s("/uri"), kind: kind.into(), title: s("/name"), sub, art: s("/images/0/url"), source: "spotify".into() });
        }
    }
    Ok(out)
}
