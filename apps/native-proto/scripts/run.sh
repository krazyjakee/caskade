#!/usr/bin/env bash
# BLITZ-0 measurement runner: starts an app (the webview build or the native
# prototype) on a fresh copy of a library and samples every process it runs
# with sample.py. Touches nothing outside --out and stops only what it started.
#
#   run.sh --app <binary> --pristine <library copy> --out <dir> [--label text]
#          [--display xvfb|current] [--size 1600x900] [--settle 60] [--duration 120]
#          [--interval 5] [--click 'X,Y;X,Y'] [--exe regex] [--startup seconds]
#          [--env K=V]... [-- app args]
#
# --pristine holds data/caskade, config/caskade and cache/caskade (artwork
# included); each run gets its own copy (artwork hard-linked, so --out must
# be on the same file system), deleted when the run ends.
#
# xvfb (default): a user and network namespace (no network), a private Xvfb
# and a private D-Bus with a throwaway keyring, so the live desktop and its
# keychain are never touched. current: the session's own display (for GPU and
# vsync numbers); still no network namespace changes, no input is sent.
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)

if [ "${1:-}" != --inside ]; then
  app= pristine= out= label= display=xvfb size=1600x900 settle=60 duration=120 interval=5 click= exe= startup=0
  envs=()
  while [ $# -gt 0 ]; do
    case $1 in
      --app) app=$(realpath "$2"); shift 2 ;;
      --pristine) pristine=$(realpath "$2"); shift 2 ;;
      --out) out=$2; shift 2 ;;
      --label) label=$2; shift 2 ;;
      --display) display=$2; shift 2 ;;
      --size) size=$2; shift 2 ;;
      --settle) settle=$2; shift 2 ;;
      --duration) duration=$2; shift 2 ;;
      --interval) interval=$2; shift 2 ;;
      --click) click=$2; shift 2 ;;
      --exe) exe=$2; shift 2 ;;
      --startup) startup=$2; shift 2 ;;
      --env) envs+=("$2"); shift 2 ;;
      --) shift; break ;;
      *) echo "unknown option $1" >&2; exit 2 ;;
    esac
  done
  [ -x "$app" ] && [ -d "$pristine/data/caskade" ] && [ -n "$out" ] || { sed -n 2,17p "$0" >&2; exit 2; }
  [ -e "$out" ] && { echo "$out exists: pick a new --out" >&2; exit 2; }
  mkdir -p "$out/home"
  out=$(realpath "$out")
  # A fresh library per run: the apps migrate and write to it.
  for kind in data config cache; do mkdir -p "$out/home/$kind"; done
  cp -a "$pristine/data/caskade" "$out/home/data/"
  cp -a "$pristine/config/caskade" "$out/home/config/" 2>/dev/null || true
  rsync -a --exclude artwork "$pristine/cache/caskade/" "$out/home/cache/caskade/"
  cp -al "$pristine/cache/caskade/artwork" "$out/home/cache/caskade/artwork"
  mkdir -p "$out/home/state"
  printf '%s\n' "${envs[@]+"${envs[@]}"}" >"$out/env.txt"
  python3 -I - "$app" "$label" "$display" "$size" "$settle" "$duration" "$interval" "$click" >"$out/meta.json" <<'EOF'
import hashlib, json, os, platform, sys
app, label, display, size, settle, duration, interval, click = sys.argv[1:]
cpu = next((l.split(":", 1)[1].strip() for l in open("/proc/cpuinfo") if l.startswith("model name")), "?")
mem = next((int(l.split()[1]) // 1024 for l in open("/proc/meminfo") if l.startswith("MemTotal")), 0)
gpu = [l.strip() for l in os.popen("lspci 2>/dev/null | grep -iE 'vga|3d'")]
print(json.dumps({"label": label, "app": app, "app_bytes": os.path.getsize(app),
  "app_sha256": hashlib.sha256(open(app, "rb").read()).hexdigest(), "display": display,
  "window": size, "settle_s": float(settle), "duration_s": float(duration), "interval_s": float(interval),
  "click": click, "kernel": platform.release(), "cpu": cpu, "cores": os.cpu_count(), "ram_mb": mem, "gpu": gpu}, indent=1))
EOF
  args=(--inside "$app" "$out" "$display" "$size" "$settle" "$duration" "$interval" "$click" "$exe" "$startup" "$@")
  if [ "$display" = xvfb ]; then
    exec unshare -rn "$0" "${args[@]}"
  fi
  exec "$0" "${args[@]}"
fi

shift
app=$1 out=$2 display=$3 size=$4 settle=$5 duration=$6 interval=$7 click=$8 exe=$9 startup=${10}
shift 10
# The run's library copy goes when the run ends; its logs and results stay.
trap 'rm -rf "$out/home"' EXIT
export HOME=$out/home XDG_DATA_HOME=$out/home/data XDG_CONFIG_HOME=$out/home/config
export XDG_CACHE_HOME=$out/home/cache XDG_STATE_HOME=$out/home/state
export CASKADE_PROTO_HOME=$out/home
unset WAYLAND_DISPLAY APPDIR APPIMAGE ARGV0 OWD
while IFS= read -r kv; do [ -n "$kv" ] && export "${kv?}"; done <"$out/env.txt"

sampler=(python3 -I "$here/sample.py" --out "$out" --settle "$settle" --duration "$duration"
  --interval "$interval" --click "$click" --exe "$exe" --startup "$startup")
if [ "$display" = xvfb ]; then
  ip link set lo up
  d=:$((300 + RANDOM % 200))
  Xvfb "$d" -screen 0 "${size}x24" -nolisten tcp >"$out/xvfb.log" 2>&1 &
  export DISPLAY=$d
  for _ in $(seq 50); do [ -S "/tmp/.X11-unix/X${d#:}" ] && break; sleep 0.1; done
  export PROTO_APP=$app PROTO_OUT=$out
  export PROTO_ARGS="$*"
  # shellcheck disable=SC2016
  dbus-run-session -- bash -c '
    eval "$(printf blitz0 | gnome-keyring-daemon --unlock --components=secrets 2>/dev/null)"
    gnome-keyring-daemon --start --components=secrets >/dev/null 2>&1
    for _ in $(seq 50); do
      dbus-send --session --print-reply --dest=org.freedesktop.DBus / org.freedesktop.DBus.NameHasOwner \
        string:org.freedesktop.secrets 2>/dev/null | grep -q true && break
      sleep 0.2
    done
    "$PROTO_APP" $PROTO_ARGS >"$PROTO_OUT/app.log" 2>&1 &
    exec "$@" --pid $!
  ' bash "${sampler[@]}"
  status=$?
  kill %1 2>/dev/null || true
  exit $status
else
  "$app" "$@" >"$out/app.log" 2>&1 &
  "${sampler[@]}" --pid $!
fi
