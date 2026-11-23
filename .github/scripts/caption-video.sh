#!/bin/bash
# Burns the step captions into a test video (720p), on Linux (its ffmpeg has the text filter).
# Usage: caption-video.sh <raw video> <steps.tsv: seconds TAB caption> <output.mp4>
set -eu
RAW="$1"; STEPS="$2"; OUT="$3"
FONT=$(fc-match -f '%{file}' 'Noto Sans:bold' 2>/dev/null || echo /usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf)
FILTER="scale=-2:720"
i=0
while IFS=$'\t' read -r s text; do
  i=$((i + 1))
  e=$(sed -n "$((i + 1))p" "$STEPS" | cut -f1); e=${e:-99999}
  printf '%s' "$text" > "/tmp/cap$i.txt"
  FILTER="$FILTER,drawtext=fontfile=$FONT:textfile=/tmp/cap$i.txt:enable='between(t,$s,$e)':x=(w-text_w)/2:y=h-70:fontsize=26:fontcolor=white:box=1:boxcolor=black@0.65:boxborderw=14"
done < "$STEPS"
ffmpeg -hide_banner -loglevel error -y -i "$RAW" -vf "$FILTER" -c:v libx264 -preset veryfast -crf 27 -pix_fmt yuv420p -an "$OUT"
