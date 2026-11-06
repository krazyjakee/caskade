#!/usr/bin/env bash
# Runs crates/caskade-core/tests/proton_real.rs: a real Windows program copied
# between two Caskade cores and started through real umu and Proton (#11),
# its saves going into the prefix and back (#14).
#
#   scripts/proton-real.sh [test name filter] [--keep]
#
# It needs what this PC has from Steam and Caskade: umu-launcher in Caskade's
# data folder, umu's Steam Runtime and a UMU-Proton build. It copies them to a
# scratch home under /tmp/caskade-proton and runs there, on an Xvfb of its own,
# in a namespace with loopback and no internet, so umu never updates anything,
# and your Steam, prefixes and library are never touched. Children such as
# wineserver would hold the heavy lane's lock, so only the build takes it.
set -euo pipefail

scratch=/tmp/caskade-proton
share=$scratch/home/.local/share
filter=proton_real::
keep=
for arg in "$@"; do
  case $arg in
    --keep) keep=1 ;;
    *) filter=$arg ;;
  esac
done

if [ "${CASKADE_PROTON_INSIDE:-}" = 1 ]; then
  ip link set lo up
  if timeout 3 bash -c "</dev/tcp/1.1.1.1/443" 2>/dev/null; then
    echo "reached the internet from a namespace with no uplink" >&2
    exit 9
  fi
  # Bringing loopback up needed capabilities; bwrap refuses to run with them.
  exec setpriv --ambient-caps=-all --inh-caps=-all \
    "$CASKADE_PROTON_BINARY" --ignored --nocapture --test-threads=1 "$filter"
fi

cd "$(dirname "$0")/.."
data=${XDG_DATA_HOME:-$HOME/.local/share}
umu=$(ls -d "$data"/caskade/umu/*/ | tail -1)
proton=$(ls -d "$data"/Steam/compatibilitytools.d/UMU-Proton-* | sort -V | tail -1)
[ -x "$umu/umu/umu-run" ] && [ -d "$data/umu/steamrt3" ] && [ -x "$proton/proton" ] || {
  echo "needs $data/caskade/umu, $data/umu/steamrt3 and a UMU-Proton build" >&2
  exit 1
}

mkdir -p "$share/caskade/umu" "$share/umu" "$share/Steam/compatibilitytools.d" \
  "$scratch/home/.cache" "$scratch/home/.config" "$scratch/tmp" "$scratch/bin"
[ -d "$share/caskade/umu/$(basename "$umu")" ] || cp -a "$umu" "$share/caskade/umu/"
[ -d "$share/umu/steamrt3" ] || cp -a "$data/umu/steamrt3" "$share/umu/"
[ -d "$share/Steam/compatibilitytools.d/$(basename "$proton")" ] ||
  cp -a "$proton" "$share/Steam/compatibilitytools.d/"

rustc --edition 2021 --target x86_64-pc-windows-gnu -O -C strip=symbols \
  -o "$scratch/bin/proof_game.exe" crates/caskade-core/tests/fixtures/saves/proof_game.rs
built=$(flock /tmp/caskade-heavy.lock cargo test -p caskade-core --test it --no-run 2>&1)
binary=$(sed -n 's/.*Executable .*(\(.*\))$/\1/p' <<<"$built" | tail -1)
[ -n "$binary" ] || { echo "$built"; exit 1; }

display=$(mktemp "$scratch/tmp/display.XXXXXX")
Xvfb -displayfd 3 -screen 0 1024x768x24 -nolisten tcp 3>"$display" >/dev/null 2>&1 &
xvfb=$!
finish() {
  kill "$xvfb" 2>/dev/null || true
  # wineservers of the scratch prefixes, and nothing else.
  for pid in $(pgrep -x wineserver || true); do
    if tr '\0' '\n' <"/proc/$pid/environ" 2>/dev/null | grep -q "^WINEPREFIX=$scratch/"; then
      kill "$pid" 2>/dev/null || true
    fi
  done
  [ -n "$keep" ] || rm -rf "$scratch"
}
trap finish EXIT
for _ in $(seq 50); do [ -s "$display" ] && break; sleep 0.1; done
[ -s "$display" ] || { echo "Xvfb didn't start" >&2; exit 1; }

env -i PATH="$PATH" HOME="$scratch/home" TMPDIR="$scratch/tmp" \
  XDG_DATA_HOME="$share" XDG_CACHE_HOME="$scratch/home/.cache" \
  XDG_CONFIG_HOME="$scratch/home/.config" DISPLAY=":$(cat "$display")" \
  RUST_BACKTRACE=1 RUST_LOG="${RUST_LOG:-}" \
  CASKADE_PROTON_SCRATCH="$scratch" CASKADE_PROOF_GAME="$scratch/bin/proof_game.exe" \
  CASKADE_PROTON_INSIDE=1 CASKADE_PROTON_BINARY="$(realpath "$binary")" \
  unshare -Un --keep-caps --map-user="$(id -u)" --map-group="$(id -g)" \
  "$PWD/scripts/proton-real.sh" "$filter"
