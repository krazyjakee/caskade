#!/usr/bin/env bash
# Every animation in themes/ (assets/gallery.html with the app's compiled CSS)
# in Blitz (the prototype) and in WebKitGTK (MiniBrowser), one after the other on
# a private desktop (scripts/private-desktop.sh: its own Xvfb and session bus,
# never the developer's, #122): after 5 s the pointer moves onto the card (on
# that private display only), then 24 screenshots 125 ms apart. Reports, per animated element,
# whether and how much it moved, and keeps a contact sheet of both.
#
#   gallery.sh <prototype binary> <library copy> <out dir>
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
if [ -z "${CASKADE_PRIVATE_DESKTOP:-}" ]; then
  exec "$here/../../../scripts/private-desktop.sh" --size 1000x700 -- "$0" "$@"
fi
proto=$(cd "$here/.." && pwd)
bin=$(realpath "$1") home=$(realpath "$2") out=$3
mkdir -p "$out"
out=$(realpath "$out")

{
  printf '<!doctype html><html><head><meta charset="utf-8"><style>\n'
  cat "$proto/assets/app.css"
  printf '\n</style></head><body style="margin: 0; --cs-motion-ambient: running">\n'
  cat "$proto/assets/gallery.html"
  printf '</body></html>\n'
} >"$out/gallery-page.html"

capture() { # name, command...
  local name=$1
  shift
  local d=$DISPLAY
  "$@" >"$out/$name.log" 2>&1 &
  local app=$!
  sleep 5
  DISPLAY=$d xdotool mousemove 700 150
  mkdir -p "$out/$name"
  for i in $(seq -w 0 23); do
    DISPLAY=$d import -window root "$out/$name/$i.png"
    sleep 0.125
  done
  kill "$app" 2>/dev/null || true
  wait "$app" 2>/dev/null || true
}

capture blitz env CASKADE_PROTO_HOME="$home" CASKADE_PROTO_GALLERY=1 CASKADE_PROTO_WIDTH=1000 \
  CASKADE_PROTO_HEIGHT=700 "$bin"
capture webkit /usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/MiniBrowser --full-screen \
  "file://$out/gallery-page.html"

python3 -I - "$out" <<'EOF'
import json, os, sys
from PIL import Image, ImageChops
out = sys.argv[1]
# Element boxes in gallery.html's layout (32 px padding, 32 px gaps).
regions = {
    "cs-grid-pan + cs-nebula (backdrop)": (40, 330, 960, 680),
    "cs-scan (hero beam)": (40, 40, 584, 200),
    "cs-drift (hero glow)": (192, 72, 432, 232),
    "cs-sweep (card art, hover)": (625, 33, 803, 231),
    "cs-pulse (live dot)": (834, 30, 856, 52),
}
report = {}
for engine in ("blitz", "webkit"):
    frames = [Image.open(f"{out}/{engine}/{n:02d}.png").convert("L") for n in range(24)]
    report[engine] = {}
    for name, box in regions.items():
        crops = [f.crop(box) for f in frames]
        changes = 0
        peak = 0
        for a, b in zip(crops, crops[1:]):
            d = ImageChops.difference(a, b)
            if d.getbbox():
                changes += 1
                peak = max(peak, max(i for i, v in enumerate(d.histogram()) if v))
        report[engine][name] = {"frames_changed": changes, "of": 23, "max_level_change": peak}
json.dump(report, open(f"{out}/gallery.json", "w"), indent=1)
for name in regions:
    b, w = report["blitz"][name], report["webkit"][name]
    print(f"{name:38s} blitz {b['frames_changed']:2d}/23 (Δ{b['max_level_change']:3d})   webkit {w['frames_changed']:2d}/23 (Δ{w['max_level_change']:3d})")
# Contact sheet: first and a middle frame of each engine.
sheet = Image.new("RGB", (2000, 1400))
for col, engine in enumerate(("blitz", "webkit")):
    for row, n in enumerate((0, 12)):
        sheet.paste(Image.open(f"{out}/{engine}/{n:02d}.png").convert("RGB"), (col * 1000, row * 700))
sheet.resize((1000, 700)).save(f"{out}/gallery-sheet.png")
EOF
