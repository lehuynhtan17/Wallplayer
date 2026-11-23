//! Linux: questions to the X server (X11 sessions, and XWayland on Wayland — Wallplayer asks GTK
//! for X11 there, see desktop.rs). Pure Rust (x11rb): no extra system library is needed.
//!
//! * the monitors (RandR), their names (`HDMI-1`, `eDP-1`) and positions in pixels;
//! * the app windows on screen (EWMH: `_NET_CLIENT_LIST_STACKING`, `_NET_WM_STATE`…), to know
//!   whether an app covers a monitor (power.rs) or any app is visible (screen colours);
//! * a small piece of the screen (screen colours).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _, ImageFormat, MapState, Window};
use x11rb::rust_connection::RustConnection;

pub struct X {
    pub conn: RustConnection,
    pub root: Window,
    pub width: u16,
    pub height: u16,
    atoms: Mutex<HashMap<&'static str, u32>>,
}

static X_CONN: OnceLock<Option<X>> = OnceLock::new();

/// The shared connection (None without an X server, e.g. a pure Wayland session).
pub fn x() -> Option<&'static X> {
    X_CONN
        .get_or_init(|| {
            let (conn, n) = x11rb::connect(None).ok()?;
            let s = conn.setup().roots.get(n)?;
            let (root, width, height) = (s.root, s.width_in_pixels, s.height_in_pixels);
            Some(X { conn, root, width, height, atoms: Mutex::new(HashMap::new()) })
        })
        .as_ref()
}

impl X {
    pub fn atom(&self, name: &'static str) -> u32 {
        if let Some(a) = self.atoms.lock().unwrap().get(name) {
            return *a;
        }
        let a = self.conn.intern_atom(false, name.as_bytes()).ok().and_then(|c| c.reply().ok()).map(|r| r.atom).unwrap_or(0);
        if a != 0 {
            self.atoms.lock().unwrap().insert(name, a);
        }
        a
    }

    /// A 32-bit list property (windows, atoms, cardinals).
    pub fn prop32(&self, w: Window, name: &'static str) -> Vec<u32> {
        let a = self.atom(name);
        if a == 0 {
            return vec![];
        }
        self.conn
            .get_property(false, w, a, AtomEnum::ANY, 0, 4096)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().map(|v| v.collect()))
            .unwrap_or_default()
    }

    fn atom_name(&self, a: u32) -> String {
        self.conn.get_atom_name(a).ok().and_then(|c| c.reply().ok()).map(|r| String::from_utf8_lossy(&r.name).into_owned()).unwrap_or_default()
    }
}

/// (name, primary, x, y, w, h) in pixels, as RandR has them.
pub fn monitors() -> Vec<(String, bool, i32, i32, i32, i32)> {
    let Some(x) = x() else { return vec![] };
    let list = x.conn.randr_get_monitors(x.root, true).ok().and_then(|c| c.reply().ok()).map(|r| r.monitors).unwrap_or_default();
    let mut out: Vec<(String, bool, i32, i32, i32, i32)> = list
        .iter()
        .filter(|m| m.width > 0 && m.height > 0)
        .map(|m| {
            let name = x.atom_name(m.name);
            let name = if name.is_empty() { format!("monitor-{}-{}", m.x, m.y) } else { name };
            (name, m.primary, m.x as i32, m.y as i32, m.width as i32, m.height as i32)
        })
        .collect();
    if out.is_empty() {
        out.push(("screen".into(), true, 0, 0, x.width as i32, x.height as i32));
    }
    if !out.iter().any(|m| m.1) {
        // no primary set (common on a single screen): the one at the origin, else the first
        let i = out.iter().position(|m| m.2 == 0 && m.3 == 0).unwrap_or(0);
        out[i].1 = true;
    }
    out
}

/// One app window on screen.
#[derive(Debug, Clone)]
pub struct Client {
    pub win: Window,
    pub pid: u32,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub full: bool,
    pub max: bool,
    pub title: String,
    /// window type DESKTOP (a desktop / file manager background like ours)
    pub desktop: bool,
}

/// The app windows that can be seen right now (mapped, not minimised, on the current workspace),
/// bottom to top. Panels, docks, menus and notifications are left out; desktop windows are marked.
pub fn clients() -> Vec<Client> {
    let Some(x) = x() else { return vec![] };
    let mut list = x.prop32(x.root, "_NET_CLIENT_LIST_STACKING");
    if list.is_empty() {
        list = x.prop32(x.root, "_NET_CLIENT_LIST");
    }
    let cur_desk = x.prop32(x.root, "_NET_CURRENT_DESKTOP").first().copied();
    let (t_desktop, t_dock, t_normal, t_dialog, t_utility) = (
        x.atom("_NET_WM_WINDOW_TYPE_DESKTOP"),
        x.atom("_NET_WM_WINDOW_TYPE_DOCK"),
        x.atom("_NET_WM_WINDOW_TYPE_NORMAL"),
        x.atom("_NET_WM_WINDOW_TYPE_DIALOG"),
        x.atom("_NET_WM_WINDOW_TYPE_UTILITY"),
    );
    let (s_hidden, s_full, s_mv, s_mh) = (
        x.atom("_NET_WM_STATE_HIDDEN"),
        x.atom("_NET_WM_STATE_FULLSCREEN"),
        x.atom("_NET_WM_STATE_MAXIMIZED_VERT"),
        x.atom("_NET_WM_STATE_MAXIMIZED_HORZ"),
    );
    let mut out = Vec::new();
    for w in list {
        let Some(attr) = x.conn.get_window_attributes(w).ok().and_then(|c| c.reply().ok()) else { continue };
        if attr.map_state != MapState::VIEWABLE {
            continue;
        }
        if let (Some(cd), Some(d)) = (cur_desk, x.prop32(w, "_NET_WM_DESKTOP").first().copied()) {
            if d != cd && d != 0xFFFF_FFFF {
                continue; // another workspace
            }
        }
        let types = x.prop32(w, "_NET_WM_WINDOW_TYPE");
        let desktop = types.contains(&t_desktop);
        if types.contains(&t_dock) {
            continue;
        }
        if !desktop && !types.is_empty() && !types.iter().any(|t| *t == t_normal || *t == t_dialog || *t == t_utility) {
            continue; // menus, tooltips, notifications, splash screens…
        }
        let state = x.prop32(w, "_NET_WM_STATE");
        if state.contains(&s_hidden) {
            continue; // minimised
        }
        let Some(g) = x.conn.get_geometry(w).ok().and_then(|c| c.reply().ok()) else { continue };
        let Some(t) = x.conn.translate_coordinates(w, x.root, 0, 0).ok().and_then(|c| c.reply().ok()) else { continue };
        let text = |prop: u32| -> String {
            x.conn
                .get_property(false, w, prop, AtomEnum::ANY, 0, 64)
                .ok()
                .and_then(|c| c.reply().ok())
                .map(|r| String::from_utf8_lossy(&r.value).into_owned())
                .unwrap_or_default()
        };
        let mut title = text(x.atom("_NET_WM_NAME"));
        if title.is_empty() {
            title = text(AtomEnum::WM_NAME.into()); // older apps (xterm) only set this one
        }
        out.push(Client {
            win: w,
            pid: x.prop32(w, "_NET_WM_PID").first().copied().unwrap_or(0),
            x: t.dst_x as i32,
            y: t.dst_y as i32,
            w: g.width as i32,
            h: g.height as i32,
            full: state.contains(&s_full),
            max: state.contains(&s_mv) && state.contains(&s_mh),
            title,
            desktop,
        });
    }
    out
}

/// A piece of the screen as 32-bit BGRx pixels (row by row), or None.
pub fn grab(x0: i32, y0: i32, w: i32, h: i32) -> Option<Vec<u8>> {
    let x = x()?;
    if w <= 0 || h <= 0 {
        return None;
    }
    let r = x.conn.get_image(ImageFormat::Z_PIXMAP, x.root, x0 as i16, y0 as i16, w as u16, h as u16, !0).ok()?.reply().ok()?;
    // 24 / 32-bit visuals use 4 bytes a pixel; anything else is not worth decoding here
    if r.data.len() < (w * h * 4) as usize {
        return None;
    }
    Some(r.data)
}
