#!/usr/bin/env bash
# Idle RAM and CPU benchmark (scripts/idle-bench/README.md): runs a built app
# against a seeded synthetic library in a sandbox of its own (user and network
# namespace, Xvfb, a private D-Bus with a throwaway keyring, no network) and
# samples every app process.
#
#   scripts/idle-bench/run.sh --app <binary> --seed <seed_library binary> --out <dir>
#                             [--label <text>] [--games 8400] [--warmup 300]
#                             [--duration 1800] [--interval 10]
#                             [--exe 'caskade|WebKit\w*Process'] [--size 1600x900]
#
# Needs no root and touches nothing outside --out. Stops only processes in its
# own namespace, never any by name.
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)

if [ "${1:-}" != --inside ]; then
  app= seed= out= label= games=8400 warmup=300 duration=1800 interval=10 size=1600x900
  exe='caskade|WebKit\w*Process'
  while [ $# -gt 0 ]; do
    case $1 in
      --app) app=$(realpath "$2"); shift 2 ;;
      --seed) seed=$(realpath "$2"); shift 2 ;;
      --out) out=$2; shift 2 ;;
      --label) label=$2; shift 2 ;;
      --games) games=$2; shift 2 ;;
      --warmup) warmup=$2; shift 2 ;;
      --duration) duration=$2; shift 2 ;;
      --interval) interval=$2; shift 2 ;;
      --exe) exe=$2; shift 2 ;;
      --size) size=$2; shift 2 ;;
      *) echo "unknown option $1" >&2; exit 2 ;;
    esac
  done
  [ -x "$app" ] && [ -x "$seed" ] && [ -n "$out" ] || { sed -n 2,13p "$0" >&2; exit 2; }
  [ -e "$out" ] && { echo "$out exists: pick a new --out" >&2; exit 2; }
  mkdir -p "$out"
  out=$(realpath "$out")
  meta=$(python3 -I - "$app" "$label" "$games" "$size" <<'EOF'
import hashlib, json, os, platform, sys
app, label, games, size = sys.argv[1:]
cpu = next((l.split(":", 1)[1].strip() for l in open("/proc/cpuinfo") if l.startswith("model name")), "?")
mem = next((int(l.split()[1]) // 1024 for l in open("/proc/meminfo") if l.startswith("MemTotal")), 0)
gpu = next((l.strip() for l in os.popen("lspci 2>/dev/null | grep -iE 'vga|3d'")), "?")
print(json.dumps({
    "label": label, "app": os.path.basename(app),
    "app_sha256": hashlib.sha256(open(app, "rb").read()).hexdigest(),
    "app_bytes": os.path.getsize(app), "games": int(games), "window": size,
    "display": "Xvfb (software GL)", "network": "none (own namespace)",
    "kernel": platform.release(), "cpu": cpu, "cores": os.cpu_count(), "ram_mb": mem, "gpu": gpu,
}))
EOF
)
  echo "$meta" > "$out/meta.json"
  exec unshare -rn "$0" --inside "$app" "$seed" "$out" "$games" "$warmup" "$duration" "$interval" "$exe" "$size" "$meta"
fi

shift
app=$1 seed=$2 out=$3 games=$4 warmup=$5 duration=$6 interval=$7 exe=$8 size=$9 meta=${10}
netns=$(readlink /proc/self/ns/net)

# Stops every process in this namespace but this script.
stop_all() {
  for proc in /proc/[0-9]*; do
    pid=${proc#/proc/}
    [ "$pid" = $$ ] && continue
    [ "$(readlink "$proc/ns/net" 2>/dev/null)" = "$netns" ] && kill "$pid" 2>/dev/null || true
  done
}
trap stop_all EXIT

ip link set lo up
export HOME=$out/home
export XDG_DATA_HOME=$HOME/data XDG_CONFIG_HOME=$HOME/config XDG_CACHE_HOME=$HOME/cache XDG_STATE_HOME=$HOME/state
mkdir -p "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_STATE_HOME"
unset WAYLAND_DISPLAY APPDIR APPIMAGE ARGV0 OWD
display=:$((150 + RANDOM % 100))
Xvfb "$display" -screen 0 "${size}x24" -nolisten tcp >"$out/xvfb.log" 2>&1 &
export DISPLAY=$display
for _ in $(seq 50); do [ -S "/tmp/.X11-unix/X${display#:}" ] && break; sleep 0.1; done

echo "seeding $games games"
"$seed" "$games" >"$out/seed.log" 2>&1

export IDLE_BENCH_APP=$app IDLE_BENCH_OUT=$out IDLE_BENCH_HERE=$here
export IDLE_BENCH_ARGS="--out $out --warmup $warmup --duration $duration --interval $interval"
export IDLE_BENCH_EXE=$exe IDLE_BENCH_META=$meta
dbus-run-session -- bash -c '
  set -euo pipefail
  eval "$(printf idle-bench | gnome-keyring-daemon --unlock --components=secrets 2>/dev/null)"
  gnome-keyring-daemon --start --components=secrets >/dev/null 2>&1
  for _ in $(seq 50); do
    dbus-send --session --print-reply --dest=org.freedesktop.DBus / org.freedesktop.DBus.NameHasOwner \
      string:org.freedesktop.secrets 2>/dev/null | grep -q true && break
    sleep 0.2
  done
  "$IDLE_BENCH_APP" >"$IDLE_BENCH_OUT/app.log" 2>&1 &
  echo "app started; warm-up, then measuring"
  python3 -I "$IDLE_BENCH_HERE/sample.py" $IDLE_BENCH_ARGS --exe "$IDLE_BENCH_EXE" --meta "$IDLE_BENCH_META"
'
