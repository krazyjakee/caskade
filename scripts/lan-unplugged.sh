#!/usr/bin/env bash
# Runs the two-device scenarios in crates/caskade-core/tests/lan_unplugged.rs
# on a real, private network: each device is a process in a network namespace
# of its own, joined by a bridge on a "router" namespace with no uplink. It
# needs no root (user namespaces) and touches nothing outside. It shows what
# the network stack does, not what a real router or Wi-Fi does.
#
#   scripts/lan-unplugged.sh --speed [rate]
#
# instead copies a large game once, in a release build, and prints how long
# each part took (N1 in docs/manual-checks.md). `rate` (such as 1gbit) shapes
# the link with tc; CASKADE_COPY_MIB sizes the game. The speed shows what
# Caskade and this PC's disk and CPU can do over a virtual link. It isn't a
# real network's.
set -euo pipefail

if [ "${1:-}" != --inside ]; then
  cd "$(dirname "$0")/.."
  profile=
  [ "${1:-}" = --speed ] && profile=--release
  built=$(flock /tmp/caskade-heavy.lock cargo test -p caskade-core --test it --no-run $profile 2>&1)
  binary=$(sed -n 's/.*Executable .*(\(.*\))$/\1/p' <<<"$built" | tail -1)
  [ -n "$binary" ] || { echo "$built"; exit 1; }
  exec unshare -rn "$0" --inside "$(realpath "$binary")" "$@"
fi
binary=$2
speed=${3:-}
rate=${4:-}

notes=$(mktemp -d)
trap 'rm -rf "$notes"' EXIT

ip link set lo up
ip link add br0 type bridge mcast_snooping 0
ip addr add 192.168.77.1/24 dev br0
ip link set br0 up

# device <name> <other> <host number> <test>: starts the device, its pid in $!.
device() {
  local gate="$notes/$1.go"
  mkfifo "$gate"
  CASKADE_LAN_ME=$1 CASKADE_LAN_OTHER=$2 CASKADE_LAN_NOTES=$notes \
    CASKADE_LAN_SECONDS=${seconds:-240} \
    unshare -n sh -c '
      read _ <"$1"
      ip link set lo up
      ip addr add "192.168.77.$2/24" dev eth0
      ip link set eth0 up
      ip route add default via 192.168.77.1
      if timeout 3 bash -c "</dev/tcp/1.1.1.1/443" 2>/dev/null; then
        echo "reached the internet from a namespace with no uplink" >&2
        exit 9
      fi
      exec timeout "$CASKADE_LAN_SECONDS" "$3" --exact --ignored --nocapture "$4"
    ' sh "$gate" "$3" "$binary" "$4" &
  local pid=$!
  ip link add "v$3" type veth peer name eth0 netns "$pid"
  ip link set "v$3" master br0 up
  [ -z "$rate" ] || tc qdisc add dev "v$3" root tbf rate "$rate" burst 1mb latency 20ms
  echo go >"$gate"
  return 0
}

# scenario <test> <first device> <second device>
scenario() {
  rm -f "$notes"/*
  echo "=== $1 ($2, $3)"
  device "$2" "$3" 2 "$1"
  local first=$!
  device "$3" "$2" 3 "$1"
  local second=$!
  local failed=0
  wait "$first" || failed=1
  wait "$second" || failed=1
  ip link del v2 2>/dev/null || true
  ip link del v3 2>/dev/null || true
  return "$failed"
}

if [ "$speed" = --speed ]; then
  seconds=1800
  scenario lan_unplugged::copy_speed Desk Laptop
  exit
fi

scenario lan_unplugged::two_devices_pair_sync_and_copy Desk Laptop
scenario new_device::set_up_from_another_with_no_internet Desktop Deck
echo "=== both scenarios passed"
