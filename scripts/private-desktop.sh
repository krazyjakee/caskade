#!/usr/bin/env bash
# Runs a command on a desktop of its own (#122): its own Xvfb display, its
# own D-Bus session bus (no service folders: nothing starts on it by
# activation) and its own XDG_RUNTIME_DIR. DISPLAY, DBUS_SESSION_BUS_ADDRESS
# and XDG_RUNTIME_DIR are set here, WAYLAND_DISPLAY is unset: nothing is
# inherited, so the command's windows, tray icons and notifications can't
# reach the developer's desktop (a test's tray icon vanishing crashed
# Cinnamon four times on 10 Nov 2026). The Rust tests' equivalent is the
# caskade-test-desktop crate.
#
#   scripts/private-desktop.sh [--size 1920x1080] -- <command> [args…]
#   scripts/private-desktop.sh --guard
#
# The command finds CASKADE_PRIVATE_DESKTOP set (a script re-runs itself
# under this unless it is). --guard only checks this shell's own DISPLAY, WAYLAND_DISPLAY and
# DBUS_SESSION_BUS_ADDRESS: it exits 1, saying why, when any is a login
# session's. Scripts that are handed a display or bus call it first.
# Stops only the Xvfb and dbus-daemon it started, by PID.
set -euo pipefail

size=1920x1080
guard_only=0
while [ $# -gt 0 ]; do
  case "$1" in
    --size) size=$2; shift 2 ;;
    --guard) guard_only=1; shift ;;
    --) shift; break ;;
    *) break ;;
  esac
done

# The login sessions' displays and bus sockets, read without starting
# anything: loginctl's sessions, this user's bus, and what the user's
# service manager hands out.
login_displays() {
  local id
  for id in $(loginctl list-sessions --no-legend 2>/dev/null | awk '{print $1}'); do
    loginctl show-session "$id" -p Display --value 2>/dev/null || true
  done
  systemctl --user show-environment 2>/dev/null | sed -n 's/^\(DISPLAY\|WAYLAND_DISPLAY\)=//p'
  ls "/run/user/$(id -u)" 2>/dev/null | grep -E '^wayland-[0-9]+$' || true
}
login_buses() {
  echo "/run/user/$(id -u)/bus"
  systemctl --user show-environment 2>/dev/null \
    | sed -n 's/^DBUS_SESSION_BUS_ADDRESS=//p' | tr ';,' '\n\n' | sed -n 's/^\(unix:\)\{0,1\}path=//p'
}
norm_display() { # :0.0 → :0, unix/:0 → :0
  local d=${1##*/}
  d=${d#unix}
  case "$d" in *.[0-9]*) d=${d%.*} ;; esac
  printf '%s' "$d"
}

# Refuses ($1 display, $2 bus) when a login session uses either.
guard() {
  local display=${1:-} bus=${2:-} problems=() login path
  if [ -n "$display" ]; then
    while read -r login; do
      [ -n "$login" ] || continue
      if [ "$(norm_display "$login")" = "$(norm_display "$display")" ]; then
        problems+=("the display $display is a login session's: run on a private one (scripts/private-desktop.sh)")
        break
      fi
    done < <(login_displays)
  fi
  if [ -n "$bus" ]; then
    path=$(printf '%s' "$bus" | tr ';,' '\n\n' | sed -n 's/^\(unix:\)\{0,1\}path=//p' | head -1)
    if [ -z "$path" ]; then
      problems+=("the bus $bus isn't a socket path this guard can check: start a private bus")
    elif grep -qxF "$path" <<<"$(login_buses)"; then
      problems+=("the bus $bus is a login session's: its tray and notifications are the developer's desktop; run on a private bus")
    fi
  fi
  if [ ${#problems[@]} -gt 0 ]; then
    printf 'private-desktop: refused: %s\n' "${problems[@]}" >&2
    return 1
  fi
}

if [ "$guard_only" = 1 ]; then
  guard "${DISPLAY:-}" "${DBUS_SESSION_BUS_ADDRESS:-}"
  guard "${WAYLAND_DISPLAY:-}" ""
  exit 0
fi
[ $# -gt 0 ] || { echo "usage: $0 [--size WxH] -- <command> [args…]" >&2; exit 2; }

dir=$(mktemp -d "${TMPDIR:-/tmp}/caskade-desktop-XXXXXX")
runtime=$dir/runtime
mkdir -m 700 "$runtime"
xvfb_pid= dbus_pid=
cleanup() {
  local pid
  for pid in $dbus_pid $xvfb_pid; do
    kill -TERM "$pid" 2>/dev/null || true
  done
  for pid in $dbus_pid $xvfb_pid; do
    wait "$pid" 2>/dev/null || true
  done
  rm -rf "$dir"
}
trap cleanup EXIT INT TERM

# Xvfb picks a free display itself and says which.
exec 3< <(Xvfb -displayfd 1 -nolisten tcp -noreset -screen 0 "${size}x24" 2>"$dir/xvfb.log")
xvfb_pid=$!
read -r -t 15 number <&3 || { echo "private-desktop: Xvfb didn't start (see $dir/xvfb.log)" >&2; exit 1; }
display=:$number

cat >"$dir/bus.conf" <<EOF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <keep_umask/>
  <listen>unix:dir=$runtime</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
EOF
exec 4< <(dbus-daemon --config-file="$dir/bus.conf" --nofork --print-address 2>"$dir/dbus.log")
dbus_pid=$!
read -r -t 15 bus <&4 || { echo "private-desktop: dbus-daemon didn't start (see $dir/dbus.log)" >&2; exit 1; }

guard "$display" "$bus"
unset WAYLAND_DISPLAY
export DISPLAY=$display DBUS_SESSION_BUS_ADDRESS=$bus XDG_RUNTIME_DIR=$runtime NO_AT_BRIDGE=1 GTK_A11Y=none CASKADE_PRIVATE_DESKTOP=$dir
unset XAUTHORITY
status=0
"$@" || status=$?
exit "$status"
