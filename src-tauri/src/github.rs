//! Star Wallplayer on GitHub without leaving the app.
//!
//! GitHub only lets a signed-in account star a repo. So the user signs in to GitHub once in an in-app
//! window (accounts.rs, like YouTube / TikTok: the session stays in the app's own WebView profile, we
//! never see the password). After that an off-screen GitHub page opens the repo, reads whether *this*
//! account already starred it, and presses GitHub's own Star button when the user asks. Same idea as the
//! TikTok like button, shared by Windows, Linux and macOS.
//!
//! Reminders: the app counts the days it was used. After 3 days of use, if the repo is not starred yet,
//! the skin shows a small card; if it is dismissed, it comes back 30 days later. Nothing is sent
//! anywhere except to github.com, and only when the user presses the star.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder, WindowEvent};

const REPO_URL: &str = "https://github.com/lehuynhtan17/Wallplayer";
const LABEL: &str = "github-star";
const DAY: u64 = 86_400;
/// The reminder card shows after this many days of use, then every REMIND_DAYS while not starred.
const FIRST_AFTER_DAYS: u32 = 3;
const REMIND_DAYS: u64 = 30;
const COOKIES: &[&str] = &["user_session"];

/// Only one page action at a time (the hidden page is shared).
static BUSY: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Clone, Default)]
pub struct Star {
    /// signed in to GitHub inside Wallplayer
    pub signed_in: bool,
    /// Some(true/false) when GitHub told us for this account; None when we couldn't ask (signed out, offline)
    pub starred: Option<bool>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn signed_in(app: &AppHandle) -> bool {
    crate::accounts::has_cookie(app, "https://github.com", COOKIES)
}

/// Called once when the app starts: counts this day as a day of use.
pub fn note_use(app: &AppHandle) {
    let st = app.state::<crate::AppState>();
    let mut s = st.settings.lock().unwrap();
    let day = now() / DAY;
    if s.star_last_day != day {
        s.star_last_day = day;
        s.star_days = s.star_days.saturating_add(1);
        crate::settings::save(app, &s);
    }
}

fn due(s: &crate::settings::Settings) -> bool {
    if s.star_done {
        return false;
    }
    if s.star_asked == 0 {
        s.star_days >= FIRST_AFTER_DAYS
    } else {
        now().saturating_sub(s.star_asked) >= REMIND_DAYS * DAY
    }
}

fn remember(app: &AppHandle, f: impl FnOnce(&mut crate::settings::Settings)) {
    let st = app.state::<crate::AppState>();
    let mut s = st.settings.lock().unwrap();
    f(&mut s);
    crate::settings::save(app, &s);
}

/// The reminder card was shown: next one in 30 days.
pub fn asked(app: &AppHandle) {
    remember(app, |s| s.star_asked = now());
}

/// The off-screen GitHub page (kept for next time, never closed: closing a busy WebView2 window can take the
/// shared engine down).
fn page(app: &AppHandle, url: &str) -> Result<(), String> {
    let u = Url::parse(url).map_err(|e| e.to_string())?;
    if let Some(w) = app.get_webview_window(LABEL) {
        return w.navigate(u).map_err(|e| e.to_string());
    }
    let w = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::External(u))
        .title("Wallplayer · GitHub")
        .inner_size(1100.0, 800.0)
        .position(-32000.0, -32000.0)
        .decorations(false)
        .skip_taskbar(true)
        .focused(false)
        .user_agent(crate::accounts::UA)
        .additional_browser_args(crate::accounts::BROWSER_ARGS)
        // a link or pop-up on the page must never open another window or leave GitHub
        .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
        .on_navigation(|u| u.scheme() == "about" || (u.scheme() == "https" && u.host_str().map_or(false, |h| h == "github.com" || h.ends_with(".github.com"))))
        .build()
        .map_err(|e| e.to_string())?;
    crate::tiktok::tuck(&w);
    let w2 = w.clone();
    w.on_window_event(move |e| {
        if let WindowEvent::CloseRequested { api, .. } = e {
            api.prevent_close();
            crate::tiktok::tuck(&w2);
        }
    });
    crate::guard::watch(&w, false);
    Ok(())
}

/// What GitHub's own page says about this account: "starred", "not", "anon" (not signed in) or None.
/// The repo page carries `"star":{"viewerHasStarred":…,"canStar":…}` in its embedded data.
const READ: &str = r#"(function(){var s=document.querySelectorAll('script[data-target="react-app.embeddedData"]');
for(var i=0;i<s.length;i++){var m=/"star":\{"viewerHasStarred":(true|false),"canStar":(true|false)\}/.exec(s[i].textContent||'');
if(m){if(m[1]==='false'&&m[2]==='false')return 'anon';return m[1]==='true'?'starred':'not'}}
return document.readyState==='complete'?'none':'wait'})()"#;

/// GitHub's Star button (not the list arrow next to it).
const PRESS: &str = r#"(function(){var b=document.querySelector('button[data-testid="star-button"]');
if(!b){var all=document.querySelectorAll('button');for(var i=0;i<all.length;i++){if(/^\s*Star\s*$/.test(all[i].textContent||'')){b=all[i];break}}}
if(!b)return 'nobutton';b.click();return 'clicked'})()"#;

async fn read(app: &AppHandle) -> Option<String> {
    let r = crate::tiktok::run(app, LABEL, READ).await?;
    Some(r.trim().trim_matches('"').to_string())
}

/// Open the repo page and wait for GitHub's answer (up to ~20 s).
async fn load_state(app: &AppHandle) -> Option<bool> {
    page(app, REPO_URL).ok()?;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    for _ in 0..40 {
        match read(app).await.as_deref() {
            Some("starred") => return Some(true),
            Some("not") => return Some(false),
            Some("anon") => return None,
            _ => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
    None
}

/// Run `f` unless another GitHub action is going on.
struct Guard;
impl Guard {
    fn take() -> Option<Guard> {
        if BUSY.swap(true, Ordering::SeqCst) { None } else { Some(Guard) }
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
    }
}

/// Settings → Support: am I signed in, did this account star the repo.
pub async fn state(app: &AppHandle) -> Star {
    let signed = signed_in(app);
    let mut starred = None;
    if signed {
        if let Some(_g) = Guard::take() {
            starred = load_state(app).await;
        }
    }
    if starred == Some(true) {
        remember(app, |s| s.star_done = true);
    }
    Star { signed_in: signed, starred }
}

/// Startup: is it time to show the reminder card? (Asks GitHub first when signed in, so nobody who
/// already starred it is ever nagged.)
pub async fn check_due(app: &AppHandle) -> bool {
    let is_due = {
        let st = app.state::<crate::AppState>();
        let s = st.settings.lock().unwrap();
        due(&s)
    };
    if !is_due {
        return false;
    }
    if signed_in(app) {
        if let Some(_g) = Guard::take() {
            if load_state(app).await == Some(true) {
                remember(app, |s| s.star_done = true);
                return false;
            }
        }
    }
    true
}

/// Press Star. Err("signin") when GitHub isn't signed in yet (the skin then opens the sign-in window).
pub async fn star(app: &AppHandle) -> Result<bool, String> {
    if !signed_in(app) {
        return Err("signin".into());
    }
    let Some(_g) = Guard::take() else { return Err("busy".into()) };
    match load_state(app).await {
        Some(true) => {
            remember(app, |s| s.star_done = true);
            return Ok(true);
        }
        Some(false) => {}
        None => {
            // the page says "anon": the session ended
            return Err(if signed_in(app) { "GitHub's page didn't answer".into() } else { "signin".into() });
        }
    }
    match crate::tiktok::run(app, LABEL, PRESS).await.as_deref().map(|s| s.trim_matches('"')) {
        Some("clicked") => {}
        _ => {
            crate::guard::log(app, "github: no Star button on the page");
            return Err("GitHub's Star button isn't there".into());
        }
    }
    // reload and ask again: the answer comes from GitHub, not from the button
    for _ in 0..3 {
        tokio::time::sleep(Duration::from_millis(1800)).await;
        if load_state(app).await == Some(true) {
            remember(app, |s| s.star_done = true);
            crate::guard::log(app, "github: starred");
            return Ok(true);
        }
    }
    crate::guard::log(app, "github: pressed Star, GitHub did not confirm");
    Ok(false)
}
