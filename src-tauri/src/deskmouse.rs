//! Mouse forwarding for "Wallpaper" mode (the way Wallpaper Engine does it).
//!
//! In Wallpaper mode the skin lives inside the shell's WorkerW, *behind* the desktop
//! icons, so Windows never delivers mouse input to it: the icon layer is on top.
//! A low-level mouse hook watches the cursor instead. When it is over empty desktop
//! (not over an icon, not over another app) the position and buttons are sent to the
//! page as `desk-mouse` events, which the page turns into hover / click / drag / wheel. Over another
//! display the same goes to that display's mirror window (`desk-mouse-<its label>`), so its playlist,
//! cover flow and TikTok grid can be clicked too.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::json;
use tauri::{AppHandle, Emitter};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
use windows::Win32::System::Memory::{VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::desktop;

struct Ev {
    k: &'static str,
    x: i32,
    y: i32,
    d: i32,
    /// "" = the main skin, else a mirror window's label
    to: String,
}

/// Where the mouse is being sent: the window it hovers, and the one holding a press (with the origin
/// of its display, so a drag keeps its coordinates even past the display's edge).
struct Route {
    hover: Option<String>,
    cap: Option<(String, i32, i32)>,
}
static ROUTE: Mutex<Route> = Mutex::new(Route { hover: None, cap: None });

static TX: OnceLock<Mutex<Sender<Ev>>> = OnceLock::new();
static LAST_MOVE: Mutex<Option<Instant>> = Mutex::new(None);
static OWN_PID: AtomicIsize = AtomicIsize::new(0);

// "Search from anywhere" bound to a mouse button (1 = middle, 2 = X1/back, 3 = X2/forward; 0 = none)
static BIND_BTN: AtomicU32 = AtomicU32::new(0);
static BIND_MODS: AtomicU32 = AtomicU32::new(0); // 1 ctrl · 2 alt · 4 shift · 8 win
static CAPTURE: AtomicBool = AtomicBool::new(false);
static SWALLOW_UP: AtomicU32 = AtomicU32::new(0);

pub fn set_bind(btn: u32, mods: u32) {
    BIND_BTN.store(btn, Ordering::Relaxed);
    BIND_MODS.store(mods, Ordering::Relaxed);
}
pub fn set_capture(on: bool) {
    CAPTURE.store(on, Ordering::Relaxed);
}

unsafe fn held_mods() -> u32 {
    let down = |vk: u16| (GetAsyncKeyState(vk as i32) as u16) & 0x8000 != 0;
    (down(VK_CONTROL.0) as u32) | ((down(VK_MENU.0) as u32) << 1) | ((down(VK_SHIFT.0) as u32) << 2) | (((down(VK_LWIN.0) || down(VK_RWIN.0)) as u32) << 3)
}

/// 0 = not a bindable button. Left / right are never bindable.
fn side_button(msg: u32, data: u32) -> (u32, bool) {
    match msg {
        WM_MBUTTONDOWN => (1, true),
        WM_MBUTTONUP => (1, false),
        WM_XBUTTONDOWN | WM_XBUTTONUP => (if (data >> 16) & 0xFFFF == 2 { 3 } else { 2 }, msg == WM_XBUTTONDOWN),
        _ => (0, false),
    }
}

fn send(k: &'static str, x: i32, y: i32, d: i32) {
    send_to(k, x, y, d, "");
}

fn send_to(k: &'static str, x: i32, y: i32, d: i32, to: &str) {
    if let Some(tx) = TX.get() {
        if let Ok(tx) = tx.lock() {
            let _ = tx.send(Ev { k, x, y, d, to: to.to_string() });
        }
    }
}

unsafe fn class_of(h: HWND) -> String {
    let mut b = [0u16; 64];
    let n = GetClassNameW(h, &mut b) as usize;
    String::from_utf16_lossy(&b[..n.min(64)])
}

/// Is there an icon under the cursor? (asks the desktop's list view; needs memory inside explorer)
unsafe fn over_icon(lv: HWND, pt: POINT) -> bool {
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct HitTest {
        x: i32,
        y: i32,
        flags: u32,
        item: i32,
        sub: i32,
        group: i32,
    }
    let mut c = pt;
    if !ScreenToClient(lv, &mut c).as_bool() {
        return false;
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(lv, Some(&mut pid));
    let Ok(proc) = OpenProcess(PROCESS_VM_OPERATION | PROCESS_VM_READ | PROCESS_VM_WRITE, false, pid) else {
        return false;
    };
    let size = std::mem::size_of::<HitTest>();
    let remote = VirtualAllocEx(proc, None, size, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
    let mut hit = false;
    if !remote.is_null() {
        let h = HitTest { x: c.x, y: c.y, ..Default::default() };
        if WriteProcessMemory(proc, remote, &h as *const _ as *const _, size, None).is_ok() {
            // LVM_HITTEST = 0x1012; result is the item index (-1 = none)
            let mut res: usize = 0;
            let _ = SendMessageTimeoutW(lv, 0x1012, WPARAM(0), LPARAM(remote as isize), SMTO_ABORTIFHUNG, 80, Some(&mut res));
            let mut out = HitTest::default();
            if ReadProcessMemory(proc, remote, &mut out as *mut _ as *mut _, size, None).is_ok() {
                // LVHT_ONITEM = 0x0E (icon, label or state image)
                hit = out.item >= 0 && (out.flags & 0x0E) != 0;
            }
        }
        let _ = VirtualFreeEx(proc, remote, 0, MEM_RELEASE);
    }
    let _ = CloseHandle(proc);
    hit
}

/// Cursor is over the bare desktop (or over us), i.e. nothing else would take the click.
unsafe fn over_desktop(pt: POINT, check_icon: bool) -> bool {
    let w = WindowFromPoint(pt);
    if w.is_invalid() {
        return false;
    }
    // only the skin itself (its web view) counts as "us"; our tray menu, login windows etc. keep their own clicks
    let main = HWND(desktop::MAIN.load(Ordering::Relaxed) as *mut core::ffi::c_void);
    if !main.is_invalid() && (w == main || IsChild(main, w).as_bool()) {
        return true;
    }
    if desktop::is_mirror_window(w) {
        return true;
    }
    match class_of(w).as_str() {
        "SysListView32" => !(check_icon && over_icon(w, pt)),
        "SHELLDLL_DefView" | "WorkerW" | "Progman" => true,
        _ => false,
    }
}

unsafe extern "system" fn hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(None, code, wp, lp);
    }
    let ms = &*(lp.0 as *const MSLLHOOKSTRUCT);
    if ms.flags & 1 != 0 {
        return CallNextHookEx(None, code, wp, lp); // injected by us / another app
    }
    // ── the user's "Search from anywhere" mouse button ──
    let (btn, down) = side_button(wp.0 as u32, ms.mouseData);
    if btn != 0 {
        if down {
            if CAPTURE.swap(false, Ordering::Relaxed) {
                SWALLOW_UP.store(btn, Ordering::Relaxed);
                send("bind", 0, 0, ((held_mods() << 8) | btn) as i32);
                return LRESULT(1);
            }
            if BIND_BTN.load(Ordering::Relaxed) == btn && BIND_MODS.load(Ordering::Relaxed) == held_mods() {
                SWALLOW_UP.store(btn, Ordering::Relaxed);
                send("search", 0, 0, 0);
                return LRESULT(1);
            }
        } else if SWALLOW_UP.swap(0, Ordering::Relaxed) == btn {
            return LRESULT(1);
        }
        return CallNextHookEx(None, code, wp, lp);
    }
    if !desktop::is_wallpaper() {
        return CallNextHookEx(None, code, wp, lp);
    }
    let pt = ms.pt;
    let (mx, my, sw, sh) = desktop::target_rect();
    // the window under the mouse: the main skin ("") or the mirror on another display
    let at: Option<(String, i32, i32)> = if pt.x >= mx && pt.x < mx + sw && pt.y >= my && pt.y < my + sh {
        Some((String::new(), pt.x - mx, pt.y - my))
    } else {
        crate::mirror::target_at(pt.x, pt.y)
    };
    let msg = wp.0 as u32;
    let mut r = ROUTE.lock().unwrap_or_else(|e| e.into_inner());

    match msg {
        WM_MOUSEMOVE => {
            if let Some((to, ox, oy)) = r.cap.clone() {
                send_to("move", pt.x - ox, pt.y - oy, 0, &to);
            } else if let Some((to, x, y)) = at.filter(|_| over_desktop(pt, false)) {
                if r.hover.as_deref() != Some(to.as_str()) {
                    if let Some(old) = r.hover.replace(to.clone()) {
                        send_to("leave", 0, 0, 0, &old);
                    }
                }
                let mut last = LAST_MOVE.lock().unwrap_or_else(|e| e.into_inner());
                if last.map(|t| t.elapsed() >= Duration::from_millis(14)).unwrap_or(true) {
                    *last = Some(Instant::now());
                    send_to("move", x, y, 0, &to);
                }
            } else if let Some(old) = r.hover.take() {
                send_to("leave", 0, 0, 0, &old);
            }
        }
        WM_LBUTTONDOWN => {
            if let Some((to, x, y)) = at.filter(|_| over_desktop(pt, true)) {
                r.cap = Some((to.clone(), pt.x - x, pt.y - y));
                send_to("down", x, y, 0, &to);
                return LRESULT(1); // the desktop must not start a selection rectangle
            }
        }
        WM_LBUTTONUP => {
            if let Some((to, ox, oy)) = r.cap.take() {
                send_to("up", pt.x - ox, pt.y - oy, 0, &to);
                return LRESULT(1);
            }
        }
        WM_MOUSEWHEEL => {
            if let Some((to, x, y)) = at.filter(|_| over_desktop(pt, false)) {
                send_to("wheel", x, y, ((ms.mouseData >> 16) as i16) as i32, &to);
            }
        }
        _ => {}
    }
    drop(r);
    CallNextHookEx(None, code, wp, lp)
}

/// Start the hook (own thread with a message loop) and the thread that relays events to the page.
pub fn start(app: AppHandle) {
    let (tx, rx) = channel::<Ev>();
    let _ = TX.set(Mutex::new(tx));
    unsafe { OWN_PID.store(GetCurrentProcessId() as isize, Ordering::Relaxed) };
    std::thread::spawn(move || {
        for e in rx {
            match e.k {
                "search" => crate::open_search(&app),
                "bind" => {
                    let (btn, mods) = ((e.d & 0xFF) as u32, ((e.d >> 8) & 0xFF) as u32);
                    let mut spec = String::new();
                    for (bit, name) in [(1u32, "Ctrl+"), (2, "Alt+"), (4, "Shift+"), (8, "Super+")] {
                        if mods & bit != 0 {
                            spec.push_str(name);
                        }
                    }
                    spec.push_str(match btn { 1 => "Mouse:Middle", 2 => "Mouse:X1", _ => "Mouse:X2" });
                    let _ = app.emit("bind-captured", spec);
                }
                _ => {
                    // the main skin listens to "desk-mouse", a mirror window to "desk-mouse-<its label>"
                    let ev = if e.to.is_empty() { "desk-mouse".to_string() } else { format!("desk-mouse-{}", e.to) };
                    let _ = app.emit(&ev, json!({"k": e.k, "x": e.x, "y": e.y, "d": e.d}));
                }
            }
        }
    });
    std::thread::spawn(|| unsafe {
        if SetWindowsHookExW(WH_MOUSE_LL, Some(hook), None, 0).is_err() {
            return;
        }
        let mut m = MSG::default();
        while GetMessageW(&mut m, HWND::default(), 0, 0).as_bool() {
            let _ = TranslateMessage(&m);
            DispatchMessageW(&m);
        }
    });
}
