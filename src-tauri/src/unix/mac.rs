//! macOS: small CoreGraphics helpers (displays, window levels, the windows on screen). These calls
//! are safe from any thread, unlike AppKit's.

#![allow(non_upper_case_globals, non_snake_case)]

use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

// CGWindowLevelKey
pub const kCGDesktopWindowLevelKey: i32 = 2;
pub const kCGFloatingWindowLevelKey: i32 = 5;
pub const kCGDesktopIconWindowLevelKey: i32 = 18;
// CGWindowListOption
const kCGWindowListOptionOnScreenOnly: u32 = 1 << 0;
const kCGWindowListExcludeDesktopElements: u32 = 1 << 4;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
    fn CGDisplayBounds(display: u32) -> CGRect;
    fn CGMainDisplayID() -> u32;
    fn CGDisplayVendorNumber(display: u32) -> u32;
    fn CGDisplayModelNumber(display: u32) -> u32;
    fn CGDisplaySerialNumber(display: u32) -> u32;
    fn CGWindowLevelForKey(key: i32) -> i32;
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> CFArrayRef;
    fn CGRectMakeWithDictionaryRepresentation(dict: CFDictionaryRef, rect: *mut CGRect) -> bool;
}

pub fn level(key: i32) -> isize {
    unsafe { CGWindowLevelForKey(key) as isize }
}

/// Height of the main display (points): Cocoa counts y from its bottom edge, CoreGraphics from its top.
pub fn main_height() -> f64 {
    unsafe { CGDisplayBounds(CGMainDisplayID()).size.height }
}

/// (id, main display, bounds in points, top-left origin). The id names the screen itself
/// (vendor, model, serial) so the choice survives a restart.
pub fn displays() -> Vec<(String, bool, CGRect)> {
    let mut ids = [0u32; 32];
    let mut n = 0u32;
    unsafe {
        if CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut n) != 0 {
            return vec![];
        }
        let main = CGMainDisplayID();
        let mut out: Vec<(String, bool, CGRect)> = Vec::new();
        for &d in &ids[..(n as usize).min(ids.len())] {
            let mut id = format!("{:04x}{:04x}-{:x}", CGDisplayVendorNumber(d), CGDisplayModelNumber(d), CGDisplaySerialNumber(d));
            if out.iter().any(|(o, _, _)| *o == id) {
                id = format!("{id}-{d}"); // two identical screens without serial numbers
            }
            out.push((id, d == main, CGDisplayBounds(d)));
        }
        out
    }
}

/// One window on screen, from the window server's list.
#[derive(Debug, Clone)]
pub struct Win {
    pub pid: i64,
    pub layer: i64,
    pub alpha: f64,
    pub bounds: CGRect,
    pub owner: String,
}

/// The windows on screen, front to back (other spaces and minimised windows are not in it).
pub fn windows() -> Vec<Win> {
    unsafe {
        let raw = CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements, 0);
        if raw.is_null() {
            return vec![];
        }
        let arr: CFArray<CFDictionary<CFString, CFType>> = CFArray::wrap_under_create_rule(raw);
        let k_layer = CFString::from_static_string("kCGWindowLayer");
        let k_pid = CFString::from_static_string("kCGWindowOwnerPID");
        let k_alpha = CFString::from_static_string("kCGWindowAlpha");
        let k_bounds = CFString::from_static_string("kCGWindowBounds");
        let k_owner = CFString::from_static_string("kCGWindowOwnerName");
        let num = |d: &CFDictionary<CFString, CFType>, k: &CFString| d.find(k).and_then(|v| v.downcast::<CFNumber>());
        let mut out = Vec::new();
        for d in arr.iter() {
            let mut bounds = CGRect::default();
            let ok = d
                .find(&k_bounds)
                .and_then(|v| v.downcast::<CFDictionary>())
                .map(|b| CGRectMakeWithDictionaryRepresentation(b.as_concrete_TypeRef(), &mut bounds))
                .unwrap_or(false);
            if !ok {
                continue;
            }
            out.push(Win {
                pid: num(&d, &k_pid).and_then(|n| n.to_i64()).unwrap_or(0),
                layer: num(&d, &k_layer).and_then(|n| n.to_i64()).unwrap_or(0),
                alpha: num(&d, &k_alpha).and_then(|n| n.to_f64()).unwrap_or(1.0),
                bounds,
                owner: d.find(&k_owner).and_then(|v| v.downcast::<CFString>()).map(|s| s.to_string()).unwrap_or_default(),
            });
        }
        out
    }
}

/// App windows (normal level, visible, not ours) as (owner, bounds).
pub fn app_windows() -> Vec<(String, CGRect)> {
    let me = std::process::id() as i64;
    windows()
        .into_iter()
        .filter(|w| w.layer == 0 && w.pid != me && w.alpha > 0.05 && w.bounds.size.width >= 120.0 && w.bounds.size.height >= 80.0)
        .map(|w| (w.owner, w.bounds))
        .collect()
}
