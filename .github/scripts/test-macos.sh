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
level() { grep -m1 "^MINE" "$OUT/windows-$1.txt" | sed -E 's/.*layer=(-?[0-9]+).*/\1/'; }
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
for i in $(seq 1 60); do /tmp/macwins "$PID" | grep -q "^MINE" && break; sleep 0.5; done
sleep 6
wins 1-start; shot 1-start
D=$(sed -E 's/display ([0-9]+)x([0-9]+).*/\1x\2/;q' "$OUT/windows-1-start.txt")
check "app is running" 'kill -0 $PID'
check "skin window is on screen" 'grep -q "^MINE" "$OUT/windows-1-start.txt"'
check "skin is just above the desktop icons, under every app" '[ "$(level 1-start)" = "$(( $(lvl 1-start desktopIcon) + 1 ))" ]'
check "skin covers the screen ($D)" 'grep -m1 "^MINE" "$OUT/windows-1-start.txt" | grep -q "size=$D "'

cap "2. Open another app (Calculator): it sits above the skin"
open -a Calculator; sleep 4
wins 2-app; shot 2-app
check "the app window is above the skin" '[ "$(grep -n "owner=Calculator" "$OUT/windows-2-app.txt" | head -1 | cut -d: -f1)" -lt "$(grep -n "^MINE" "$OUT/windows-2-app.txt" | head -1 | cut -d: -f1)" ]'

cap "3. Shift+Command+Space: the skin comes above every app; type to search"
key 'keystroke space using {shift down, command down}'; sleep 2.5
wins 3-search
check "Shift+Command+Space raises the skin above the apps" '[ "$(level 3-search)" = "$(lvl 3-search floating)" ]'
key 'keystroke "nang tho"'; sleep 3
shot 3-search

cap "4. Self-test (Ctrl+Shift+D): volume, desktop, screen colours"
key 'keystroke "d" using {control down, shift down}'; sleep 5
shot 4-selftest

cap "5. Switch to the other app: the skin goes back to the desktop"
osascript -e 'tell application "Calculator" to activate' 2>> "$OUT/osascript.log"; sleep 3
wins 5-back; shot 5-back
check "skin back just above the desktop icons" '[ "$(level 5-back)" = "$(( $(lvl 5-back desktopIcon) + 1 ))" ]'
osascript -e 'quit app "Calculator"' 2>> "$OUT/osascript.log"; sleep 2

cap "6. Open the volume slider (system volume)"
W=${D%x*}; H=${D#*x}
/tmp/macwins click $(( W * 660 / 1000 )) $(( H * 797 / 1000 )); sleep 3
shot 6-volume
osascript -e 'set volume output volume 30' 2>> "$OUT/osascript.log"; sleep 3

cap "7. Mission Control: the skin stays on the desktop"
open -a "Mission Control" 2>> "$OUT/osascript.log"; sleep 3
shot 7-mission
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

# ---------- captions on the video ----------
cp "/System/Library/Fonts/Supplemental/Arial Bold.ttf" /tmp/capfont.ttf 2>/dev/null || cp /System/Library/Fonts/Helvetica.ttc /tmp/capfont.ttf
FILTER="scale=-2:720"
N=$(wc -l < "$STEPS"); i=0
while IFS=$'\t' read -r s text; do
  i=$((i + 1))
  e=$(sed -n "$((i + 1))p" "$STEPS" | cut -f1); e=${e:-99999}
  printf '%s' "$text" > "/tmp/cap$i.txt"
  FILTER="$FILTER,drawtext=fontfile=/tmp/capfont.ttf:textfile=/tmp/cap$i.txt:enable='between(t,$s,$e)':x=(w-text_w)/2:y=h-70:fontsize=26:fontcolor=white:box=1:boxcolor=black@0.65:boxborderw=14"
done < "$STEPS"
ffmpeg -hide_banner -loglevel error -y -i "$OUT/raw.mp4" -vf "$FILTER" -c:v libx264 -preset veryfast -crf 27 -pix_fmt yuv420p \
  "$OUT/wallplayer-macos-test.mp4" && rm -f "$OUT/raw.mp4"

echo; cat "$OUT/checks.txt"
exit $FAIL
