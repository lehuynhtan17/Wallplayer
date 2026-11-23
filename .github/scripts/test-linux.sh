#!/bin/bash
# Test run of the Linux build on a virtual desktop (Xvfb 1920x1080 + Openbox + PCManFM desktop icons
# + tray + PulseAudio virtual speaker), recorded as a video with a caption for each step.
# Usage: test-linux.sh <wallplayer program> <output folder>
set -u
APP="$1"; OUT="$2"; mkdir -p "$OUT"
HERE="$(cd "$(dirname "$0")" && pwd)"
CAP=/tmp/wp-cap.txt
FONT=$(fc-match -f '%{file}' 'Noto Sans:bold' 2>/dev/null || echo /usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf)
FAIL=0

cap() { printf '%s' "$1" > $CAP.tmp && mv $CAP.tmp $CAP; echo "[$(date +%T)] $1" | tee -a "$OUT/steps.txt"; }
t() { timeout 5 "$@"; }
check() { if eval "$2" > /dev/null 2>&1; then echo "PASS  $1" | tee -a "$OUT/checks.txt"; else echo "FAIL  $1" | tee -a "$OUT/checks.txt"; FAIL=1; fi; }

# ---------- test material: a song cover and a beat ----------
convert -size 300x300 gradient:'#e94560-#0f3460' /tmp/wp-test-cover.png
python3 - <<'EOF'
import wave, struct, math
fr = 48000
w = wave.open('/tmp/wp-beat.wav', 'wb'); w.setnchannels(2); w.setsampwidth(2); w.setframerate(fr)
data = bytearray()
for i in range(fr * 12):
    t = i / fr
    v = 0.8 * (1.0 if (t % 0.5) < 0.25 else 0.0) * math.sin(2 * math.pi * 60 * t) + 0.3 * math.sin(2 * math.pi * 440 * t)
    s = int(max(-1, min(1, v)) * 30000)
    data += struct.pack('<hh', s, s)
w.writeframes(bytes(data)); w.close()
EOF

# ---------- the virtual desktop ----------
Xvfb :99 -screen 0 1920x1080x24 +extension RANDR -nolisten tcp > "$OUT/xvfb.log" 2>&1 &
sleep 2
export DISPLAY=:99
eval "$(dbus-launch --sh-syntax)"
openbox > /dev/null 2>&1 &
pulseaudio --start --exit-idle-time=-1 > /dev/null 2>&1; sleep 1
pactl load-module module-null-sink sink_name=speaker sink_properties=device.description=Speaker > /dev/null 2>&1
pactl set-default-sink speaker; pactl set-sink-volume speaker 70%
mkdir -p ~/Desktop/Music && touch ~/Desktop/Notes.txt
pcmanfm --desktop > /dev/null 2>&1 &
stalonetray --geometry 6x1-0+0 -bg '#222' > /dev/null 2>&1 &
sleep 2

move() { eval "$(xdotool getmouselocation --shell)"; for i in $(seq 1 14); do t xdotool mousemove $((X + ($1 - X) * i / 14)) $((Y + ($2 - Y) * i / 14)); sleep 0.03; done; }
click() { move "$1" "$2"; sleep 0.25; t xdotool click 1; }
skin() { for w in $(t xprop -root _NET_CLIENT_LIST | grep -o '0x[0-9a-f]*'); do t xprop -id $w WM_CLASS | grep -qi wallplayer && { echo $w; return; }; done; }

# ---------- recording ----------
cap "Linux (Ubuntu, GitHub): desktop with file manager icons"
ffmpeg -hide_banner -loglevel error -y \
  -f x11grab -framerate 15 -video_size 1920x1080 -i :99 -f pulse -i speaker.monitor \
  -vf "scale=1280:720,drawtext=fontfile=$FONT:textfile=$CAP:reload=1:x=(w-text_w)/2:y=h-70:fontsize=26:fontcolor=white:box=1:boxcolor=black@0.65:boxborderw=14" \
  -c:v libx264 -preset veryfast -crf 27 -pix_fmt yuv420p -c:a aac -b:a 96k -shortest "$OUT/wallplayer-linux-test.mp4" > "$OUT/ffmpeg.log" 2>&1 &
FF=$!
sleep 3

cap "1. Start Wallplayer: the skin becomes the desktop wallpaper"
"$APP" > "$OUT/app.log" 2>&1 &
W=""; for i in $(seq 1 60); do W=$(skin); [ -n "$W" ] && break; sleep 0.5; done
sleep 5
check "app is running" 'pgrep -x wallplayer'
check "skin window exists" '[ -n "$W" ]'
check "skin is a desktop-layer window" 't xprop -id $W _NET_WM_WINDOW_TYPE | grep -q DESKTOP'
check "skin covers the screen (1920x1080)" 't xwininfo -id $W | grep -q "Width: 1920" && t xwininfo -id $W | grep -q "Height: 1080"'
check "skin is on every workspace" 't xprop -id $W _NET_WM_DESKTOP | grep -q 4294967295'
check "sound analysis (parec) runs" 'pgrep -x parec'

cap "2. Open another app: it sits above, the skin stays below like a wallpaper"
xterm -geometry 92x24+260+170 > /dev/null 2>&1 &
sleep 4
XT=$(t wmctrl -lx | awk 'tolower($3) ~ /xterm/{print $1; exit}')

cap "3. Show desktop: other apps go away, the skin stays"
t wmctrl -k on; sleep 3
check "skin stays on Show desktop" 't xwininfo -id $W | grep -q IsViewable'
t wmctrl -k off; sleep 2.5

cap "4. Ctrl+Alt+Space: the skin comes above every app; type to search"
t xdotool key ctrl+alt+space; sleep 2
check "Ctrl+Alt+Space raises the skin" 't xprop -id $W _NET_WM_STATE | grep -q ABOVE'
t xdotool type --delay 110 "nang tho hoang dung"; sleep 3

cap "5. Switch to another app: the skin goes back to the wallpaper layer"
t xdotool key Escape; sleep 0.5
[ -n "$XT" ] && t wmctrl -i -a "$XT"; sleep 3
check "skin back in the desktop layer" 't xprop -id $W _NET_WM_WINDOW_TYPE | grep -q DESKTOP'
[ -n "$XT" ] && t xdotool windowminimize "$XT"; sleep 1.5

cap "6. Music playing in another app (MPRIS, like Spotify) shows in the player"
python3 "$HERE/fake_mpris.py" > /dev/null 2>&1 &
sleep 5

cap "7. Pause / Next in Wallplayer control that app"
click 700 848; sleep 2.5
click 745 848; sleep 3
check "Pause / Next reached the other app" 'grep -q PlayPause /tmp/fake_mpris.log && grep -q Next /tmp/fake_mpris.log'
click 700 848; sleep 2.5

cap "8. System volume: click for the slider, click to change it"
click 1268 861; sleep 2
click 1150 788; sleep 1.5
check "volume slider changed the system volume" 'pactl get-sink-volume speaker | grep -qv " 70%"'
click 1210 788; sleep 1.5
t xdotool key Escape; move 1500 500; sleep 1

cap "9. The edge light follows the system sound (with sound)"
paplay --device=speaker /tmp/wp-beat.wav &
sleep 8
pkill paplay; sleep 1.5

cap "10. Tray menu (top right)"
click 1788 12; sleep 3
t xdotool key Escape; sleep 1

cap "11. Plug in a second monitor: the right one gets a copy of the skin"
t xrandr --setmonitor LEFT-1 960/254x1080/286+0+0 screen > /dev/null
t xrandr --setmonitor RIGHT-2 960/254x1080/286+960+0 none > /dev/null
sleep 8
check "skin fits the left monitor (960 wide)" 't xwininfo -id $W | grep -q "Width: 960"'
check "a copy opened on the right monitor" '[ $(for w in $(t xprop -root _NET_CLIENT_LIST | grep -o "0x[0-9a-f]*"); do t xprop -id $w WM_CLASS | grep -qi wallplayer && echo $w; done | wc -l) -ge 2 ]'

cap "12. Unplug it: the skin fills the whole screen again"
t xrandr --delmonitor RIGHT-2 > /dev/null; t xrandr --delmonitor LEFT-1 > /dev/null
sleep 6
check "skin back to 1920 wide" 't xwininfo -id $W | grep -q "Width: 1920"'
check "app still running at the end" 'pgrep -x wallplayer'

cap "End: Wallplayer on Linux"
sleep 3
kill -INT $FF; wait $FF 2>/dev/null
import -window root "$OUT/last-frame.png" 2>/dev/null
pkill -x wallplayer; pkill -f fake_mpris; pkill -x xterm
echo; cat "$OUT/checks.txt"
exit $FAIL
