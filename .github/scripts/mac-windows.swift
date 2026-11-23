// Test helper for the macOS test run (test-macos.sh).
//   mac-windows <pid>      the windows on screen, front to back ("MINE" = Wallplayer's), with the
//                          desktop / desktop icon / floating window levels to compare with
//   mac-windows click X Y  one left click at X, Y (points, top-left origin)
import CoreGraphics
import Foundation

let args = CommandLine.arguments
if args.count >= 4 && args[1] == "click", let x = Double(args[2]), let y = Double(args[3]) {
    let p = CGPoint(x: x, y: y)
    let src = CGEventSource(stateID: .hidSystemState)
    CGEvent(mouseEventSource: src, mouseType: .mouseMoved, mouseCursorPosition: p, mouseButton: .left)?.post(tap: .cghidEventTap)
    usleep(200_000)
    CGEvent(mouseEventSource: src, mouseType: .leftMouseDown, mouseCursorPosition: p, mouseButton: .left)?.post(tap: .cghidEventTap)
    usleep(90_000)
    CGEvent(mouseEventSource: src, mouseType: .leftMouseUp, mouseCursorPosition: p, mouseButton: .left)?.post(tap: .cghidEventTap)
    exit(0)
}

let pid = args.count > 1 ? Int(args[1]) ?? 0 : 0
let main = CGDisplayBounds(CGMainDisplayID())
print("display \(Int(main.width))x\(Int(main.height))")
print("levels desktop=\(CGWindowLevelForKey(.desktopWindow)) desktopIcon=\(CGWindowLevelForKey(.desktopIconWindow)) floating=\(CGWindowLevelForKey(.floatingWindow))")
let list = (CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]]) ?? []
for w in list {
    let owner = w[kCGWindowOwnerName as String] as? String ?? "?"
    let wpid = w[kCGWindowOwnerPID as String] as? Int ?? 0
    let layer = w[kCGWindowLayer as String] as? Int ?? 0
    let b = w[kCGWindowBounds as String] as? [String: Any] ?? [:]
    let x = (b["X"] as? Double).map { Int($0) } ?? 0
    let y = (b["Y"] as? Double).map { Int($0) } ?? 0
    let wd = (b["Width"] as? Double).map { Int($0) } ?? 0
    let ht = (b["Height"] as? Double).map { Int($0) } ?? 0
    print("\(wpid == pid ? "MINE" : "    ") layer=\(layer) size=\(wd)x\(ht) at=\(x),\(y) owner=\(owner)")
}
