#!/usr/bin/env bash
# The library's scroll benchmark (#94): the native app, in fixtures mode with
# 10,000 games, scrolls its virtual grid and list top to bottom a step per
# frame, on a desktop of its own (scripts/private-desktop.sh) with a profile
# of its own. Prints each layout's report: frame times (p50, p95, max, in
# ms), the most games mounted at once, and how long it took.
#
#   apps/native/scripts/library-scroll.sh [--here] [games]
#
# Builds caskade-native in release with the `bench` feature first. On Xvfb
# the frames are the CPU renderer's, not a display's refresh rate: that's a
# real-machine check (#120), which `--here` runs on this session's own
# display. That's for a person at the machine; agents never use it.
set -euo pipefail
repo=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
here=false
if [ "${1:-}" = "--here" ]; then
  here=true
  shift
fi
games=${1:-10000}
cd "$repo"
flock /tmp/caskade-heavy.lock env CARGO_INCREMENTAL=0 \
  cargo build -q -p caskade-native --release --features bench
target=$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
binary=$target/release/caskade-native
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
for layout in grid list; do
  home=$work/$layout
  mkdir -p "$home"
  if $here; then desktop=(); else desktop=(scripts/private-desktop.sh --size 1440x1000 --); fi
  "${desktop[@]}" env \
    HOME="$home" XDG_DATA_HOME="$home/data" XDG_CONFIG_HOME="$home/config" \
    XDG_CACHE_HOME="$home/cache" XDG_STATE_HOME="$home/state" \
    NO_AT_BRIDGE=1 GTK_A11Y=none RUST_LOG=warn \
    CASKADE_FIXTURES=1 CASKADE_FIXTURE_COUNT="$games" \
    CASKADE_LIBRARY_BENCH="$work/$layout.json" CASKADE_LIBRARY_BENCH_LAYOUT="$layout" \
    timeout 300 "$binary" >"$work/$layout.log" 2>&1 || true
  if [ -s "$work/$layout.json" ]; then
    cat "$work/$layout.json"
    echo
  else
    echo "$layout: no report; the app's log:" >&2
    tail -20 "$work/$layout.log" >&2
    exit 1
  fi
done
