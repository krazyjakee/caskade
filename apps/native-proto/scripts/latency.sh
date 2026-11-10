#!/usr/bin/env bash
# Input latency on a private Xvfb: moves the pointer onto a card and off it
# again 20 times each (xdotool, on the private display only; each move restyles
# the card's hover state, which takes one frame) and reports the time from
# sending the event to the end of the next frame, on the monotonic clock.
#
#   latency.sh <prototype built with log-frame-times> <library copy> <out dir>
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
bin=$(realpath "$1") home=$(realpath "$2") out=$3
mkdir -p "$out"
d=:$((520 + RANDOM % 100))
Xvfb "$d" -screen 0 1440x900x24 -nolisten tcp >/dev/null 2>&1 &
xvfb=$!
trap 'kill $xvfb 2>/dev/null' EXIT
sleep 1
export DISPLAY=$d CASKADE_PROTO_HOME=$home CASKADE_PROTO_WIDTH=1440 CASKADE_PROTO_HEIGHT=900
export CASKADE_PROTO_BENCH=idle CASKADE_PROTO_BENCH_SETTLE=2 CASKADE_PROTO_BENCH_SECONDS=40
python3 -I "$here/frames.py" --out "$out" -- "$bin" >/dev/null &
runner=$!
sleep 12
: >"$out/sent.txt"
for _ in $(seq 20); do
  for xy in "400 300" "400 880"; do   # a card's art, then the status bar
    python3 -I -c 'import time; print(time.monotonic())' >>"$out/sent.txt"
    # shellcheck disable=SC2086
    xdotool mousemove $xy
    sleep 0.4
  done
done
wait $runner
python3 -I - "$out" <<'PY'
import json, re, statistics, sys
out = sys.argv[1]
launch = json.load(open(f"{out}/frames.json"))["launch_monotonic"]
frames = [launch + float(t) for t, rest in (l.split(" ", 1) for l in open(f"{out}/app.log"))
          if re.match(r"(vello_hybrid|vello|softbuffer|skia|pixels): ", rest)]
lat = []
for sent in (float(x) for x in open(f"{out}/sent.txt")):
    after = [f for f in frames if f > sent]
    if after and after[0] - sent < 0.39:
        lat.append(1000 * (after[0] - sent))
res = {"events": sum(1 for _ in open(f"{out}/sent.txt")), "answered": len(lat),
       "event_to_frame_end_ms": {"p50": round(statistics.median(lat), 1), "p95": round(sorted(lat)[int(0.95 * (len(lat) - 1))], 1), "max": round(max(lat), 1)} if lat else None}
json.dump(res, open(f"{out}/latency.json", "w"), indent=1)
print(json.dumps(res))
PY
