#!/usr/bin/env bash
# Watches a desktop session while a command runs, and fails if the command
# added a tray icon, a top-level window or a notification to it (#122). It
# only observes the session: it reads the StatusNotifierWatcher's
# RegisteredStatusNotifierItems and the root window's children, and listens
# for Notify calls (dbus-monitor); it starts nothing there.
#
#   scripts/desktop-watch.sh [--display :0] [--bus <address>] -- <command> [args…]
#
# The session is this shell's DISPLAY and DBUS_SESSION_BUS_ADDRESS unless
# given. The command runs with the environment it would have anyway; this
# script only compares what the session shows before, during (every 0.2 s)
# and after.
set -euo pipefail

display=${DISPLAY:-} bus=${DBUS_SESSION_BUS_ADDRESS:-}
while [ $# -gt 0 ]; do
  case "$1" in
    --display) display=$2; shift 2 ;;
    --bus) bus=$2; shift 2 ;;
    --) shift; break ;;
    *) break ;;
  esac
done
[ $# -gt 0 ] || { echo "usage: $0 [--display D] [--bus ADDRESS] -- <command> [args…]" >&2; exit 2; }

work=$(mktemp -d "${TMPDIR:-/tmp}/caskade-desktop-watch-XXXXXX")
items() { # the watcher's registered icons, one per line
  [ -n "$bus" ] || return 0
  DBUS_SESSION_BUS_ADDRESS=$bus dbus-send --session --print-reply=literal \
    --dest=org.kde.StatusNotifierWatcher /StatusNotifierWatcher \
    org.freedesktop.DBus.Properties.Get string:org.kde.StatusNotifierWatcher \
    string:RegisteredStatusNotifierItems 2>/dev/null | tr ' ' '\n' | grep -E '^[:a-zA-Z]' | grep -v '^array$' || true
}
windows() { # top-level windows, by id and name
  [ -n "$display" ] || return 0
  xwininfo -display "$display" -root -children 2>/dev/null | grep -E '^ +0x' | awk '{print $1}' || true
}
items | sort -u >"$work/items.before"
windows | sort -u >"$work/windows.before"
monitor_pid=
if [ -n "$bus" ]; then
  DBUS_SESSION_BUS_ADDRESS=$bus dbus-monitor --session \
    "type='method_call',interface='org.freedesktop.Notifications',member='Notify'" \
    >"$work/notify.log" 2>/dev/null &
  monitor_pid=$!
fi
: >"$work/items.during" >"$work/windows.during"
(
  while :; do
    items >>"$work/items.during"
    windows >>"$work/windows.during"
    sleep 0.2
  done
) &
poller=$!
status=0
"$@" || status=$?
kill "$poller" 2>/dev/null || true
wait "$poller" 2>/dev/null || true
items >>"$work/items.during"
windows >>"$work/windows.during"
[ -z "$monitor_pid" ] || { kill "$monitor_pid" 2>/dev/null || true; wait "$monitor_pid" 2>/dev/null || true; }

new_items=$(sort -u "$work/items.during" | comm -13 "$work/items.before" -)
new_windows=$(sort -u "$work/windows.during" | comm -13 "$work/windows.before" -)
notifications=$(grep -c 'member=Notify' "$work/notify.log" 2>/dev/null || true)
echo "desktop-watch: display ${display:-none}, bus ${bus:-none}; command exited $status"
echo "  tray icons added: $(printf '%s' "$new_items" | grep -c . || true)"
[ -z "$new_items" ] || printf '    %s\n' $new_items
echo "  top-level windows added: $(printf '%s' "$new_windows" | grep -c . || true)"
echo "  notifications sent: ${notifications:-0}"
rm -rf "$work"
if [ -n "$new_items" ] || [ -n "$new_windows" ] || [ "${notifications:-0}" != 0 ]; then
  echo "desktop-watch: the command reached the watched desktop" >&2
  exit 1
fi
exit "$status"
