#!/usr/bin/env bash
# The tray quit race against Cinnamon's real StatusNotifierWatcher (#122):
# race.py on a private desktop (scripts/private-desktop.sh: its own Xvfb and
# session bus), never the developer's. Needs libxapp1 (xapp-sn-watcher) and
# gir1.2-xapp-1.0.
#
#   scripts/tray-race/run.sh <caskade or caskade-native binary> [--runs 20]
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
if [ -z "${CASKADE_PRIVATE_DESKTOP:-}" ]; then
  exec "$here/../private-desktop.sh" -- "$0" "$@"
fi
app=$(realpath "$1")
shift
work=$(mktemp -d "${TMPDIR:-/tmp}/caskade-tray-race-XXXXXX")
trap 'rm -rf "$work/home"' EXIT
mkdir -p "$work/home"
echo "tray-race: $app on $DISPLAY, logs in $work"
python3 -I "$here/race.py" --app "$app" --home "$work/home" --log "$work/watcher.log" "$@"
