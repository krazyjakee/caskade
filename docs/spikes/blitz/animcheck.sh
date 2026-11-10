#!/bin/bash
# usage: animcheck.sh <label> cmd...  — two screenshots 1 s apart; prints changed-pixel %
label=$1; shift
DISPLAY=:147 setsid "$@" >/dev/null 2>&1 & pid=$!
sleep 15
import -display :147 -window root /mnt/Rusty/caskade-blitz-spike/out/$label-a.png
sleep 1.3
import -display :147 -window root /mnt/Rusty/caskade-blitz-spike/out/$label-b.png
kill -TERM -- -$pid 2>/dev/null || kill $pid
sleep 1
python3 -I - "$label" <<'PY'
import sys
from PIL import Image, ImageChops
l = sys.argv[1]
a = Image.open(f"/mnt/Rusty/caskade-blitz-spike/out/{l}-a.png").convert("RGB")
b = Image.open(f"/mnt/Rusty/caskade-blitz-spike/out/{l}-b.png").convert("RGB")
d = ImageChops.difference(a, b).convert("L").point(lambda v: 255 if v > 2 else 0)
print(f"{l}: changed pixels {100*d.histogram()[255]/(a.width*a.height):.2f}%  bbox={d.getbbox()}")
PY
