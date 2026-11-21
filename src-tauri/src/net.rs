//! Network lookups done natively (no CORS limits): song search (iTunes),
//! YouTube video ids, synced lyrics (LRCLIB), image fetch for colour sampling.

use std::sync::OnceLock;
use std::time::Duration;

use base64::Engine;
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};

pub fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36 Wallplayer/0.1")
            .timeout(Duration::from_secs(12))
            .build()
            .expect("http client")
    })
}

fn enc(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

#[derive(Serialize, Clone, Debug)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art: String,
    pub preview: String,
    pub url: String,
    pub duration_ms: i64,
    /// platform the result came from: ytmusic | soundcloud | spotify | apple
    pub source: String,
    /// what the player needs: YouTube videoId, SoundCloud permalink, Spotify URI, Apple URL
    pub play: String,
}

pub async fn search_music(query: &str) -> Result<Vec<Track>, String> {
    let url = format!(
        "https://itunes.apple.com/search?term={}&entity=song&media=music&limit=25",
        enc(query)
    );
    let v: Value = client()
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let s = |x: &Value, k: &str| x.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    Ok(v.get("results")
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .map(|x| Track {
                    id: x.get("trackId").map(|v| v.to_string()).unwrap_or_default(),
                    title: s(x, "trackName"),
                    artist: s(x, "artistName"),
                    album: s(x, "collectionName"),
                    art: s(x, "artworkUrl100").replace("100x100bb", "600x600bb"),
                    preview: s(x, "previewUrl"),
                    url: s(x, "trackViewUrl"),
                    duration_ms: x.get("trackTimeMillis").and_then(|v| v.as_i64()).unwrap_or(0),
                    source: "apple".into(),
                    play: s(x, "trackViewUrl"),
                })
                .filter(|t| !t.title.is_empty())
                .collect()
        })
        .unwrap_or_default())
}

pub async fn youtube_search(query: &str) -> Result<Vec<String>, String> {
    // sp=EgIQAQ== → "Videos" filter
    let url = format!(
        "https://www.youtube.com/results?search_query={}&sp=EgIQAQ%3D%3D",
        enc(query)
    );
    let html = client()
        .get(url)
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("Cookie", "CONSENT=YES+1; SOCS=CAI")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let re = Regex::new(r#""videoId":"([A-Za-z0-9_-]{11})""#).unwrap();
    let mut out: Vec<String> = Vec::new();
    for c in re.captures_iter(&html) {
        let id = c[1].to_string();
        if !out.contains(&id) {
            out.push(id);
        }
        if out.len() >= 10 {
            break;
        }
    }
    Ok(out)
}

#[derive(Serialize, Clone, Debug)]
pub struct Lyrics {
    pub id: i64,
    pub track: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    pub synced: Option<String>,
    pub plain: Option<String>,
    /// lrclib | genius
    pub source: String,
    /// Genius page (lyrics fetched on demand)
    pub url: Option<String>,
}

fn to_lyrics(x: &Value) -> Lyrics {
    let s = |k: &str| x.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let o = |k: &str| x.get(k).and_then(|v| v.as_str()).map(|v| v.to_string());
    Lyrics {
        id: x.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
        track: s("trackName"),
        artist: s("artistName"),
        album: s("albumName"),
        duration: x.get("duration").and_then(|v| v.as_f64()).unwrap_or(0.0),
        synced: o("syncedLyrics"),
        plain: o("plainLyrics"),
        source: "lrclib".into(),
        url: None,
    }
}

async fn lrclib(url: String) -> Result<Value, String> {
    client()
        .get(url)
        .header("Lrclib-Client", "Wallplayer v0.1")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<Value>()
        .await
        .map_err(|e| e.to_string())
}

/// Strip "(feat. …)", " - Remastered 2011", "[Official Video]" etc.
fn clean_title(t: &str) -> String {
    let re = Regex::new(r"(?i)\s*([\(\[](feat|ft|with|official|lyric|audio|video|remaster|explicit)[^\)\]]*[\)\]]|\s-\s.*remaster.*$)").unwrap();
    re.replace_all(t, "").trim().to_string()
}

/// Query variants for a track. YouTube titles are often "Artist - Song (Official MV)" and the
/// "artist" is the channel ("SomethingVEVO", "Name - Topic").
fn variants(artist: &str, title: &str) -> Vec<(String, String)> {
    let re = Regex::new(r"(?i)\s*(-\s*topic|vevo|official)\s*$").unwrap();
    let first = artist.split(&[',', '&'][..]).next().unwrap_or(artist);
    let a = re.replace_all(first, "").trim().to_string();
    let t = clean_title(title);
    let mut v: Vec<(String, String)> = vec![(t.clone(), a.clone())];
    for sep in [" - ", " – ", " | "] {
        if let Some((l, r)) = t.split_once(sep) {
            let (l, r) = (l.trim().to_string(), r.trim().to_string());
            if !l.is_empty() && !r.is_empty() {
                v.push((r.clone(), l.clone())); // "Artist - Song"
                v.push((l, r)); // "Song - Artist"
            }
            break;
        }
    }
    v.push((t, String::new()));
    let mut out: Vec<(String, String)> = Vec::new();
    for x in v {
        if !x.0.is_empty() && !out.contains(&x) {
            out.push(x);
        }
    }
    out
}

fn sim(a: &str, b: &str) -> f64 {
    let (a, b) = (norm(a), norm(b));
    if a.is_empty() || b.is_empty() {
        0.0
    } else if a == b {
        1.0
    } else if a.contains(&b) || b.contains(&a) {
        0.8
    } else {
        0.0
    }
}

async fn lrclib_candidates(t: &str, a: &str, d: f64, out: &mut Vec<Lyrics>) {
    if d > 0.0 && !a.is_empty() {
        let url = format!("https://lrclib.net/api/get?track_name={}&artist_name={}&duration={}", enc(t), enc(a), d.round() as i64);
        if let Ok(v) = lrclib(url).await {
            if v.get("id").is_some() {
                out.push(to_lyrics(&v));
            }
        }
    }
    let url = if a.is_empty() {
        format!("https://lrclib.net/api/search?q={}", enc(t))
    } else {
        format!("https://lrclib.net/api/search?track_name={}&artist_name={}", enc(t), enc(a))
    };
    if let Ok(v) = lrclib(url).await {
        if let Some(arr) = v.as_array() {
            out.extend(arr.iter().take(12).map(to_lyrics));
        }
    }
}

/// NetEase Cloud Music: large catalogue (incl. Asian and Vietnamese songs) with timed lyrics.
async fn netease_candidates(t: &str, a: &str, out: &mut Vec<Lyrics>) {
    let url = format!(
        "https://music.163.com/api/search/get/web?csrf_token=&hlpretrans=NONE&hlposttrans=NONE&s={}&type=1&offset=0&total=true&limit=6",
        enc(&format!("{t} {a}"))
    );
    let Ok(r) = client().get(url).header("Referer", "https://music.163.com/").send().await else { return };
    let Ok(v) = r.json::<Value>().await else { return };
    for x in v.pointer("/result/songs").and_then(|s| s.as_array()).cloned().unwrap_or_default().into_iter().take(6) {
        out.push(Lyrics {
            id: x.get("id").and_then(|i| i.as_i64()).unwrap_or(0),
            track: x.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string(),
            artist: x.pointer("/artists/0/name").and_then(|n| n.as_str()).unwrap_or("").to_string(),
            album: String::new(),
            duration: x.get("duration").and_then(|d| d.as_f64()).unwrap_or(0.0) / 1000.0,
            synced: None,
            plain: None,
            source: "netease".into(),
            url: None,
        });
    }
}

async fn netease_lrc(id: i64) -> Option<String> {
    let v: Value = client()
        .get(format!("https://music.163.com/api/song/lyric?id={id}&lv=1&kv=1&tv=-1"))
        .header("Referer", "https://music.163.com/")
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let lrc = v.pointer("/lrc/lyric")?.as_str()?;
    let credit = Regex::new(r"^\[[^\]]+\]\s*(作词|作曲|编曲|制作|混音|母带|Lyrics|Composer|Arranger|Producer)\s*[:：]").unwrap();
    let kept: Vec<&str> = lrc.lines().filter(|l| !credit.is_match(l)).collect();
    let text = kept.join("\n");
    if Regex::new(r"\[\d+:\d+").unwrap().is_match(&text) {
        Some(text)
    } else {
        None
    }
}

pub async fn lyrics(artist: &str, title: &str, duration: Option<f64>) -> Result<Option<Lyrics>, String> {
    let d = duration.unwrap_or(0.0);
    let vars = variants(artist, title);
    let mut cands: Vec<Lyrics> = Vec::new();
    for (t, a) in vars.iter().take(3) {
        lrclib_candidates(t, a, d, &mut cands).await;
        // a timed lyric that fits the song length: no need to look further
        let good = |l: &Lyrics| l.synced.is_some() && d > 0.0 && (l.duration - d).abs() <= 2.5 && vars.iter().any(|(vt, _)| sim(&l.track, vt) >= 0.8);
        if cands.iter().any(good) {
            break;
        }
    }
    if !cands.iter().any(|l| l.synced.is_some()) {
        let (t, a) = &vars[0];
        netease_candidates(t, a, &mut cands).await;
        if vars.len() > 1 && !cands.iter().any(|l| l.source == "netease") {
            let (t, a) = &vars[1];
            netease_candidates(t, a, &mut cands).await;
        }
    }
    // Only songs whose title matches; nearest length first; timed beats plain; a matching artist helps.
    let mut ranked: Vec<(f64, Lyrics)> = cands
        .into_iter()
        .filter(|l| vars.iter().any(|(vt, _)| sim(&l.track, vt) >= 0.8))
        .map(|l| {
            let mut s = if l.synced.is_some() {
                0.0
            } else if l.source == "netease" {
                2.0
            } else {
                100.0
            };
            s += if d > 0.0 && l.duration > 0.0 { (l.duration - d).abs() } else { 5.0 };
            if !vars.iter().any(|(_, va)| sim(&l.artist, va) > 0.0) {
                s += 8.0;
            }
            (s, l)
        })
        .collect();
    ranked.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    for (_, mut l) in ranked.into_iter().take(4) {
        if l.source == "netease" {
            match netease_lrc(l.id).await {
                Some(lrc) => {
                    l.synced = Some(lrc);
                    return Ok(Some(l));
                }
                None => continue,
            }
        }
        return Ok(Some(l));
    }
    let (t, a) = &vars[0];
    Ok(genius_lyrics(a, t).await.ok().flatten())
}

/// NetEase "YRC": word-by-word timing, `[lineStart,lineDur](wordStart,wordDur,0)word…` in ms.
async fn netease_yrc(id: i64) -> Option<String> {
    let v: Value = client()
        .get(format!("https://music.163.com/api/song/lyric/v1?id={id}&cp=false&lv=0&kv=0&tv=0&rv=0&yv=0&ytv=0&yrv=0"))
        .header("Referer", "https://music.163.com/")
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let y = v.pointer("/yrc/lyric")?.as_str()?;
    if Regex::new(r"\(\d+,\d+(,-?\d+)?\)").unwrap().is_match(y) {
        Some(y.to_string())
    } else {
        None
    }
}

/// Word-by-word timing for a song, when NetEase has it. `id`: a NetEase song id already known.
/// Only a version with the same title, a matching artist and (when known) the same length.
pub async fn lyrics_words(artist: &str, title: &str, duration: Option<f64>, id: Option<i64>) -> Result<Option<String>, String> {
    if let Some(id) = id.filter(|i| *i > 0) {
        return Ok(netease_yrc(id).await);
    }
    let d = duration.unwrap_or(0.0);
    let vars = variants(artist, title);
    let named: Vec<&String> = vars.iter().map(|(_, a)| a).filter(|a| !a.is_empty()).collect();
    let mut cands: Vec<Lyrics> = Vec::new();
    for (t, a) in vars.iter().filter(|(_, a)| !a.is_empty()).take(2) {
        netease_candidates(t, a, &mut cands).await;
        if cands.iter().any(|l| vars.iter().any(|(vt, _)| sim(&l.track, vt) >= 0.8)) {
            break;
        }
    }
    let mut ok: Vec<(f64, i64)> = cands
        .into_iter()
        .filter(|l| l.id > 0 && vars.iter().any(|(vt, _)| sim(&l.track, vt) >= 0.8))
        .filter(|l| named.is_empty() || named.iter().any(|a| sim(&l.artist, a) > 0.0))
        .filter(|l| d <= 0.0 || l.duration <= 0.0 || (l.duration - d).abs() <= 3.0)
        .map(|l| (if d > 0.0 && l.duration > 0.0 { (l.duration - d).abs() } else { 1.5 }, l.id))
        .collect();
    ok.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    ok.dedup_by_key(|x| x.1);
    for (_, id) in ok.into_iter().take(2) {
        if let Some(y) = netease_yrc(id).await {
            return Ok(Some(y));
        }
    }
    Ok(None)
}

pub async fn lyrics_search(query: &str) -> Result<Vec<Lyrics>, String> {
    let mut out: Vec<Lyrics> = match lrclib(format!("https://lrclib.net/api/search?q={}", enc(query))).await {
        Ok(v) => v.as_array().map(|a| a.iter().take(10).map(to_lyrics).collect()).unwrap_or_default(),
        Err(_) => vec![],
    };
    if let Ok(hits) = genius_search(query).await {
        for h in hits.into_iter().take(4) {
            out.push(Lyrics {
                id: 0,
                track: h.title,
                artist: h.artist,
                album: String::new(),
                duration: 0.0,
                synced: None,
                plain: None,
                source: "genius".into(),
                url: Some(h.url),
            });
        }
    }
    Ok(out)
}

// ───────────────────────── Genius ─────────────────────────

#[derive(Serialize, Clone, Debug)]
pub struct GeniusHit {
    pub url: String,
    pub title: String,
    pub artist: String,
}

pub async fn genius_search(q: &str) -> Result<Vec<GeniusHit>, String> {
    let v: Value = client()
        .get(format!("https://genius.com/api/search/song?per_page=6&q={}", enc(q)))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let hits = v.pointer("/response/sections/0/hits").and_then(|h| h.as_array()).cloned().unwrap_or_default();
    Ok(hits
        .iter()
        .filter_map(|h| {
            let r = h.get("result")?;
            Some(GeniusHit {
                url: r.get("url")?.as_str()?.to_string(),
                title: r.get("title").and_then(|t| t.as_str()).unwrap_or("").to_string(),
                artist: r
                    .get("artist_names")
                    .or_else(|| r.pointer("/primary_artist/name"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect())
}

fn html_decode(s: &str) -> String {
    let re = Regex::new(r"&(#x[0-9a-fA-F]+|#\d+|amp|lt|gt|quot|apos|nbsp);").unwrap();
    re.replace_all(s, |c: &regex::Captures| {
        let e = &c[1];
        match e {
            "amp" => "&".to_string(),
            "lt" => "<".to_string(),
            "gt" => ">".to_string(),
            "quot" => "\"".to_string(),
            "apos" => "'".to_string(),
            "nbsp" => " ".to_string(),
            _ if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32).map(String::from).unwrap_or_default(),
            _ => e[1..].parse::<u32>().ok().and_then(char::from_u32).map(String::from).unwrap_or_default(),
        }
    })
    .into_owned()
}

/// (tag start, content start, index after the matching </div>) for the div containing `attr_pos`.
fn div_block(html: &str, attr_pos: usize) -> Option<(usize, usize, usize)> {
    let start = html[..attr_pos].rfind("<div")?;
    let content = attr_pos + html[attr_pos..].find('>')? + 1;
    let (mut depth, mut i) = (1usize, content);
    while depth > 0 {
        let close = html[i..].find("</div>").map(|x| x + i)?;
        match html[i..].find("<div").map(|x| x + i) {
            Some(o) if o < close => {
                depth += 1;
                i = o + 4;
            }
            _ => {
                depth -= 1;
                i = close + 6;
            }
        }
    }
    Some((start, content, i))
}

fn genius_extract(html: &str) -> String {
    let br = Regex::new(r"(?i)<br\s*/?>").unwrap();
    let tag = Regex::new(r"<[^>]+>").unwrap();
    let mut out = String::new();
    let mut pos = 0;
    while let Some(rel) = html[pos..].find("data-lyrics-container=\"true\"") {
        let at = pos + rel;
        let Some((_, cs, end)) = div_block(html, at) else { break };
        let mut inner = html[cs..end - 6].to_string();
        while let Some(x) = inner.find("data-exclude-from-selection=\"true\"") {
            match div_block(&inner, x) {
                Some((s0, _, e0)) => inner.replace_range(s0..e0, ""),
                None => break,
            }
        }
        let t = br.replace_all(&inner, "\n");
        let t = tag.replace_all(&t, "");
        out.push_str(&html_decode(&t));
        out.push('\n');
        pos = end;
    }
    out.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !(l.starts_with('[') && l.ends_with(']')))
        .collect::<Vec<_>>()
        .join("\n")
}

pub async fn genius_page(url: &str) -> Result<String, String> {
    let html = client()
        .get(url)
        .header("Accept-Language", "en-US,en;q=0.9")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let t = genius_extract(&html);
    if t.is_empty() {
        Err("no lyrics on that page".into())
    } else {
        Ok(t)
    }
}

fn norm(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

async fn genius_lyrics(artist: &str, title: &str) -> Result<Option<Lyrics>, String> {
    let hits = genius_search(&format!("{title} {artist}")).await?;
    let (nt, na) = (norm(title), norm(artist));
    let Some(hit) = hits.into_iter().find(|h| {
        let (ht, ha) = (norm(&h.title), norm(&h.artist));
        (ht.contains(&nt) || nt.contains(&ht)) && !ht.is_empty() && (na.is_empty() || ha.contains(&na) || na.contains(&ha))
    }) else {
        return Ok(None);
    };
    let text = genius_page(&hit.url).await?;
    Ok(Some(Lyrics {
        id: 0,
        track: hit.title,
        artist: hit.artist,
        album: String::new(),
        duration: 0.0,
        synced: None,
        plain: Some(text),
        source: "genius".into(),
        url: Some(hit.url),
    }))
}

// ───────────────────────── translation ─────────────────────────

#[derive(Serialize, Clone, Debug)]
pub struct Translated {
    pub lines: Vec<String>,
    pub detected: String,
}

/// Machine-translate lyric lines (keeps 1:1 alignment, so timings are preserved).
pub async fn translate(lines: Vec<String>, target: &str) -> Result<Translated, String> {
    let mut out = vec![String::new(); lines.len()];
    let idx: Vec<usize> = (0..lines.len()).filter(|&i| !lines[i].trim().is_empty()).collect();
    let mut chunks: Vec<Vec<usize>> = Vec::new();
    let (mut cur, mut size) = (Vec::new(), 0usize);
    for &i in &idx {
        if size + lines[i].len() > 1600 && !cur.is_empty() {
            chunks.push(std::mem::take(&mut cur));
            size = 0;
        }
        size += lines[i].len() + 1;
        cur.push(i);
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    let mut detected = String::new();
    for c in chunks {
        let text = c.iter().map(|&i| lines[i].trim()).collect::<Vec<_>>().join("\n");
        let v: Value = client()
            .post(format!(
                "https://translate.googleapis.com/translate_a/single?client=gtx&sl=auto&tl={}&dt=t",
                enc(target)
            ))
            .form(&[("q", text.as_str())])
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        let mut joined = String::new();
        if let Some(segs) = v.get(0).and_then(|a| a.as_array()) {
            for seg in segs {
                if let Some(t) = seg.get(0).and_then(|t| t.as_str()) {
                    joined.push_str(t);
                }
            }
        }
        if detected.is_empty() {
            detected = v.get(2).and_then(|d| d.as_str()).unwrap_or("").to_string();
        }
        let parts: Vec<&str> = joined.split('\n').collect();
        for (k, &i) in c.iter().enumerate() {
            out[i] = parts.get(k).map(|p| p.trim().to_string()).unwrap_or_default();
        }
    }
    Ok(Translated { lines: out, detected })
}

pub async fn fetch_image(url: &str) -> Result<String, String> {
    let r = client().get(url).send().await.map_err(|e| e.to_string())?;
    let mime = r
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .to_string();
    let bytes = r.bytes().await.map_err(|e| e.to_string())?;
    Ok(format!(
        "data:{};base64,{}",
        mime,
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    ))
}


// ───────────────────────── JSON helpers ─────────────────────────

fn walk<'a>(v: &'a Value, key: &str, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if k == key {
                    out.push(x);
                } else {
                    walk(x, key, out);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| walk(x, key, out)),
        _ => {}
    }
}

fn first_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    match v {
        Value::Object(m) => {
            if let Some(Value::String(s)) = m.get(key) {
                return Some(s);
            }
            m.values().find_map(|x| first_str(x, key))
        }
        Value::Array(a) => a.iter().find_map(|x| first_str(x, key)),
        _ => None,
    }
}

fn runs_text(v: &Value) -> String {
    if let Some(s) = v.get("simpleText").and_then(|s| s.as_str()) {
        return s.to_string();
    }
    v.get("runs")
        .and_then(|r| r.as_array())
        .map(|a| a.iter().filter_map(|x| x.get("text").and_then(|t| t.as_str())).collect::<String>())
        .unwrap_or_default()
}

fn parse_dur(s: &str) -> i64 {
    s.split(':').fold(0i64, |acc, p| acc * 60 + p.trim().parse::<i64>().unwrap_or(0)) * 1000
}

fn last_thumb(v: &Value) -> String {
    let mut ts = Vec::new();
    walk(v, "thumbnails", &mut ts);
    ts.first()
        .and_then(|t| t.as_array())
        .and_then(|a| a.last())
        .and_then(|x| x.get("url"))
        .and_then(|u| u.as_str())
        .unwrap_or("")
        .to_string()
}

fn big_thumb(u: &str) -> String {
    let re = Regex::new(r"=w\d+-h\d+").unwrap();
    let u = re.replace(u, "=w544-h544").to_string();
    if u.starts_with("//") {
        format!("https:{u}")
    } else {
        u
    }
}

// ───────────────────────── YouTube Music ─────────────────────────

/// Songs search on music.youtube.com (InnerTube WEB_REMIX client).
pub async fn ytm_search(q: &str) -> Result<Vec<Track>, String> {
    let body = json!({
        "context": {"client": {"clientName": "WEB_REMIX", "clientVersion": "1.20241111.01.00", "hl": "en", "gl": "US"}},
        "query": q,
        "params": "EgWKAQIIAWoMEA4QChADEAQQCRAF"
    });
    let v: Value = client()
        .post("https://music.youtube.com/youtubei/v1/search?prettyPrint=false")
        .header("Origin", "https://music.youtube.com")
        .header("Referer", "https://music.youtube.com/")
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let mut items = Vec::new();
    walk(&v, "musicResponsiveListItemRenderer", &mut items);
    let mut out: Vec<Track> = Vec::new();
    for it in items {
        let Some(vid) = first_str(it, "videoId") else { continue };
        if out.iter().any(|t| t.id == vid) {
            continue;
        }
        let cols = it.get("flexColumns").and_then(|c| c.as_array());
        let col = |i: usize| {
            cols.and_then(|c| c.get(i))
                .and_then(|c| c.get("musicResponsiveListItemFlexColumnRenderer"))
                .and_then(|c| c.get("text"))
                .map(runs_text)
                .unwrap_or_default()
        };
        let title = col(0);
        let mut parts: Vec<String> = col(1)
            .split(" • ")
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        if parts.first().map(|p| p == "Song" || p == "Video").unwrap_or(false) {
            parts.remove(0);
        }
        let dur = parts
            .last()
            .filter(|p| p.contains(':') && p.chars().all(|c| c.is_ascii_digit() || c == ':'))
            .map(|p| parse_dur(p))
            .unwrap_or(0);
        let artist = parts.first().cloned().unwrap_or_default();
        let album = if parts.len() >= 3 { parts[1].clone() } else { String::new() };
        if title.is_empty() {
            continue;
        }
        out.push(Track {
            id: vid.to_string(),
            title,
            artist,
            album,
            art: big_thumb(&last_thumb(it)),
            preview: String::new(),
            url: format!("https://music.youtube.com/watch?v={vid}"),
            duration_ms: dur,
            source: "ytmusic".into(),
            play: vid.to_string(),
        });
    }
    if out.is_empty() {
        return yt_search(q).await;
    }
    Ok(out)
}

/// One song row (musicResponsiveListItemRenderer) → Track. `hint_*` fill what album / playlist rows leave out.
fn ytm_track(it: &Value, hint_artist: &str, hint_art: &str) -> Option<Track> {
    let vid = first_str(it, "videoId")?;
    let cols = it.get("flexColumns").and_then(|c| c.as_array());
    let col = |i: usize| {
        cols.and_then(|c| c.get(i))
            .and_then(|c| c.get("musicResponsiveListItemFlexColumnRenderer"))
            .and_then(|c| c.get("text"))
            .map(runs_text)
            .unwrap_or_default()
    };
    let title = col(0);
    if title.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = col(1).split(" • ").map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
    if parts.first().map(|p| p == "Song" || p == "Video").unwrap_or(false) {
        parts.remove(0);
    }
    let dur = parts
        .last()
        .filter(|p| p.contains(':') && p.chars().all(|c| c.is_ascii_digit() || c == ':'))
        .map(|p| parse_dur(p))
        .unwrap_or(0);
    let mut artist = parts.first().cloned().unwrap_or_default();
    if artist.is_empty() || artist.to_lowercase().contains("plays") || artist.to_lowercase().contains("views") {
        artist = hint_artist.to_string();
    }
    let album = if parts.len() >= 3 { parts[1].clone() } else { String::new() };
    let art = big_thumb(&last_thumb(it));
    Some(Track {
        id: vid.to_string(),
        title,
        artist,
        album,
        art: if art.is_empty() { hint_art.to_string() } else { art },
        preview: String::new(),
        url: format!("https://music.youtube.com/watch?v={vid}"),
        duration_ms: dur,
        source: "ytmusic".into(),
        play: vid.to_string(),
    })
}

#[derive(Serialize, Clone, Debug)]
pub struct Entity {
    /// artist | album | playlist
    pub kind: String,
    /// YouTube Music browseId
    pub id: String,
    pub title: String,
    pub sub: String,
    pub art: String,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Rich {
    pub songs: Vec<Track>,
    pub artists: Vec<Entity>,
    pub suggestions: Vec<String>,
}

fn ytm_ctx() -> Value {
    json!({"client": {"clientName": "WEB_REMIX", "clientVersion": "1.20241111.01.00", "hl": "en", "gl": "US"}})
}

/// The user's YouTube session (cookie header, SAPISID). When set, YouTube Music requests are personalised.
static YT_AUTH: std::sync::Mutex<Option<(String, String)>> = std::sync::Mutex::new(None);

pub fn set_yt_auth(a: Option<(String, String)>) {
    *YT_AUTH.lock().unwrap_or_else(|e| e.into_inner()) = a;
}

fn yt_signed_in() -> bool {
    YT_AUTH.lock().map(|g| g.is_some()).unwrap_or(false)
}

async fn ytm_post(path: &str, body: Value) -> Result<Value, String> {
    use sha1::{Digest, Sha1};
    let sep = if path.contains('?') { '&' } else { '?' };
    let mut req = client()
        .post(format!("https://music.youtube.com/youtubei/v1/{path}{sep}prettyPrint=false"))
        .header("Origin", "https://music.youtube.com")
        .header("Referer", "https://music.youtube.com/");
    if let Some((cookie, sapisid)) = YT_AUTH.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let hash = Sha1::digest(format!("{ts} {sapisid} https://music.youtube.com").as_bytes());
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        req = req
            .header("Cookie", cookie)
            .header("Authorization", format!("SAPISIDHASH {ts}_{hex}"))
            .header("X-Goog-AuthUser", "0")
            .header("X-Origin", "https://music.youtube.com");
    }
    req.json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())
}

/// Entity (artist / album / playlist) from a list item or the "top result" card.
fn ytm_entity(it: &Value, title: String, sub: String) -> Option<Entity> {
    let be = it.pointer("/navigationEndpoint/browseEndpoint").or_else(|| {
        let mut v = Vec::new();
        walk(it, "browseEndpoint", &mut v);
        v.into_iter().next()
    })?;
    let id = be.get("browseId")?.as_str()?.to_string();
    let page = be
        .pointer("/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType")
        .and_then(|p| p.as_str())
        .unwrap_or("");
    let kind = if page.contains("ARTIST") {
        "artist"
    } else if page.contains("ALBUM") {
        "album"
    } else if page.contains("PLAYLIST") {
        "playlist"
    } else {
        return None;
    };
    Some(Entity { kind: kind.into(), id, title, sub, art: big_thumb(&last_thumb(it)) })
}

/// Smart search on YouTube Music: songs, artists, albums, playlists and typing suggestions together.
pub async fn ytm_rich(q: &str) -> Result<Rich, String> {
    let v = ytm_post("search", json!({"context": ytm_ctx(), "query": q})).await?;
    let mut out = Rich::default();
    // the big "top result" card (usually the artist or the exact song)
    let mut cards = Vec::new();
    walk(&v, "musicCardShelfRenderer", &mut cards);
    for c in cards {
        let title = c.get("title").map(runs_text).unwrap_or_default();
        let sub = c.get("subtitle").map(runs_text).unwrap_or_default();
        if let Some(e) = ytm_entity(c.get("title").unwrap_or(c), title, sub) {
            let e = Entity { art: big_thumb(&last_thumb(c)), ..e };
            if e.kind == "artist" {
                out.artists.push(e);
            }
        }
    }
    let mut items = Vec::new();
    walk(&v, "musicResponsiveListItemRenderer", &mut items);
    for it in items {
        if first_str(it, "videoId").is_some() {
            if let Some(t) = ytm_track(it, "", "") {
                if !out.songs.iter().any(|s| s.id == t.id) {
                    out.songs.push(t);
                }
            }
            continue;
        }
        let cols = it.get("flexColumns").and_then(|c| c.as_array());
        let col = |i: usize| {
            cols.and_then(|c| c.get(i))
                .and_then(|c| c.get("musicResponsiveListItemFlexColumnRenderer"))
                .and_then(|c| c.get("text"))
                .map(runs_text)
                .unwrap_or_default()
        };
        let (title, sub) = (col(0), col(1));
        if title.is_empty() {
            continue;
        }
        if let Some(e) = ytm_entity(it, title, sub) {
            if e.kind == "artist" && !out.artists.iter().any(|x| x.id == e.id) {
                out.artists.push(e);
            }
        }
    }
    if out.songs.is_empty() {
        if let Ok(t) = yt_search(q).await {
            out.songs = t;
        }
    }
    // suggestions while typing (search phrases, artist names…)
    if let Ok(sv) = ytm_post("music/get_search_suggestions", json!({"context": ytm_ctx(), "input": q})).await {
        let mut runs = Vec::new();
        walk(&sv, "searchSuggestionRenderer", &mut runs);
        for r in runs {
            let t = r.get("suggestion").map(runs_text).unwrap_or_default();
            let t = t.trim().to_string();
            if !t.is_empty() && !t.eq_ignore_ascii_case(q) && !out.suggestions.contains(&t) {
                out.suggestions.push(t);
            }
        }
        out.suggestions.truncate(8);
    }
    Ok(out)
}

/// The songs of a singer (artist page + the singer's other songs). Playlists / albums go through `ytm_playlist`.
pub async fn ytm_artist(id: &str, title: &str, art: &str) -> Result<Vec<Track>, String> {
    let v = ytm_post("browse", json!({"context": ytm_ctx(), "browseId": id})).await?;
    let hint_artist = title;
    let mut items = Vec::new();
    walk(&v, "musicResponsiveListItemRenderer", &mut items);
    let mut out: Vec<Track> = Vec::new();
    for it in items {
        if let Some(t) = ytm_track(it, hint_artist, art) {
            if !out.iter().any(|x| x.id == t.id) {
                out.push(t);
            }
        }
    }
    if out.len() < 12 {
        // the artist page shows only a handful of songs: add the artist's other songs
        if let Ok(more) = ytm_search(title).await {
            let key = norm(title);
            for t in more {
                if norm(&t.artist).contains(&key) && !out.iter().any(|x| x.id == t.id) {
                    out.push(t);
                }
            }
        }
    }
    if out.is_empty() {
        Err("nothing playable here".into())
    } else {
        Ok(out)
    }
}

/// Regular YouTube video search (InnerTube WEB client) – fallback and MV lookup.
pub async fn yt_search(q: &str) -> Result<Vec<Track>, String> {
    let body = json!({
        "context": {"client": {"clientName": "WEB", "clientVersion": "2.20241111.01.00", "hl": "en", "gl": "US"}},
        "query": q,
        "params": "EgIQAQ=="
    });
    let v: Value = client()
        .post("https://www.youtube.com/youtubei/v1/search?prettyPrint=false")
        .header("Origin", "https://www.youtube.com")
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let mut items = Vec::new();
    walk(&v, "videoRenderer", &mut items);
    Ok(items
        .into_iter()
        .filter_map(|it| {
            let vid = it.get("videoId")?.as_str()?.to_string();
            let title = it.get("title").map(runs_text).unwrap_or_default();
            let artist = it.get("ownerText").map(runs_text).unwrap_or_default();
            let dur = it.get("lengthText").map(runs_text).map(|d| parse_dur(&d)).unwrap_or(0);
            Some(Track {
                id: vid.clone(),
                title,
                artist: artist.trim_end_matches(" - Topic").to_string(),
                album: String::new(),
                art: format!("https://i.ytimg.com/vi/{vid}/hqdefault.jpg"),
                preview: String::new(),
                url: format!("https://music.youtube.com/watch?v={vid}"),
                duration_ms: dur,
                source: "ytmusic".into(),
                play: vid,
            })
        })
        .take(24)
        .collect())
}

// ───────────────────────── playlists, albums, radio ─────────────────────────

/// A playlist or album found by search.
#[derive(Serialize, Clone, Debug)]
pub struct Collection {
    /// YouTube Music playlistId, SoundCloud playlist id or Spotify URI
    pub id: String,
    /// album | playlist
    pub kind: String,
    pub title: String,
    pub sub: String,
    pub art: String,
    pub source: String,
}

/// Albums and playlists on YouTube Music matching a query.
pub async fn ytm_collections(q: &str) -> Result<Vec<Collection>, String> {
    // search filters: albums, community playlists, featured playlists
    let filters = [
        ("album", "EgWKAQIYAWoMEA4QChADEAQQCRAF"),
        ("playlist", "EgeKAQQoADgBagwQDhAKEAMQBBAJEAU="),
        ("playlist", "EgeKAQQoAEABagwQDhAKEAMQBBAJEAU="),
    ];
    let mut out: Vec<Collection> = Vec::new();
    for (kind, params) in filters {
        let Ok(v) = ytm_post("search", json!({"context": ytm_ctx(), "query": q, "params": params})).await else { continue };
        let mut items = Vec::new();
        walk(&v, "musicResponsiveListItemRenderer", &mut items);
        for it in items.into_iter().take(if kind == "album" { 8 } else { 6 }) {
            // the play button carries the list to play (albums: OLAK5uy_…, playlists: PL…/RDCLAK…)
            let Some(pid) = first_str(it, "playlistId") else { continue };
            if out.iter().any(|c| c.id == pid) {
                continue;
            }
            let cols = it.get("flexColumns").and_then(|c| c.as_array());
            let col = |i: usize| {
                cols.and_then(|c| c.get(i))
                    .and_then(|c| c.get("musicResponsiveListItemFlexColumnRenderer"))
                    .and_then(|c| c.get("text"))
                    .map(runs_text)
                    .unwrap_or_default()
            };
            let title = col(0);
            if title.is_empty() {
                continue;
            }
            out.push(Collection {
                id: pid.to_string(),
                kind: kind.into(),
                title,
                sub: col(1),
                art: big_thumb(&last_thumb(it)),
                source: "ytmusic".into(),
            });
        }
    }
    Ok(out)
}

/// Songs of a YouTube Music "up next" panel (a playlist, an album or a radio).
fn ytm_panel(v: &Value) -> Vec<Track> {
    let mut items = Vec::new();
    walk(v, "playlistPanelVideoRenderer", &mut items);
    let mut out: Vec<Track> = Vec::new();
    for it in items {
        let Some(vid) = it.get("videoId").and_then(|x| x.as_str()) else { continue };
        let title = it.get("title").map(runs_text).unwrap_or_default();
        let by = it.get("longBylineText").map(runs_text).unwrap_or_default();
        let parts: Vec<String> = by.split(" • ").map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
        let artist = parts.first().cloned().unwrap_or_default();
        // the same song can appear twice (audio + music-video version)
        if title.is_empty() || out.iter().any(|t| t.id == vid || (t.title == title && t.artist == artist)) {
            continue;
        }
        // songs read "Artist • Album • Year"; music videos read "Artist • 6.8B views • 36M likes"
        let album = if parts.len() >= 3 && !parts[1].ends_with(" views") && !parts[1].ends_with(" likes") { parts[1].clone() } else { String::new() };
        let dur = it.get("lengthText").map(runs_text).map(|d| parse_dur(&d)).unwrap_or(0);
        out.push(Track {
            id: vid.to_string(),
            title,
            artist,
            album,
            art: big_thumb(&last_thumb(it)),
            preview: String::new(),
            url: format!("https://music.youtube.com/watch?v={vid}"),
            duration_ms: dur,
            source: "ytmusic".into(),
            play: vid.to_string(),
        });
    }
    out
}

/// All songs of a YouTube Music playlist or album (by playlistId).
pub async fn ytm_playlist(playlist_id: &str) -> Result<Vec<Track>, String> {
    let v = ytm_post(
        "next",
        json!({"context": ytm_ctx(), "playlistId": playlist_id, "enablePersistentPlaylistPanel": true, "isAudioOnly": true}),
    )
    .await?;
    let out = ytm_panel(&v);
    if out.is_empty() {
        return Err("That playlist is empty or private".into());
    }
    Ok(out)
}

/// YouTube Music radio for a song: what YouTube Music would play after it.
/// Songs from the YouTube Music home page ("Quick picks", "Listen again"…): personal when signed in.
pub async fn ytm_home_picks() -> Result<Vec<Track>, String> {
    let v = ytm_post("browse", json!({"context": ytm_ctx(), "browseId": "FEmusic_home"})).await?;
    let mut out: Vec<Track> = Vec::new();
    let mut rows = Vec::new();
    walk(&v, "musicResponsiveListItemRenderer", &mut rows);
    for it in rows {
        if let Some(t) = ytm_track(it, "", "") {
            out.push(t);
        }
    }
    let mut cards = Vec::new();
    walk(&v, "musicTwoRowItemRenderer", &mut cards);
    for it in cards {
        let Some(vid) = first_str(it, "videoId") else { continue };
        let title = it.get("title").map(runs_text).unwrap_or_default();
        let sub = it.get("subtitle").map(runs_text).unwrap_or_default();
        let parts: Vec<&str> = sub.split(" • ").map(|p| p.trim()).filter(|p| !p.is_empty() && *p != "Song" && *p != "Video").collect();
        let artist = parts.first().copied().unwrap_or("").to_string();
        if title.is_empty() {
            continue;
        }
        out.push(Track {
            id: vid.to_string(),
            title,
            artist,
            album: String::new(),
            art: big_thumb(&last_thumb(it)),
            preview: String::new(),
            url: format!("https://music.youtube.com/watch?v={vid}"),
            duration_ms: 0,
            source: "ytmusic".into(),
            play: vid.to_string(),
        });
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|t| seen.insert(t.id.clone()));
    Ok(out)
}

/// Combined list for one chosen song: its radio ("up next") mixed with picks from the user's home page
/// (3 radio songs : 1 home pick). Personalised when the user is signed in to YouTube.
pub async fn ytm_suggest(video_id: &str) -> Result<(Vec<Track>, bool), String> {
    let radio = ytm_radio(video_id).await;
    let home = ytm_home_picks().await;
    let radio = radio.unwrap_or_default();
    let home: Vec<Track> = home.unwrap_or_default().into_iter().filter(|t| t.id != video_id).collect();
    if radio.is_empty() && home.is_empty() {
        return Err("no suggestions".into());
    }
    let mut out: Vec<Track> = Vec::new();
    let (mut r, mut h) = (radio.into_iter(), home.into_iter());
    loop {
        let mut got = false;
        for _ in 0..3 {
            if let Some(t) = r.next() {
                got = true;
                if !out.iter().any(|x| x.id == t.id) {
                    out.push(t);
                }
            }
        }
        if let Some(t) = h.next() {
            got = true;
            if !out.iter().any(|x| x.id == t.id) {
                out.push(t);
            }
        }
        if !got || out.len() >= 40 {
            break;
        }
    }
    Ok((out, yt_signed_in()))
}

/// YouTube's own Mix for a song ("Danh sách kết hợp"): the list YouTube Music builds when you play it,
/// personal when signed in. `cont` asks for the next part of the same Mix (it goes on and on);
/// returns the songs and the token for the part after.
pub async fn ytm_mix(video_id: &str, cont: Option<&str>) -> Result<(Vec<Track>, Option<String>), String> {
    let body = json!({
        "context": ytm_ctx(), "videoId": video_id, "playlistId": format!("RDAMVM{video_id}"),
        "enablePersistentPlaylistPanel": true, "isAudioOnly": true, "tunerSettingValue": "AUTOMIX_SETTING_NORMAL",
        "watchEndpointMusicSupportedConfigs": {"watchEndpointMusicConfig": {"hasPersistentPlaylistPanel": true, "musicVideoType": "MUSIC_VIDEO_TYPE_ATV"}}
    });
    let path = match cont {
        Some(c) if !c.is_empty() => format!("next?ctoken={0}&continuation={0}&type=next", enc(c)),
        _ => "next".to_string(),
    };
    let v = ytm_post(&path, body).await?;
    let tracks: Vec<Track> = ytm_panel(&v).into_iter().filter(|t| t.id != video_id).collect();
    let next = find_key(&v, "nextRadioContinuationData")
        .and_then(|d| d.get("continuation"))
        .and_then(|c| c.as_str())
        .map(String::from);
    if tracks.is_empty() && cont.is_none() {
        return Err("YouTube has no Mix for this song".into());
    }
    Ok((tracks, next))
}

// ───────────────────────── YouTube · Music (home page, signed in) ─────────────────────────

/// A playlist from the YouTube home page: mostly Mixes ("Danh sách kết hợp"), played from `video` in `list`.
#[derive(Serialize, Clone, Debug)]
pub struct FeedList {
    pub list: String,
    pub video: String,
    pub title: String,
    pub sub: String,
    pub art: String,
}

/// A row of playlists; the first row has no title (the Music feed itself), the others keep YouTube's title.
#[derive(Serialize, Clone, Debug)]
pub struct FeedShelf {
    pub title: String,
    pub items: Vec<FeedList>,
}

const YT_WEB_VERSION: &str = "2.20261119.07.00";

fn yt_ctx(hl: &str) -> Value {
    json!({"client": {"clientName": "WEB", "clientVersion": YT_WEB_VERSION, "hl": hl}})
}

/// www.youtube.com InnerTube request, signed in with the user's YouTube session when there is one.
async fn yt_post(path: &str, body: Value) -> Result<Value, String> {
    use sha1::{Digest, Sha1};
    let mut req = client()
        .post(format!("https://www.youtube.com/youtubei/v1/{path}?prettyPrint=false"))
        .header("Origin", "https://www.youtube.com")
        .header("Referer", "https://www.youtube.com/");
    if let Some((cookie, sapisid)) = YT_AUTH.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let hash = Sha1::digest(format!("{ts} {sapisid} https://www.youtube.com").as_bytes());
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        req = req
            .header("Cookie", cookie)
            .header("Authorization", format!("SAPISIDHASH {ts}_{hex}"))
            .header("X-Goog-AuthUser", "0")
            .header("X-Origin", "https://www.youtube.com");
    }
    let resp = req.json(&body).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("YouTube answered {}", resp.status().as_u16()));
    }
    resp.json().await.map_err(|e| e.to_string())
}

/// One playlist card (lockupViewModel) of the home page; videos and other cards give None.
fn feed_list(l: &Value) -> Option<FeedList> {
    if l.get("contentType")?.as_str()? != "LOCKUP_CONTENT_TYPE_PLAYLIST" {
        return None;
    }
    let we = find_key(l.pointer("/rendererContext/commandContext/onTap")?, "watchEndpoint")?;
    let list = we.get("playlistId")?.as_str()?.to_string();
    let video = we.get("videoId")?.as_str()?.to_string();
    let meta = l.pointer("/metadata/lockupMetadataViewModel")?;
    let title = meta.pointer("/title/content")?.as_str()?.trim().to_string();
    let sub = meta
        .pointer("/metadata/contentMetadataViewModel/metadataRows/0/metadataParts/0/text/content")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    // the biggest picture of the card (the seed video of the Mix)
    let img = l.pointer("/contentImage/collectionThumbnailViewModel/primaryThumbnail").or_else(|| l.get("contentImage"))?;
    let mut srcs = Vec::new();
    walk(img, "sources", &mut srcs);
    let art = srcs
        .iter()
        .filter_map(|s| s.as_array())
        .flatten()
        .max_by_key(|s| s.get("width").and_then(|w| w.as_i64()).unwrap_or(0))
        .and_then(|s| s.get("url"))
        .and_then(|u| u.as_str())
        .unwrap_or("")
        .to_string();
    if title.is_empty() {
        return None;
    }
    Some(FeedList { list, video, title, sub, art })
}

/// The playlists YouTube picks for the signed-in user under Home → Music ("Âm nhạc"):
/// the Mixes of the feed first, then the playlist rows ("Artist mixes for you"…). Videos are left out.
pub async fn yt_music_feed(hl: &str) -> Result<Vec<FeedShelf>, String> {
    if !yt_signed_in() {
        return Err("not_signed_in".into());
    }
    let hl = if hl == "vi" { "vi" } else { "en" };
    let home = yt_post("browse", json!({"context": yt_ctx(hl), "browseId": "FEwhat_to_watch"})).await?;
    let mut chips = Vec::new();
    walk(&home, "chipCloudChipRenderer", &mut chips);
    let token = chips
        .iter()
        .find(|c| {
            let t = c.get("text").map(runs_text).unwrap_or_default().trim().to_lowercase();
            t == "music" || t == "âm nhạc"
        })
        .and_then(|c| find_key(c, "continuationCommand"))
        .and_then(|c| c.get("token"))
        .and_then(|t| t.as_str())
        .ok_or("no_music_feed")?
        .to_string();

    let mut main = FeedShelf { title: String::new(), items: Vec::new() };
    let mut shelves: Vec<FeedShelf> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut next = Some(token);
    for _page in 0..2 {
        let Some(tok) = next.take() else { break };
        let v = yt_post("browse", json!({"context": yt_ctx(hl), "continuation": tok})).await?;
        let mut lists = Vec::new();
        walk(&v, "continuationItems", &mut lists);
        let Some(items) = lists.first().and_then(|x| x.as_array()) else { break };
        for it in items {
            if let Some(l) = it.pointer("/richItemRenderer/content/lockupViewModel") {
                if let Some(f) = feed_list(l) {
                    if seen.insert(f.list.clone()) {
                        main.items.push(f);
                    }
                }
            } else if let Some(s) = it.pointer("/richSectionRenderer/content/richShelfRenderer") {
                let title = s.get("title").map(runs_text).unwrap_or_default().trim().to_string();
                let mut ls = Vec::new();
                walk(s, "lockupViewModel", &mut ls);
                let mut row = Vec::new();
                for f in ls.into_iter().filter_map(feed_list) {
                    if seen.insert(f.list.clone()) {
                        row.push(f);
                    }
                }
                if !row.is_empty() {
                    shelves.push(FeedShelf { title, items: row });
                }
            } else if it.get("continuationItemRenderer").is_some() {
                next = find_key(it, "continuationCommand").and_then(|c| c.get("token")).and_then(|t| t.as_str()).map(String::from);
            }
        }
        // a short feed: one more page, as YouTube loads it when you scroll
        if main.items.len() >= 12 {
            break;
        }
    }
    if main.items.is_empty() && shelves.is_empty() {
        return Err("no_music_feed".into());
    }
    let mut out = Vec::new();
    if !main.items.is_empty() {
        out.push(main);
    }
    out.extend(shelves);
    Ok(out)
}

/// The songs of a YouTube playlist or Mix, starting at `video_id` (a Mix goes on: ask again from its last song).
pub async fn yt_list(video_id: &str, playlist_id: &str) -> Result<Vec<Track>, String> {
    let ok = |s: &str| !s.is_empty() && s.len() <= 80 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok(video_id) || !ok(playlist_id) {
        return Err("That playlist couldn't be opened".into());
    }
    let v = yt_post("next", json!({"context": yt_ctx("en"), "videoId": video_id, "playlistId": playlist_id})).await?;
    let panel = v.pointer("/contents/twoColumnWatchNextResults/playlist").ok_or("That playlist couldn't be opened")?;
    let out = ytm_panel(panel);
    if out.is_empty() {
        return Err("That playlist is empty or private".into());
    }
    Ok(out)
}


pub async fn ytm_radio(video_id: &str) -> Result<Vec<Track>, String> {
    let v = ytm_post(
        "next",
        json!({"context": ytm_ctx(), "videoId": video_id, "playlistId": format!("RDAMVM{video_id}"),
               "params": "wAEB", "enablePersistentPlaylistPanel": true, "isAudioOnly": true}),
    )
    .await?;
    Ok(ytm_panel(&v).into_iter().filter(|t| t.id != video_id).collect())
}

// ───────────────────────── SoundCloud ─────────────────────────

static SC_ID: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

async fn sc_client_id(refresh: bool) -> Result<String, String> {
    if !refresh {
        let cached = SC_ID.lock().unwrap().clone();
        if let Some(id) = cached {
            return Ok(id);
        }
    }
    let html = client()
        .get("https://soundcloud.com/")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let sre = Regex::new(r#"src="(https://a-v2\.sndcdn\.com/assets/[^"]+\.js)""#).unwrap();
    let idre = Regex::new(r#"client_id\s*[:=]\s*"([A-Za-z0-9]{32})""#).unwrap();
    let urls: Vec<String> = sre.captures_iter(&html).map(|c| c[1].to_string()).collect();
    for u in urls.iter().rev() {
        if let Ok(r) = client().get(u).send().await {
            if let Ok(js) = r.text().await {
                if let Some(c) = idre.captures(&js) {
                    let id = c[1].to_string();
                    *SC_ID.lock().unwrap() = Some(id.clone());
                    return Ok(id);
                }
            }
        }
    }
    Err("SoundCloud search is unavailable right now".into())
}

/// GET an api-v2.soundcloud.com path (with its own query string, or none);
/// adds the client_id and retries once with a fresh one.
async fn sc_get(path: &str) -> Result<Value, String> {
    let mut id = sc_client_id(false).await?;
    let sep = if path.contains('?') { '&' } else { '?' };
    let mut tries = 0;
    loop {
        let r = client()
            .get(format!("https://api-v2.soundcloud.com{path}{sep}client_id={id}"))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if (r.status().as_u16() == 401 || r.status().as_u16() == 403) && tries == 0 {
            tries += 1;
            id = sc_client_id(true).await?;
            continue;
        }
        return r.json().await.map_err(|e| e.to_string());
    }
}

fn sc_track(x: &Value) -> Option<Track> {
    let s = |p: &str| x.pointer(p).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let link = s("/permalink_url");
    if link.is_empty() {
        return None;
    }
    let art = {
        let a = s("/artwork_url");
        let a = if a.is_empty() { s("/user/avatar_url") } else { a };
        a.replace("-large.", "-t500x500.")
    };
    let artist = {
        let a = s("/publisher_metadata/artist");
        if a.is_empty() { s("/user/username") } else { a }
    };
    Some(Track {
        id: x.get("id").map(|i| i.to_string()).unwrap_or_default(),
        title: s("/title"),
        artist,
        album: s("/publisher_metadata/album_title"),
        art,
        preview: String::new(),
        url: link.clone(),
        duration_ms: x.get("full_duration").or_else(|| x.get("duration")).and_then(|d| d.as_i64()).unwrap_or(0),
        source: "soundcloud".into(),
        play: link,
    })
}

pub async fn sc_search(q: &str) -> Result<Vec<Track>, String> {
    let v = sc_get(&format!("/search/tracks?q={}&limit=24", enc(q))).await?;
    let arr = v.get("collection").and_then(|c| c.as_array()).cloned().unwrap_or_default();
    Ok(arr.iter().filter_map(sc_track).collect())
}

/// SoundCloud playlists and albums matching a query.
pub async fn sc_collections(q: &str) -> Result<Vec<Collection>, String> {
    let v = sc_get(&format!("/search/playlists?q={}&limit=12", enc(q))).await?;
    let arr = v.get("collection").and_then(|c| c.as_array()).cloned().unwrap_or_default();
    Ok(arr
        .iter()
        .filter_map(|x| {
            let s = |p: &str| x.pointer(p).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let id = x.get("id")?.to_string();
            let art = {
                let a = s("/artwork_url");
                let a = if a.is_empty() { s("/tracks/0/artwork_url") } else { a };
                let a = if a.is_empty() { s("/user/avatar_url") } else { a };
                a.replace("-large.", "-t500x500.")
            };
            let n = x.get("track_count").and_then(|n| n.as_i64()).unwrap_or(0);
            let album = x.get("is_album").and_then(|b| b.as_bool()).unwrap_or(false) || s("/set_type") == "album";
            Some(Collection {
                id,
                kind: if album { "album" } else { "playlist" }.into(),
                title: s("/title"),
                sub: format!("{} · {} tracks", s("/user/username"), n),
                art,
                source: "soundcloud".into(),
            })
        })
        .filter(|c| !c.title.is_empty())
        .collect())
}

/// All tracks of a SoundCloud playlist / album (the API only fills in the first few).
pub async fn sc_playlist(id: &str) -> Result<Vec<Track>, String> {
    let v = sc_get(&format!("/playlists/{id}")).await?;
    let arr = v.get("tracks").and_then(|c| c.as_array()).cloned().unwrap_or_default();
    let missing: Vec<String> = arr
        .iter()
        .filter(|x| x.get("permalink_url").is_none())
        .filter_map(|x| x.get("id").map(|i| i.to_string()))
        .collect();
    let mut full: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
    for chunk in missing.chunks(50) {
        if let Ok(Value::Array(a)) = sc_get(&format!("/tracks?ids={}", chunk.join(","))).await {
            for x in a {
                if let Some(i) = x.get("id").map(|i| i.to_string()) {
                    full.insert(i, x);
                }
            }
        }
    }
    Ok(arr
        .iter()
        .filter_map(|x| {
            if x.get("permalink_url").is_some() {
                sc_track(x)
            } else {
                x.get("id").and_then(|i| full.get(&i.to_string())).and_then(sc_track)
            }
        })
        .collect())
}

/// Tracks SoundCloud considers related to a track (its "radio").
pub async fn sc_related(track_id: &str) -> Result<Vec<Track>, String> {
    let v = sc_get(&format!("/tracks/{track_id}/related?limit=25")).await?;
    let arr = v.get("collection").and_then(|c| c.as_array()).cloned().unwrap_or_default();
    Ok(arr.iter().filter_map(sc_track).collect())
}

#[derive(Serialize, Clone, Debug)]
pub struct Cue {
    pub t: f64,
    pub text: String,
}

/// The YouTube video's own captions (CC), even though the player keeps them hidden.
/// Manual captions win over auto-generated ones. Returns [] when there are none.
pub async fn yt_captions(video_id: &str) -> Result<Vec<Cue>, String> {
    let clients = [
        json!({"clientName": "ANDROID", "clientVersion": "19.09.37", "androidSdkVersion": 30, "hl": "en", "gl": "US"}),
        json!({"clientName": "WEB", "clientVersion": "2.20241111.01.00", "hl": "en", "gl": "US"}),
    ];
    for c in clients {
        let body = json!({"context": {"client": c}, "videoId": video_id, "contentCheckOk": true, "racyCheckOk": true});
        let ua = if c["clientName"] == "ANDROID" { "com.google.android.youtube/19.09.37 (Linux; U; Android 11) gzip" } else { "" };
        let mut req = client().post("https://www.youtube.com/youtubei/v1/player?prettyPrint=false").json(&body);
        if !ua.is_empty() {
            req = req.header("User-Agent", ua);
        } else {
            req = req.header("Origin", "https://www.youtube.com");
        }
        let v: Value = match req.send().await {
            Ok(r) => match r.json().await {
                Ok(v) => v,
                Err(_) => continue,
            },
            Err(_) => continue,
        };
        let tracks = v
            .pointer("/captions/playerCaptionsTracklistRenderer/captionTracks")
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default();
        if tracks.is_empty() {
            continue;
        }
        let pick = tracks
            .iter()
            .find(|t| t.get("kind").and_then(|k| k.as_str()) != Some("asr"))
            .or_else(|| tracks.first())
            .unwrap();
        let Some(base) = pick.get("baseUrl").and_then(|b| b.as_str()) else { continue };
        let url = format!("{}&fmt=json3", base.replace("&fmt=srv3", ""));
        let r: Value = match client().get(url).send().await {
            Ok(r) => match r.json().await {
                Ok(v) => v,
                Err(_) => continue,
            },
            Err(_) => continue,
        };
        let mut out: Vec<Cue> = Vec::new();
        for e in r.get("events").and_then(|e| e.as_array()).cloned().unwrap_or_default() {
            let Some(segs) = e.get("segs").and_then(|s| s.as_array()) else { continue };
            let t = e.get("tStartMs").and_then(|t| t.as_f64()).unwrap_or(0.0) / 1000.0;
            let text: String = segs.iter().filter_map(|s| s.get("utf8").and_then(|u| u.as_str())).collect::<Vec<_>>().join("");
            let text = text.replace('\n', " ").split_whitespace().collect::<Vec<_>>().join(" ");
            if text.is_empty() || (text.starts_with('[') && text.ends_with(']') && text.len() < 4) {
                continue;
            }
            if out.last().map(|l| l.text == text).unwrap_or(false) {
                continue;
            }
            out.push(Cue { t, text });
        }
        if !out.is_empty() {
            return Ok(out);
        }
    }
    // YouTube now hides the plain timedtext files behind a proof-of-origin token (empty reply),
    // so fall back to the same "Show transcript" data the YouTube website itself uses.
    yt_transcript(video_id).await
}

fn find_key<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(m) => {
            if let Some(x) = m.get(key) {
                return Some(x);
            }
            m.values().find_map(|x| find_key(x, key))
        }
        Value::Array(a) => a.iter().find_map(|x| find_key(x, key)),
        _ => None,
    }
}

async fn yt_transcript(video_id: &str) -> Result<Vec<Cue>, String> {
    let ctx = json!({"client": {"clientName": "WEB", "clientVersion": "2.20250925.01.00", "hl": "en", "gl": "US"}});
    let post = |url: &str, body: Value| {
        client()
            .post(url)
            .header("Origin", "https://www.youtube.com")
            .header("X-Youtube-Client-Name", "1")
            .header("X-Youtube-Client-Version", "2.20250925.01.00")
            .json(&body)
            .send()
    };
    let next: Value = post("https://www.youtube.com/youtubei/v1/next?prettyPrint=false", json!({"context": ctx, "videoId": video_id}))
        .await
        .map_err(|e| format!("next: {e}"))?
        .json()
        .await
        .map_err(|e| format!("next json: {e}"))?;
    let params = find_key(&next, "getTranscriptEndpoint")
        .and_then(|e| e.get("params"))
        .and_then(|p| p.as_str())
        .ok_or_else(|| "this video has no transcript".to_string())?
        .to_string();
    let tr: Value = post("https://www.youtube.com/youtubei/v1/get_transcript?prettyPrint=false", json!({"context": ctx, "params": params}))
        .await
        .map_err(|e| format!("transcript: {e}"))?
        .json()
        .await
        .map_err(|e| format!("transcript json: {e}"))?;
    let segs = find_key(&tr, "initialSegments").and_then(|s| s.as_array()).cloned().unwrap_or_default();
    let mut out: Vec<Cue> = Vec::new();
    for sg in segs {
        let Some(r) = sg.get("transcriptSegmentRenderer") else { continue };
        let t = r
            .get("startMs")
            .and_then(|m| m.as_str().and_then(|x| x.parse::<f64>().ok()).or_else(|| m.as_f64()))
            .unwrap_or(0.0)
            / 1000.0;
        let text: String = r
            .pointer("/snippet/runs")
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter_map(|x| x.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join(""))
            .unwrap_or_default();
        let text = text.replace('\n', " ").split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() || out.last().map(|l| l.text == text).unwrap_or(false) {
            continue;
        }
        out.push(Cue { t, text });
    }
    if out.is_empty() {
        Err("transcript is empty".into())
    } else {
        Ok(out)
    }
}

// ───────────────────────── YouTube Music library (signed in) ─────────────────────────

fn ytm_error(v: &Value) -> Option<String> {
    v.get("error").map(|e| e.get("message").and_then(|m| m.as_str()).unwrap_or("YouTube refused the request").to_string())
}

/// Save a playlist or album (by playlistId) to the signed-in user's YouTube Music library.
pub async fn ytm_save_playlist(playlist_id: &str) -> Result<(), String> {
    if !yt_signed_in() {
        return Err("not_signed_in".into());
    }
    let v = ytm_post("like/like", json!({"context": ytm_ctx(), "target": {"playlistId": playlist_id}})).await?;
    match ytm_error(&v) {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Create a private playlist in the user's YouTube Music library from video ids; returns its playlistId.
pub async fn ytm_create_playlist(title: &str, video_ids: &[String]) -> Result<String, String> {
    if !yt_signed_in() {
        return Err("not_signed_in".into());
    }
    let v = ytm_post(
        "playlist/create",
        json!({"context": ytm_ctx(), "title": title, "description": "Made with Wallplayer", "privacyStatus": "PRIVATE", "videoIds": video_ids}),
    )
    .await?;
    if let Some(e) = ytm_error(&v) {
        return Err(e);
    }
    v.get("playlistId").and_then(|p| p.as_str()).map(String::from).ok_or_else(|| "YouTube did not create the playlist".into())
}
