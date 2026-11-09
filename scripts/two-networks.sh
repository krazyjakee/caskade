#!/usr/bin/env bash
# Runs the scenarios in crates/caskade-core/tests/two_networks.rs and, for
# shares, crates/caskade-share/tests/two_networks.rs: each device
# is a process in a network namespace of its own, behind a router namespace
# doing NAT, with this namespace as the internet (a bridge, and a real iroh
# relay on it). It needs no root (user namespaces) and touches nothing
# outside. It shows what Linux's network stack does, not what a real router,
# a phone's hotspot or a second network does.
#
#   scripts/two-networks.sh            every scenario and negative control
#   scripts/two-networks.sh t1 o6      only those groups: nat, t1, t2, t3,
#                                      copy, o5, o6, meter
set -euo pipefail

if [ "${1:-}" != --inside ]; then
  cd "$(dirname "$0")/.."
  # it <crate>: builds the crate's test binary and prints its path.
  it() {
    local built binary
    built=$(flock /tmp/caskade-heavy.lock cargo test -p "$1" --test it --no-run 2>&1) \
      || { echo "$built" >&2; return 1; }
    binary=$(sed -n 's/.*Executable .*(\(.*\))$/\1/p' <<<"$built" | tail -1)
    [ -n "$binary" ] || { echo "$built" >&2; return 1; }
    realpath "$binary"
  }
  core=$(it caskade-core)
  share=$(it caskade-share)
  exec unshare -rn "$0" --inside "$core" "$share" "$@"
fi
shift
run=$(mktemp -d)
trap 'kill $(jobs -p) 2>/dev/null || true; rm -rf "$run"' EXIT
# Copies: other work on this checkout may rebuild them during the run. The
# share scenarios are caskade-share's, the rest caskade-core's.
binary=$run/it
share_binary=$run/share-it
cp "$1" "$binary"
cp "$2" "$share_binary"
shift 2

ip link set lo up
ip link add wan0 type bridge mcast_snooping 0
ip addr add 203.0.113.1/24 dev wan0
ip addr add 203.0.113.9/24 dev wan0
ip link set wan0 up

holders=()

# holder: a process holding a new network namespace; its pid in $holder.
hold() {
  unshare -n sleep 100000 &
  holder=$!
  holders+=("$holder")
  until [ "$(readlink /proc/$holder/ns/net)" != "$(readlink /proc/self/ns/net)" ]; do
    sleep 0.01
  done
  nsenter -t "$holder" -n ip link set lo up
}

ns() {
  local pid=$1
  shift
  nsenter -t "$pid" -n "$@"
}

# router <letter> <wan host> <lan prefix> <nat>: sets $router.
# nat: cone      MASQUERADE: one mapping per inside port, keeping the port;
#                only what it sent to answers (address and port)
#      symmetric MASQUERADE --random-fully: a random port per destination
#      fullcone  cone, and everything else from outside goes to the device
router() {
  local name=$1 host=$2 prefix=$3 nat=$4
  hold
  router=$holder
  ip link add "r$name" type veth peer name wan netns "$router"
  ip link set "r$name" master wan0 up
  ns "$router" ip addr add "203.0.113.$host/24" dev wan
  ns "$router" ip link set wan up
  ns "$router" ip link add lan0 type bridge mcast_snooping 0
  ns "$router" ip addr add "$prefix.1/24" dev lan0
  ns "$router" ip link set lan0 up
  ns "$router" sh -c 'echo 1 >/proc/sys/net/ipv4/ip_forward'
  # As a home router: nothing from outside reaches the router itself. A
  # packet it accepted here would leave a conntrack entry the mapping for
  # the device's own traffic then has to avoid.
  ns "$router" iptables -A INPUT -i wan -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT
  ns "$router" iptables -A INPUT -i wan -j DROP
  ns "$router" iptables -P FORWARD DROP
  ns "$router" iptables -A FORWARD -i lan0 -o wan -j ACCEPT
  ns "$router" iptables -A FORWARD -i wan -m conntrack --ctstate ESTABLISHED,RELATED,DNAT -j ACCEPT
  case $nat in
    cone) ns "$router" iptables -t nat -A POSTROUTING -o wan -j MASQUERADE ;;
    symmetric) ns "$router" iptables -t nat -A POSTROUTING -o wan -j MASQUERADE --random-fully ;;
    fullcone)
      ns "$router" iptables -t nat -A POSTROUTING -o wan -j MASQUERADE
      ns "$router" iptables -t nat -A PREROUTING -i wan -j DNAT --to-destination "$prefix.2"
      ;;
    *) echo "no NAT called $nat" >&2; exit 2 ;;
  esac
}

# device <router pid> <prefix> <host>: a namespace on the router's LAN, in $holder.
device() {
  local gateway=$1 prefix=$2 number=$3
  hold
  ip link add "d$number" type veth peer name eth0 netns "$holder"
  ip link set "d$number" netns "$gateway"
  ns "$gateway" ip link set "d$number" master lan0 up
  ns "$holder" ip addr add "$prefix.$number/24" dev eth0
  ns "$holder" ip link set eth0 up
  ns "$holder" ip route add default via "$prefix.1"
}

# on_wan <host>: a device with a public address and no router, in $holder.
on_wan() {
  hold
  ip link add "p$1" type veth peer name eth0 netns "$holder"
  ip link set "p$1" master wan0 up
  ns "$holder" ip addr add "203.0.113.$1/24" dev eth0
  ns "$holder" ip link set eth0 up
}

# start_test <namespace pid> <name> <other> <test>: one device's half; $!.
start_test() {
  local pid=$1 me=$2 other=$3 test=$4 runs=$binary
  case $test in
    share_* | stopping_a_share_*) runs=$share_binary ;;
  esac
  env CASKADE_LAN_ME=$me CASKADE_LAN_OTHER=$other CASKADE_LAN_NOTES=$run/notes \
    "CASKADE_NET_RELAY=${relay_at:-}" "CASKADE_NET_EXPECT=${expect:-}" \
    "CASKADE_NET_PATIENCE=${patience:-}" ${fixed_port:+CASKADE_NET_PORT=$fixed_port} \
    ${only_desk:+CASKADE_NET_ONLY_DESK=1} \
    nsenter -t "$pid" -n timeout 280 "$runs" --exact --ignored --nocapture "two_networks::$test" \
    > >(grep --line-buffered -vE '^(running 1 test|test .* has been running.*|note: run with.*|failures:.*| *two_networks::.*|)$' \
      | sed -u "s/^/  [$me] /" | tee -a "$run/log") 2>&1 &
}

# The relay's traffic, both ways, counted by this namespace's netfilter.
relay_up() {
  mkdir -p "$run/notes"
  iptables -F
  iptables -A INPUT -p tcp -d 203.0.113.1
  iptables -A OUTPUT -p tcp -s 203.0.113.1
  echo 0 >"$run/notes/relay-bytes"
  CASKADE_LAN_ME=Relay CASKADE_LAN_OTHER=none CASKADE_LAN_NOTES=$run/notes \
    timeout 300 "$binary" --exact --ignored --nocapture two_networks::relay >/dev/null 2>&1 &
  relay_pid=$!
  until [ -e "$run/notes/Relay-at" ]; do sleep 0.05; done
  relay_at=$(cat "$run/notes/Relay-at")
  while sleep 0.2; do
    iptables -L -v -x -n 2>/dev/null \
      | awk '$1 ~ /^[0-9]+$/ { sum += $2 } END { print sum + 0 }' >"$run/notes/relay-bytes.new" \
      && mv "$run/notes/relay-bytes.new" "$run/notes/relay-bytes"
  done &
  meter_pid=$!
}

teardown() {
  kill "${relay_pid:-}" "${meter_pid:-}" "${holders[@]}" 2>/dev/null || true
  for pid in "${tests[@]}"; do kill "$pid" 2>/dev/null || true; done
  wait 2>/dev/null || true
  holders=()
  tests=()
  relay_pid=
  meter_pid=
  relay_at=
  ip link del rA 2>/dev/null || true
  ip link del rB 2>/dev/null || true
  ip link del p20 2>/dev/null || true
  rm -rf "$run/notes"
}

tests=()

# await_tests: waits for the devices. The first to fail ends the other.
await_tests() {
  local failed=0 left=${#tests[@]} pid
  while [ "$left" -gt 0 ]; do
    wait -n "${tests[@]}" 2>/dev/null || { failed=1; break; }
    left=$((left - 1))
  done
  for pid in "${tests[@]}"; do kill "$pid" 2>/dev/null || true; done
  wait "${tests[@]}" 2>/dev/null || true
  tests=()
  return "$failed"
}

# Starts <test> on the Desk and the Friend.
both() {
  start_test "$1" Desk Friend "$3"
  tests+=($!)
  start_test "$2" Friend Desk "$3"
  tests+=($!)
}

# two_nats <test> <Desk's NAT> <Friend's NAT>. The Desk's NAT `public` is no
# NAT: a public address of its own.
two_nats() {
  local test=$1 desk friend
  relay_up
  if [ "$2" = public ]; then
    on_wan 20
    desk=$holder
  else
    router A 2 192.168.10 "$2"
    device "$router" 192.168.10 2
    desk=$holder
  fi
  router B 3 192.168.20 "$3"
  local b=$router
  device "$b" 192.168.20 2
  friend=$holder
  if [ -n "${limit:-}" ]; then
    ns "$b" tc qdisc add dev wan root tbf rate "$limit" burst 64kb latency 100ms
  fi
  both "$desk" "$friend" "$test"
  local failed=0
  await_tests || failed=1
  teardown
  return "$failed"
}

# one_lan <test>: two devices on one router. <uplink>: `plugged later` when
# the test says plug, or `none` for a network with no way out. <multicast>:
# `blocked` stops it at the bridge's ports, as client isolation would.
one_lan() {
  local test=$1 uplink=$2 multicast=$3
  relay_up
  router A 2 192.168.10 cone
  local gateway=$router
  ns "$gateway" ip link set wan down
  device "$gateway" 192.168.10 2
  local desk=$holder
  device "$gateway" 192.168.10 3
  local friend=$holder
  if [ "$multicast" = blocked ]; then
    ns "$gateway" bridge link set dev d2 mcast_flood off
    ns "$gateway" bridge link set dev d3 mcast_flood off
  fi
  both "$desk" "$friend" "$test"
  if [ "$uplink" = later ]; then
    (
      until [ -e "$run/notes/Desk-plug" ] && [ -e "$run/notes/Friend-plug" ]; do sleep 0.1; done
      ns "$gateway" ip link set wan up
      echo "  the router's internet is plugged in"
      if [ "${isolate_after:-}" = yes ]; then
        ns "$gateway" bridge link set dev d2 isolated on
        ns "$gateway" bridge link set dev d3 isolated on
        echo "  control: the LAN ports can't reach each other any more"
      fi
    ) &
  fi
  local failed=0
  await_tests || failed=1
  teardown
  return "$failed"
}

# probe <NAT> [<what it's taken for>]
probe() {
  mkdir -p "$run/notes"
  router A 2 192.168.10 "$1"
  device "$router" 192.168.10 2
  local inside=$holder
  CASKADE_LAN_NOTES=$run/notes timeout 60 "$binary" --exact --ignored --nocapture two_networks::nat_echo \
    >/dev/null 2>&1 &
  local echo_pid=$!
  until [ -e "$run/notes/echo-ready" ]; do sleep 0.05; done
  local outcome=0
  CASKADE_NAT=${2:-$1} nsenter -t "$inside" -n timeout 30 "$binary" --exact --ignored --nocapture two_networks::nat_probe \
    >"$run/probe" 2>&1 || outcome=1
  grep -E '^(nat:|assertion)' "$run/probe" | sed 's/^/  /' | tee -a "$run/log" || true
  kill "$echo_pid" 2>/dev/null || true
  teardown
  return "$outcome"
}

# The groups asked for: all of them with none named.
groups=" $* "
wanted() { [ "$groups" = "  " ] || [[ "$groups" == *" $1 "* ]]; }

results=()
wrong=0

# passes <label> <command…>: the command has to succeed.
passes() {
  local label=$1
  shift
  echo "=== $label"
  if "$@"; then
    results+=("pass              $label")
  else
    results+=("FAILED            $label")
    wrong=1
  fi
}

# fails <label> <what it has to say> <command…>: a negative control. It has
# to fail, and for the reason given: a broken network fails anything.
fails() {
  local label=$1 reason=$2
  shift 2
  echo "=== control: $label (has to fail: $reason)"
  : >"$run/log"
  if "$@"; then
    results+=("CONTROL PASSED    $label")
    wrong=1
  elif sleep 0.3; ! grep -qF -- "$reason" "$run/log"; then
    results+=("CONTROL BROKE     $label: never said \"$reason\"")
    wrong=1
  else
    results+=("failed, as it must  $label")
  fi
  echo "  (end of control)"
}

if wanted nat; then
  passes "NAT probe: cone" probe cone
  passes "NAT probe: symmetric" probe symmetric
  passes "NAT probe: fullcone" probe fullcone
  fails "the probe taking a symmetric NAT for a cone" "one mapping for both" \
    probe symmetric cone
  fails "the probe taking a cone for a full cone" "assertion" probe cone fullcone
fi

if wanted meter; then
  passes "the relay meter counts a transfer through the relay" \
    two_nats the_relay_meter_sees_what_passes_through_the_relay symmetric symmetric
fi

if wanted t1; then
  passes "T1 share, cone/cone: arrives directly and plays" \
    two_nats share_arrives_and_plays cone cone
  passes "T1 share, full cone sharer: arrives by a plain dial and plays" \
    two_nats share_arrives_and_plays fullcone cone
  passes "T1 share, symmetric/symmetric: waits, nothing through the relay" \
    two_nats share_waits_when_only_a_relay_reaches symmetric symmetric
  passes "T1 share, symmetric sharer/cone: waits" \
    two_nats share_waits_when_only_a_relay_reaches symmetric cone
  passes "T1 share, cone sharer/symmetric: waits" \
    two_nats share_waits_when_only_a_relay_reaches cone symmetric
  patience=15 fails "a share arriving between symmetric NATs" "left: RelayOnly" \
    two_nats share_arrives_and_plays symmetric symmetric
  fails "a share waiting between cone NATs" "left: Direct" \
    two_nats share_waits_when_only_a_relay_reaches cone cone
fi

if wanted t2; then
  limit=64mbit passes "T2 Stop sharing part-way, cone/cone" \
    two_nats stopping_a_share_fails_the_copy_part_way cone cone
  limit=64mbit expect=no-stop patience=60 passes "T2 control: left alone, the same copy finishes" \
    two_nats stopping_a_share_fails_the_copy_part_way cone cone
fi

if wanted copy; then
  expect=copies passes "copy between paired devices, cone/cone: direct, and it plays" \
    two_nats copy_between_paired_devices cone cone
fi

if wanted t3; then
  expect=waits passes "T3 off, symmetric/symmetric: link relay, the copy waits, nothing moves" \
    two_nats copy_between_paired_devices symmetric symmetric
  expect=offline passes "T3 on, symmetric/symmetric: never relay, offline" \
    two_nats connect_only_directly_never_shows_a_relay symmetric symmetric
  expect=offline passes "T3 on, cone/cone: never relay, offline" \
    two_nats connect_only_directly_never_shows_a_relay cone cone
  expect=offline only_desk=yes passes "T3 on the Desk alone, symmetric/symmetric: never relay, offline" \
    two_nats connect_only_directly_never_shows_a_relay symmetric symmetric
  expect=direct fixed_port=41641 passes "T3 on, public Desk with a fixed port/cone: direct" \
    two_nats connect_only_directly_never_shows_a_relay public cone
  expect=offline fixed_port=41641 fails "T3 on: a Desk that can be dialled isn't offline" \
    "left: Direct" two_nats connect_only_directly_never_shows_a_relay public cone
  expect=waits fails "T3 off: between cone NATs the link isn't relay" \
    "the link is Relay within 30 s" two_nats copy_between_paired_devices cone cone
fi

if wanted o5; then
  passes "O5 Connect only directly on, the internet returns: stays lan" \
    one_lan the_link_stays_lan_when_the_internet_returns later open
  isolate_after=yes fails "O5 with the LAN cut after the internet returns" \
    "a session opens" one_lan the_link_stays_lan_when_the_internet_returns later open
fi

if wanted o6; then
  passes "O6 multicast blocked: a pairing link pairs and syncs, lan" \
    one_lan a_pairing_link_pairs_with_multicast_blocked none blocked
  fails "O6 with multicast passing: they see each other nearby" "seen nearby" \
    one_lan a_pairing_link_pairs_with_multicast_blocked none open
fi

echo
echo "=== results"
printf '%s\n' "${results[@]}"
exit "$wrong"
