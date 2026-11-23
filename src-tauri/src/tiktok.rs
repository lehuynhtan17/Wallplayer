//! TikTok channel.
//!
//! TikTok has no public feed API. So an off-screen TikTok page (with your own TikTok session: the same
//! WebView2 cookies as the sign-in window) opens For You or a search, and a small script in that page
//! collects the videos TikTok's own site receives. The skin plays each video in TikTok's official embed
//! player (www.tiktok.com/player/v1/<id>). The page stays muted and paused; it is never closed at
//! runtime (closing a busy WebView2 window can take the shared engine down), only emptied.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
#[cfg(windows)]
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2_2, COREWEBVIEW2_WEB_RESOURCE_CONTEXT, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA,
};
#[cfg(windows)]
use webview2_com::{ExecuteScriptCompletedHandler, WebResourceRequestedEventHandler};
#[cfg(windows)]
use windows_core::{Interface, HSTRING};
#[cfg(windows)]
use windows::Win32::Foundation::HWND;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_TOOLWINDOW};

const LABEL: &str = "tiktok-feed";
/// A second off-screen TikTok page, on the video you act on (like, save, comments).
const ACT: &str = "tiktok-act";
const OFF: f64 = -32000.0;
/// The hidden pages load no videos or pictures (only TikTok's lists), so the video you watch gets the
/// whole connection; off while a page is on screen for TikTok's check (its puzzle is a picture).
static QUIET: AtomicBool = AtomicBool::new(true);

/// One TikTok video, as the skin needs it.
#[derive(Serialize, Clone, Debug, Default)]
pub struct Clip {
    pub id: String,
    pub desc: String,
    pub cover: String,
    pub duration: f64,
    pub width: f64,
    pub height: f64,
    pub author: String,
    pub nick: String,
    pub avatar: String,
    pub music: String,
    pub likes: f64,
    pub comments: f64,
    pub shares: f64,
    pub plays: f64,
    pub saves: f64,
    /// you liked it / saved it (signed in)
    pub liked: bool,
    pub saved: bool,
}

/// One comment under a video.
#[derive(Serialize, Clone, Debug, Default)]
pub struct Comment {
    pub id: String,
    pub text: String,
    pub likes: f64,
    pub replies: f64,
    pub time: f64,
    pub nick: String,
    pub user: String,
    pub avatar: String,
}

/// What the page collected since the last call.
#[derive(Serialize, Default)]
pub struct Take {
    pub clips: Vec<Clip>,
    /// TikTok says there are more videos to load
    pub more: bool,
    /// TikTok shows a "verify you are human" check in the page
    pub captcha: bool,
    pub ready: bool,
}

/// Runs in the TikTok page before TikTok's own scripts. It reads the answers of the feed / search
/// requests the site makes (it changes nothing in them), keeps every video silent and still, and lets
/// Wallplayer take the list and ask for more (by scrolling the page).
const HOOK: &str = r#"(() => {
  if (window.top !== window || !/(^|\.)tiktok\.com$/.test(location.hostname) || window.__wptt) return;
  const Q = [], seen = new Set(), C = []; let more = true, res = null;
  const WANT = /\/api\/(recommend\/item_list|search\/(item|general)\/full|comment\/list|commit\/item\/digg|item\/collect)\//;
  const num = (s, s2, k) => +((s && s[k] != null ? s[k] : s2 && s2[k]) || 0) || 0;
  const pick = it => {
    if (!it || !it.id || !it.video || it.isAd || it.imagePost || it.liveRoomInfo) return null;
    const v = it.video, a = it.author || {}, m = it.music || {}, s = it.stats, s2 = it.statsV2;
    if (!(+v.duration > 0)) return null;
    return {id: String(it.id), desc: String(it.desc || ''), cover: String(v.cover || v.originCover || ''), duration: +v.duration || 0,
      width: +v.width || 0, height: +v.height || 0, author: String(a.uniqueId || ''), nick: String(a.nickname || ''),
      avatar: String(a.avatarThumb || a.avatarMedium || ''), music: String(m.title || ''),
      likes: num(s, s2, 'diggCount'), comments: num(s, s2, 'commentCount'), shares: num(s, s2, 'shareCount'), plays: num(s, s2, 'playCount'),
      saves: num(s, s2, 'collectCount'), liked: !!it.digged, saved: !!it.collected};
  };
  const cm = c => { const u = c.user || {}, av = (u.avatar_thumb && u.avatar_thumb.url_list) || [];
    return {id: String(c.cid || ''), item: String(c.aweme_id || ''), text: String(c.text || ''), likes: +c.digg_count || 0, replies: +c.reply_comment_total || 0,
      time: +c.create_time || 0, nick: String(u.nickname || ''), user: String(u.unique_id || ''), avatar: String(av[0] || '')} };
  const eat = (url, body) => {
    if (!WANT.test(url)) return;
    let j = body; if (typeof j === 'string'){ try { j = JSON.parse(j) } catch(e){ return } }
    if (!j || typeof j !== 'object') return;
    if (/comment\/list/.test(url)){ for (const c of j.comments || []) C.push(cm(c)); return }
    if (/commit\/item\/digg|item\/collect/.test(url)){ res = {ok: j.status_code === 0 || j.statusCode === 0}; return }
    const raw = j.itemList || j.item_list || (Array.isArray(j.data) ? j.data.map(d => d && d.item).filter(Boolean) : []);
    if ('hasMore' in j) more = !!j.hasMore; else if ('has_more' in j) more = !!j.has_more;
    for (const it of raw || []){ const c = pick(it); if (c && !seen.has(c.id)){ seen.add(c.id); Q.push(c) } }
  };
  try {
    const F = window.fetch;
    if (F) window.fetch = new Proxy(F, {apply(t, self, args){
      const p = Reflect.apply(t, self, args);
      try { const a = args[0], url = typeof a === 'string' ? a : (a && a.url) || String(a);
        if (WANT.test(url)) p.then(r => r.clone().text()).then(x => eat(url, x)).catch(() => {}) } catch(e){}
      return p }});
    const XP = XMLHttpRequest.prototype;
    XP.open = new Proxy(XP.open, {apply(t, self, args){ try { self.__wpu = String(args[1]) } catch(e){} return Reflect.apply(t, self, args) }});
    XP.send = new Proxy(XP.send, {apply(t, self, args){
      try { if (self.__wpu && WANT.test(self.__wpu)) self.addEventListener('load', () => { try { eat(self.__wpu, self.responseType === 'json' ? self.response : self.responseText) } catch(e){} }) } catch(e){}
      return Reflect.apply(t, self, args) }});
  } catch(e){}
  try { HTMLMediaElement.prototype.play = function(){ try { this.muted = true; this.pause() } catch(e){} return Promise.resolve() } } catch(e){}
  setInterval(() => { document.querySelectorAll('video,audio').forEach(v => { try { v.muted = true; if (!v.paused) v.pause() } catch(e){} }) }, 2000);
  Object.defineProperty(window, '__wptt', {enumerable: false, value: {
    take(){ const clips = Q.splice(0, Q.length);
      const captcha = [...document.querySelectorAll('[id*="captcha" i],[class*="captcha" i]')].some(e => e.offsetWidth > 150 && e.offsetHeight > 120);
      return {clips, more, captcha, ready: document.readyState !== 'loading'} },
    // the video page (second window): press its like / save / comments button
    find(kind, id){
      const sel = {like: '[data-e2e="like-icon"],[data-e2e="browse-like-icon"]', save: '[data-e2e="favorite-icon"],[data-e2e="browse-favorite-icon"],[data-e2e="undefined-icon"]',
        comment: '[data-e2e="comment-icon"],[data-e2e="browse-comment-icon"]'}[kind];
      const all = [...document.querySelectorAll(sel)].filter(b => b.offsetParent !== null || b.getClientRects().length);
      const mine = id && all.find(b => { const box = b.closest('article,section,[id^="one-column-item"],[class*="ItemContainer"],[class*="VideoWrapper"]');
        return box && box.querySelector('[href*="' + id + '"],[id*="' + id + '"],[data-video-id="' + id + '"]') });
      return mine || all[0] || null },
    has(id){ return (!id || location.pathname.indexOf(id) >= 0) && !!this.find('like', id) },
    act(kind, id){ const b = this.find(kind, id); if (!b) return false;
      res = null; this.before = b.outerHTML; this.b = b; b.click(); return true },
    check(){ return [...document.querySelectorAll('[id*="captcha" i],[class*="captcha" i]')].some(e => e.offsetWidth > 150 && e.offsetHeight > 120) },
    acted(){ if (res) return {done: true, ok: res.ok}; const b = this.b; return {done: false, changed: !!(b && b.isConnected && b.outerHTML !== this.before)} },
    openComments(id){ if (C.some(c => c.item === id)) return true; const b = this.find('comment', id); if (b) b.click();
      // the side panel may open on "You may like": pick its Comments tab
      setTimeout(() => { const t = [...document.querySelectorAll('[role="tab"],[data-e2e*="comment-tab"],[class*="TabItem"]')].find(e => /comment|bình luận/i.test(e.textContent || '') || /comment/i.test(e.getAttribute('data-e2e') || ''));
        if (t && t.getAttribute('aria-selected') !== 'true') t.click() }, 700);
      // and if the page still has not asked TikTok for them, ask (TikTok's own request code signs it)
      setTimeout(() => { if (!C.some(c => c.item === id)) fetch('/api/comment/list/?aid=1988&count=30&cursor=0&aweme_id=' + id, {credentials: 'include'}).catch(() => {}) }, 2600);
      return !!b },
    comments(item){ return C.filter(c => !item || c.item === item) },
    more(){ const s = [document.scrollingElement, ...document.querySelectorAll('div,main')].filter(e => e && e.scrollHeight > e.clientHeight + 200 && (e === document.scrollingElement || /(auto|scroll)/.test(getComputedStyle(e).overflowY)));
      s.forEach(e => { e.scrollTop = e.scrollHeight });
      const a = document.querySelectorAll('article,[data-e2e="search_video-item"]'); if (a.length) a[a.length - 1].scrollIntoView({block: 'end'}) }
  }});
})();"#;

fn url_for(mode: &str, query: &str) -> String {
    let q = query.trim();
    if mode == "search" && !q.is_empty() {
        format!("https://www.tiktok.com/search/video?q={}", urlencoding::encode(q))
    } else {
        "https://www.tiktok.com/foryou".to_string()
    }
}

/// Answer the hidden page's video and picture requests with nothing (see QUIET).
/// (Windows only: WebKit has no such hook; there the hidden page just loads them.)
#[cfg(not(windows))]
fn quiet(_w: &WebviewWindow) {}

#[cfg(windows)]
fn quiet(w: &WebviewWindow) {
    let _ = w.with_webview(|wv| unsafe {
        let Ok(core) = wv.controller().CoreWebView2() else { return };
        let Ok(env) = core.cast::<ICoreWebView2_2>().and_then(|c| c.Environment()) else { return };
        for ctx in [COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE] {
            let _ = core.AddWebResourceRequestedFilter(&HSTRING::from("*"), ctx);
        }
        let handler = WebResourceRequestedEventHandler::create(Box::new(move |_sender, args| {
            let Some(args) = args else { return Ok(()) };
            if !QUIET.load(Ordering::Relaxed) {
                return Ok(());
            }
            let mut ctx = COREWEBVIEW2_WEB_RESOURCE_CONTEXT::default();
            args.ResourceContext(&mut ctx)?;
            if ctx == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA || ctx == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE {
                if let Ok(r) = env.CreateWebResourceResponse(None, 204, &HSTRING::from("No Content"), &HSTRING::from("")) {
                    let _ = args.SetResponse(&r);
                }
            }
            Ok(())
        }));
        let mut token = 0i64;
        let _ = core.add_WebResourceRequested(&handler, &mut token);
    });
}

/// Out of sight and out of Alt+Tab, but still "on screen" for TikTok's page so it keeps loading.
fn tuck(w: &WebviewWindow) {
    let _ = w.set_position(tauri::LogicalPosition::new(OFF, OFF));
    #[cfg(not(windows))]
    let _ = w.set_skip_taskbar(true);
    #[cfg(windows)]
    if let Ok(h) = w.hwnd() {
        unsafe {
            let hwnd = HWND(h.0 as *mut core::ffi::c_void);
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_TOOLWINDOW.0 as isize);
        }
    }
}

/// Open For You (mode "foryou") or a search (mode "search") in the off-screen page.
pub fn feed(app: &AppHandle, mode: &str, query: &str) -> Result<(), String> {
    let u = Url::parse(&url_for(mode, query)).map_err(|e| e.to_string())?;
    if let Some(w) = app.get_webview_window(LABEL) {
        return w.navigate(u).map_err(|e| e.to_string());
    }
    let w = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::External(u))
        .title("Wallplayer · TikTok")
        .inner_size(480.0, 860.0)
        .position(OFF, OFF)
        .decorations(false)
        .skip_taskbar(true)
        .focused(false)
        .user_agent(crate::accounts::UA)
        .additional_browser_args(crate::accounts::BROWSER_ARGS)
        .initialization_script(HOOK)
        .build()
        .map_err(|e| e.to_string())?;
    tuck(&w);
    quiet(&w);
    let w2 = w.clone();
    w.on_window_event(move |e| {
        if let WindowEvent::CloseRequested { api, .. } = e {
            api.prevent_close();
            QUIET.store(true, Ordering::Relaxed);
            tuck(&w2);
        }
    });
    crate::guard::watch(&w, false);
    Ok(())
}

/// Leaving the TikTok channel: empty the page (it stays, for next time).
pub fn stop(app: &AppHandle) {
    for label in [LABEL, ACT] {
        if let Some(w) = app.get_webview_window(label) {
            tuck(&w);
            if let Ok(u) = Url::parse("about:blank") {
                let _ = w.navigate(u);
            }
        }
    }
    *ACT_ITEM.lock().unwrap_or_else(|e| e.into_inner()) = String::new();
}

/// Scroll the page so TikTok loads the next videos.
pub fn more(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.eval("window.__wptt && window.__wptt.more()");
    }
}

/// Bring a TikTok page on screen so you can answer TikTok's check yourself ("video": the page of the
/// video you act on; otherwise the feed page); `false` puts it away again.
pub fn show(app: &AppHandle, on: bool, video: bool) {
    let Some(w) = app.get_webview_window(if video { ACT } else { LABEL }) else { return };
    if on {
        QUIET.store(false, Ordering::Relaxed);
        let _ = w.eval("location.reload()");
        let _ = w.center();
        let _ = w.set_focus();
    } else {
        QUIET.store(true, Ordering::Relaxed);
        tuck(&w);
    }
}

fn https(v: &Value, k: &str) -> String {
    let s = v.get(k).and_then(|x| x.as_str()).unwrap_or("");
    if s.starts_with("https://") { s.to_string() } else { String::new() }
}

fn text(v: &Value, k: &str, max: usize) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").chars().take(max).collect()
}

fn num(v: &Value, k: &str) -> f64 {
    v.get(k).and_then(|x| x.as_f64()).filter(|x| x.is_finite() && *x >= 0.0).unwrap_or(0.0)
}

fn parse(res: &str) -> Take {
    let Ok(v) = serde_json::from_str::<Value>(res) else { return Take::default() };
    if !v.is_object() {
        return Take::default();
    }
    let clips = v
        .get("clips")
        .and_then(|c| c.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|c| {
                    let id = c.get("id")?.as_str()?;
                    if id.len() < 8 || id.len() > 25 || !id.bytes().all(|b| b.is_ascii_digit()) {
                        return None;
                    }
                    Some(Clip {
                        id: id.to_string(),
                        desc: text(c, "desc", 400),
                        cover: https(c, "cover"),
                        duration: num(c, "duration"),
                        width: num(c, "width"),
                        height: num(c, "height"),
                        author: text(c, "author", 64),
                        nick: text(c, "nick", 80),
                        avatar: https(c, "avatar"),
                        music: text(c, "music", 120),
                        likes: num(c, "likes"),
                        comments: num(c, "comments"),
                        shares: num(c, "shares"),
                        plays: num(c, "plays"),
                        saves: num(c, "saves"),
                        liked: c.get("liked").and_then(|x| x.as_bool()).unwrap_or(false),
                        saved: c.get("saved").and_then(|x| x.as_bool()).unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Take {
        clips,
        more: v.get("more").and_then(|x| x.as_bool()).unwrap_or(true),
        captcha: v.get("captcha").and_then(|x| x.as_bool()).unwrap_or(false),
        ready: v.get("ready").and_then(|x| x.as_bool()).unwrap_or(false),
    }
}

/// Run a script in one of the TikTok pages and get its result (JSON), or None.
#[cfg(target_os = "linux")]
async fn run(app: &AppHandle, label: &str, js: &str) -> Option<String> {
    use javascriptcore::ValueExt;
    use webkit2gtk::WebViewExt;
    let w = app.get_webview_window(label)?;
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let tx = Arc::new(Mutex::new(Some(tx)));
    let js = js.to_string();
    w.with_webview(move |wv| {
        let tx2 = tx.clone();
        wv.inner().evaluate_javascript(&js, None, None, None::<&gio::Cancellable>, move |r| {
            // the result as JSON, like WebView2's ExecuteScript ("null" for undefined)
            let json = r.ok().and_then(|v| v.to_json(0)).map(|s| s.to_string()).unwrap_or_else(|| "null".into());
            if let Some(t) = tx2.lock().ok().and_then(|mut g| g.take()) {
                let _ = t.send(json);
            }
        });
    })
    .ok()?;
    match tokio::time::timeout(Duration::from_secs(4), rx).await {
        Ok(Ok(res)) => Some(res),
        _ => None,
    }
}

/// Run a script in one of the TikTok pages and get its result (JSON), or None. Every script here is
/// an expression: WebKit returns its JSON text as a string.
#[cfg(target_os = "macos")]
async fn run(app: &AppHandle, label: &str, js: &str) -> Option<String> {
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSError, NSString};
    use objc2_web_kit::WKWebView;
    let w = app.get_webview_window(label)?;
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let tx = Arc::new(Mutex::new(Some(tx)));
    let js = format!("(() => {{ const r = ({js}); return r === undefined ? 'null' : JSON.stringify(r) }})()");
    w.with_webview(move |wv| unsafe {
        let view: &WKWebView = &*(wv.inner() as *const WKWebView);
        let tx2 = tx.clone();
        let done = block2::RcBlock::new(move |res: *mut AnyObject, _err: *mut NSError| {
            let text = if res.is_null() {
                "null".to_string()
            } else {
                (*res).downcast_ref::<NSString>().map(|s| s.to_string()).unwrap_or_else(|| "null".into())
            };
            if let Some(t) = tx2.lock().ok().and_then(|mut g| g.take()) {
                let _ = t.send(text);
            }
        });
        view.evaluateJavaScript_completionHandler(&NSString::from_str(&js), Some(&done));
    })
    .ok()?;
    match tokio::time::timeout(Duration::from_secs(4), rx).await {
        Ok(Ok(res)) => Some(res),
        _ => None,
    }
}

#[cfg(windows)]
async fn run(app: &AppHandle, label: &str, js: &str) -> Option<String> {
    let w = app.get_webview_window(label)?;
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let tx = Arc::new(Mutex::new(Some(tx)));
    let js = windows_core::HSTRING::from(js);
    w.with_webview(move |wv| unsafe {
        let Ok(core) = wv.controller().CoreWebView2() else { return };
        let tx2 = tx.clone();
        let handler = ExecuteScriptCompletedHandler::create(Box::new(move |_err, res| {
            if let Some(t) = tx2.lock().ok().and_then(|mut g| g.take()) {
                let _ = t.send(res);
            }
            Ok(())
        }));
        let _ = core.ExecuteScript(&js, &handler);
    })
    .ok()?;
    match tokio::time::timeout(Duration::from_secs(4), rx).await {
        Ok(Ok(res)) => Some(res),
        _ => None,
    }
}

/// The videos the page collected since the last call.
pub async fn take(app: &AppHandle) -> Result<Take, String> {
    Ok(run(app, LABEL, "window.__wptt ? window.__wptt.take() : null").await.map(|r| parse(&r)).unwrap_or_default())
}

static ACT_ITEM: Mutex<String> = Mutex::new(String::new());

/// Open the video's own page in the second off-screen window and wait for its buttons.
async fn item(app: &AppHandle, author: &str, id: &str) -> Result<(), String> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) || author.chars().any(|c| !(c.is_alphanumeric() || c == '.' || c == '_')) {
        return Err("Not a TikTok video".into());
    }
    let url = format!("https://www.tiktok.com/@{author}/video/{id}");
    let same = *ACT_ITEM.lock().unwrap_or_else(|e| e.into_inner()) == id;
    if !same {
        let u = Url::parse(&url).map_err(|e| e.to_string())?;
        if let Some(w) = app.get_webview_window(ACT) {
            w.navigate(u).map_err(|e| e.to_string())?;
        } else {
            let w = WebviewWindowBuilder::new(app, ACT, WebviewUrl::External(u))
                .title("Wallplayer · TikTok video")
                .inner_size(1280.0, 900.0)
                .position(OFF, OFF)
                .decorations(false)
                .skip_taskbar(true)
                .focused(false)
                .user_agent(crate::accounts::UA)
                .additional_browser_args(crate::accounts::BROWSER_ARGS)
                .initialization_script(HOOK)
                .build()
                .map_err(|e| e.to_string())?;
            tuck(&w);
            quiet(&w);
            let w2 = w.clone();
            w.on_window_event(move |e| {
                if let WindowEvent::CloseRequested { api, .. } = e {
                    api.prevent_close();
                    QUIET.store(true, Ordering::Relaxed);
                    tuck(&w2);
                }
            });
            crate::guard::watch(&w, false);
        }
        *ACT_ITEM.lock().unwrap_or_else(|e| e.into_inner()) = id.to_string();
        tokio::time::sleep(Duration::from_millis(1200)).await;
    }
    let js = format!("!!(window.__wptt && window.__wptt.has('{id}'))");
    for _ in 0..40 {
        if run(app, ACT, &js).await.as_deref() == Some("true") {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    *ACT_ITEM.lock().unwrap_or_else(|e| e.into_inner()) = String::new();
    let url = run(app, ACT, "location.href").await.unwrap_or_default();
    crate::guard::log(app, &format!("tiktok: the video page has no buttons ({id}, now at {url})"));
    Err("TikTok did not open that video".into())
}

/// Like or save a video (kind "like" / "save") with your TikTok account: presses that button on the
/// video's own TikTok page. Each call flips it (liked ↔ not liked). Ok(true) when TikTok took it.
pub async fn act(app: &AppHandle, author: &str, id: &str, kind: &str) -> Result<bool, String> {
    item(app, author, id).await?;
    let k = if kind == "save" { "save" } else { "like" };
    if run(app, ACT, "!!(window.__wptt && window.__wptt.check())").await.as_deref() == Some("true") {
        crate::guard::log(app, "tiktok: the video page wants a check");
        return Err("check".into());
    }
    if run(app, ACT, &format!("window.__wptt.act('{k}', '{id}')")).await.as_deref() != Some("true") {
        crate::guard::log(app, &format!("tiktok: no {k} button for {id}"));
        return Err("TikTok's button is not there".into());
    }
    for _ in 0..25 {
        tokio::time::sleep(Duration::from_millis(300)).await;
        let Some(r) = run(app, ACT, "window.__wptt.acted()").await else { continue };
        let v: Value = serde_json::from_str(&r).unwrap_or(Value::Null);
        if v.get("done").and_then(|x| x.as_bool()) == Some(true) {
            let ok = v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false);
            crate::guard::log(app, &format!("tiktok: {k} {id}: TikTok answered {}", if ok { "ok" } else { "no" }));
            return Ok(ok);
        }
        if v.get("changed").and_then(|x| x.as_bool()) == Some(true) {
            // the button changed; give TikTok's answer a moment, else trust the button
            tokio::time::sleep(Duration::from_millis(900)).await;
            if let Some(r2) = run(app, ACT, "window.__wptt.acted()").await {
                let v2: Value = serde_json::from_str(&r2).unwrap_or(Value::Null);
                if v2.get("done").and_then(|x| x.as_bool()) == Some(true) {
                    return Ok(v2.get("ok").and_then(|x| x.as_bool()).unwrap_or(false));
                }
            }
            crate::guard::log(app, &format!("tiktok: {k} {id}: the button changed"));
            return Ok(true);
        }
    }
    if run(app, ACT, "!!(window.__wptt && window.__wptt.check())").await.as_deref() == Some("true") {
        crate::guard::log(app, "tiktok: the video page wants a check");
        return Err("check".into());
    }
    crate::guard::log(app, &format!("tiktok: {k} {id}: no answer from TikTok"));
    Ok(false)
}

/// The first comments under a video (opens its comment panel on the video's TikTok page).
pub async fn comments(app: &AppHandle, author: &str, id: &str) -> Result<Vec<Comment>, String> {
    item(app, author, id).await?;
    if run(app, ACT, &format!("window.__wptt.openComments('{id}')")).await.as_deref() != Some("true") {
        crate::guard::log(app, &format!("tiktok: no comment button for {id}"));
    }
    let js = format!("window.__wptt.comments('{id}')");
    for _ in 0..24 {
        tokio::time::sleep(Duration::from_millis(400)).await;
        let Some(r) = run(app, ACT, &js).await else { continue };
        let list: Vec<Value> = serde_json::from_str(&r).unwrap_or_default();
        if !list.is_empty() {
            let mut seen = std::collections::HashSet::new();
            return Ok(list
                .iter()
                .filter(|c| seen.insert(text(c, "id", 40)))
                .take(40)
                .map(|c| Comment {
                    id: text(c, "id", 40),
                    text: text(c, "text", 600),
                    likes: num(c, "likes"),
                    replies: num(c, "replies"),
                    time: num(c, "time"),
                    nick: text(c, "nick", 80),
                    user: text(c, "user", 64),
                    avatar: https(c, "avatar"),
                })
                .collect());
        }
    }
    if run(app, ACT, "!!(window.__wptt && window.__wptt.check())").await.as_deref() == Some("true") {
        crate::guard::log(app, "tiktok: the video page wants a check");
        return Err("check".into());
    }
    crate::guard::log(app, &format!("tiktok: no comments came for {id}"));
    Ok(Vec::new())
}
