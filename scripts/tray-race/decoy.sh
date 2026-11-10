#!/usr/bin/env bash
# Runs a command as if on a developer's desktop and checks it leaves that
# desktop alone (#122): a stand-in session (scripts/private-desktop.sh with
# Cinnamon's real tray watcher and an applet's monitor, session.py) is the
# command's DISPLAY and DBUS_SESSION_BUS_ADDRESS, and scripts/desktop-watch.sh
# fails if a tray icon, top-level window or notification reaches it. The
# real desktop is never involved.
#
#   scripts/tray-race/decoy.sh -- <command> [args…]
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
[ "${1:-}" != -- ] || shift
if [ -z "${CASKADE_PRIVATE_DESKTOP:-}" ]; then
  exec "$here/../private-desktop.sh" -- "$0" -- "$@"
fi
python3 -I "$here/session.py" &
session=$!
trap 'kill -TERM "$session" 2>/dev/null || true; wait "$session" 2>/dev/null || true' EXIT
for _ in $(seq 50); do
  dbus-send --session --print-reply --dest=org.freedesktop.DBus / org.freedesktop.DBus.NameHasOwner \
    string:org.kde.StatusNotifierWatcher 2>/dev/null | grep -q true && break
  sleep 0.1
done
# The command sees this desktop as the session's, as a developer's shell would.
"$here/../desktop-watch.sh" -- env -u CASKADE_PRIVATE_DESKTOP "$@"
