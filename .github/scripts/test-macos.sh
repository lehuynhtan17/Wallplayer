#!/bin/bash
# Test run of the macOS build on GitHub's Mac, recorded as a video with a caption for each step.
# Usage: test-macos.sh <Wallplayer.app> <output folder>
set -u
APP="$1"; OUT="$2"; mkdir -p "$OUT"
HERE="$(cd "$(dirname "$0")" && pwd)"
BIN=$(find "$APP/Contents/MacOS" -type f -perm -u+x | head -1)
FAIL=0
STEPS="$OUT/steps.tsv"; : > "$STEPS"
T0=$(date +%s)

swiftc -O -o /tmp/macwins "$HERE/mac-windows.swift" || echo "swift helper failed"
cap() { printf '%s\t%s\n' "$(( $(date +%s) - T0 ))" "$1" >> "$STEPS"; echo "[$(date +%T)] $1"; }
shot() { screencapture -x "$OUT/$1.png" 2>> "$OUT/screencapture.log"; }
wins() { /tmp/macwins "$PID" > "$OUT/windows-$1.txt" 2>&1; cat "$OUT/windows-$1.txt" | head -12; }
check() { if eval "$2" > /dev/null 2>&1; then echo "PASS  $1" | tee -a "$OUT/checks.txt"; else echo "FAIL  $1" | tee -a "$OUT/checks.txt"; FAIL=1; fi; }
key() { osascript -e "tell application \"System Events\" to $1" 2>> "$OUT/osascript.log"; }
# the skin's own window (not its tray icon in the menu bar): Wallplayer's window wider than 200 points
skinline() { grep "^MINE" "$OUT/windows-$1.txt" | awk '{split($3, a, "="); split(a[2], b, "x"); if (b[1] + 0 > 200) { print; exit }}'; }
skinrow() { grep -n "^MINE" "$OUT/windows-$1.txt" | awk -F: '{split($2, f, " "); split(f[3], a, "="); split(a[2], b, "x"); if (b[1] + 0 > 200) { print $1; exit }}'; }
level() { skinline "$1" | sed -E 's/.*layer=(-?[0-9]+).*/\1/'; }
lvl() { grep -m1 "^levels" "$OUT/windows-$1.txt" | sed -E "s/.*$2=(-?[0-9]+).*/\1/"; }

# ---------- recording (the whole screen) ----------
ffmpeg -hide_banner -f avfoundation -list_devices true -i "" > "$OUT/devices.txt" 2>&1
SCREEN=$(grep -m1 "Capture screen 0" "$OUT/devices.txt" | sed -E 's/.*\[([0-9]+)\] Capture screen 0.*/\1/')
ffmpeg -hide_banner -loglevel error -y -f avfoundation -capture_cursor 1 -framerate 15 -i "${SCREEN:-1}:none" \
  -c:v libx264 -preset ultrafast -crf 23 -pix_fmt yuv420p "$OUT/raw.mp4" > "$OUT/ffmpeg.log" 2>&1 &
FF=$!
T0=$(date +%s)
# also one still a second, in case the video recording is not allowed on this Mac
mkdir -p /tmp/wp-stills
( while [ ! -f /tmp/wp-stop ]; do screencapture -x -t jpg "/tmp/wp-stills/$(date +%s).jpg" 2>/dev/null; sleep 1; done ) &
sleep 2
cap "macOS (a real Mac at GitHub): the desktop"
sleep 3

cap "1. Start Wallplayer: the skin becomes the desktop of every Space"
"$BIN" > "$OUT/app.log" 2>&1 &
PID=$!
for i in $(seq 1 60); do /tmp/macwins "$PID" | grep "^MINE" | grep -qv "layer=25 " && break; sleep 0.5; done
sleep 6
wins 1-start; shot 1-start
D=$(sed -E 's/display ([0-9]+)x([0-9]+).*/\1x\2/;q' "$OUT/windows-1-start.txt")
check "app is running" 'kill -0 $PID'
check "skin window is on screen" '[ -n "$(skinline 1-start)" ]'
check "skin is just above the desktop icons, under every app" '[ "$(level 1-start)" = "$(( $(lvl 1-start desktopIcon) + 1 ))" ]'
check "skin covers the screen ($D)" 'skinline 1-start | grep -q "size=$D "'

cap "2. Open another app (Calculator): it sits above the skin"
open -a Calculator; sleep 4
wins 2-app; shot 2-app
check "the app window is above the skin" '[ "$(grep -n "owner=Calculator" "$OUT/windows-2-app.txt" | head -1 | cut -d: -f1)" -lt "$(skinrow 2-app)" ]'

cap "3. Shift+Command+Space: the skin comes above every app; type to search"
key 'keystroke space using {shift down, command down}'; sleep 2.5
wins 3-search
check "Shift+Command+Space raises the skin above the apps" '[ "$(level 3-search)" = "$(lvl 3-search floating)" ]'
key 'keystroke "nang tho hoang dung"'; sleep 7
shot 3-search

cap "4. Click the first top result: it plays from YouTube Music (real internet)"
W=${D%x*}; H=${D#*x}
/tmp/macwins click $(( W * 328 / 1000 )) $(( H * 483 / 1000 )); sleep 15
shot 4-playing

cap "5. Self-test (Ctrl+Shift+D): volume, desktop, YouTube"
key 'keystroke "d" using {control down, shift down}'; sleep 6
shot 5-selftest

cap "6. Switch to the other app: the skin goes back to the desktop"
osascript -e 'tell application "Calculator" to activate' 2>> "$OUT/osascript.log"; sleep 3
wins 6-back; shot 6-back
check "skin back just above the desktop icons" '[ "$(level 6-back)" = "$(( $(lvl 6-back desktopIcon) + 1 ))" ]'
osascript -e 'quit app "Calculator"' 2>> "$OUT/osascript.log"; sleep 2

cap "7. Point at the volume icon: the slider; one click (Finder in front): mute"
/tmp/macwins move $(( W * 681 / 1000 )) $(( H * 779 / 1000 )); sleep 3
shot 7-volume
/tmp/macwins click $(( W * 681 / 1000 )) $(( H * 779 / 1000 )); sleep 3
shot 7-muted
osascript -e 'set volume output volume 30' 2>> "$OUT/osascript.log"; sleep 3

cap "8. Mission Control: the skin stays on the desktop"
open -a "Mission Control" 2>> "$OUT/osascript.log"; sleep 3
shot 8-mission
open -a "Mission Control" 2>> "$OUT/osascript.log"; sleep 2

check "app still running at the end" 'kill -0 $PID'
cap "End: Wallplayer on macOS"
sleep 3
kill -INT $FF; wait $FF 2>/dev/null
touch /tmp/wp-stop
kill $PID 2>/dev/null
# no video (recording refused)? make one from the stills, one second each
if [ ! -s "$OUT/raw.mp4" ] && ls /tmp/wp-stills/*.jpg > /dev/null 2>&1; then
  echo "screen video unavailable: building it from the stills"
  ffmpeg -hide_banner -loglevel error -y -framerate 1 -pattern_type glob -i '/tmp/wp-stills/*.jpg' -vf "fps=15,scale=-2:720" \
    -c:v libx264 -preset ultrafast -pix_fmt yuv420p "$OUT/raw.mp4"
fi

# captions are added on Linux (caption-video.sh): this Mac's ffmpeg has no text filter
echo; cat "$OUT/checks.txt"
exit $FAIL
