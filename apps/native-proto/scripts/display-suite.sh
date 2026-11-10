#!/usr/bin/env bash
# BLITZ-0 on a real display: idle, spinner, ambient and scroll runs of the
# prototype, each driving itself (no synthetic input) and exiting when done.
# Opens one window at a time on the current display: only the user runs it
# (#120). It refuses a login session's display or bus unless
# CASKADE_REAL_DISPLAY=1 says the user is running it (#122).
#
#   display-suite.sh --bin <dir with proto-hybrid and proto-hybrid-timed> \
#                    --home <library copy> --out <dir> [--refresh 59.95] [--only name]
#
# --home is a copy of a library (data/caskade/library.sqlite and
# cache/caskade/artwork), never the live one: opening it runs migrations.
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
if [ "${CASKADE_REAL_DISPLAY:-}" != 1 ]; then
  "$here/../../../scripts/private-desktop.sh" --guard || {
    echo "display-suite.sh: real-display runs are the user's (#120); set CASKADE_REAL_DISPLAY=1 if this is one" >&2
    exit 1
  }
fi
bin= home= out= refresh=59.95 only=
while [ $# -gt 0 ]; do
  case $1 in
    --bin) bin=$(realpath "$2"); shift 2 ;;
    --home) home=$(realpath "$2"); shift 2 ;;
    --out) out=$2; shift 2 ;;
    --refresh) refresh=$2; shift 2 ;;
    --only) only=$2; shift 2 ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done
[ -n "$bin" ] && [ -n "$home" ] && [ -n "$out" ] || { sed -n 2,11p "$0" >&2; exit 2; }
mkdir -p "$out"
export CASKADE_PROTO_HOME=$home
# Vsynced presents need the window visible on a display that is on (with the
# monitors in DPMS off they come once a second): stay on top, without the focus.
export CASKADE_PROTO_ON_TOP=${CASKADE_PROTO_ON_TOP:-1}

# name, binary, bench kind, seconds, then extra VAR=value settings.
runs=(
  "idle proto-hybrid idle 60"
  "spinner proto-hybrid spinner 60"
  "ambient proto-hybrid idle 60 CASKADE_PROTO_AMBIENT=1"
  "spinner-timed proto-hybrid-timed spinner 20"
  "ambient-timed proto-hybrid-timed idle 20 CASKADE_PROTO_AMBIENT=1"
  "scroll-1000 proto-hybrid-timed scroll 30 CASKADE_PROTO_BENCH_SPEED=1000"
  "scroll-3000 proto-hybrid-timed scroll 30 CASKADE_PROTO_BENCH_SPEED=3000"
  "scroll-3000-spinner proto-hybrid-timed scroll 30 CASKADE_PROTO_BENCH_SPEED=3000 CASKADE_PROTO_SPINNER=1"
)
for run in "${runs[@]}"; do
  read -r name exe kind seconds extra <<<"$run"
  [ -n "$only" ] && [ "$only" != "$name" ] && continue
  [ -e "$out/$name/frames.json" ] && { echo "$name: done already"; continue; }
  echo "== $name"
  # shellcheck disable=SC2086
  env CASKADE_PROTO_BENCH="$kind" CASKADE_PROTO_BENCH_SECONDS="$seconds" CASKADE_PROTO_BENCH_SETTLE=10 \
    $extra python3 -I "$here/frames.py" --out "$out/$name" --refresh "$refresh" -- "$bin/$exe" \
    | python3 -I -c 'import json,sys; d=json.load(sys.stdin); f=d.get("frames") or {}; print(json.dumps({"cpu_%": d.get("cpu_percent_of_one_core"), "frames/s": f.get("per_s"), "interval_ms": f.get("interval_ms"), "late_%": f.get("late_percent"), "first_frame_s": d.get("first_frame_s"), "mem": d.get("memory_at_end_mb")}))'
done
