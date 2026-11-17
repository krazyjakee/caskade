#!/usr/bin/env bash
# Runs a command under strace and fails if anything it starts addresses a
# network other than loopback (#135): tests reach 127.0.0.1 and ::1 only.
# Build first (`cargo test ... --no-run`): cargo itself is traced too.
#
#   scripts/loopback-only.sh -- cargo test -p caskade-native
#
# Allowed: 127.0.0.0/8, ::1, the unspecified addresses, and netdev's
# 10.254.254.254:1, a UDP connect that finds the default route and sends no
# packet (interface listings, iroh's network monitor). The mDNS tests are
# skipped (CASKADE_TEST_NO_MULTICAST): multicast leaves the machine.
set -uo pipefail

[[ ${1:-} == -- ]] && shift
if [[ $# -eq 0 ]]; then
    echo "usage: $0 -- <command>" >&2
    exit 2
fi
command -v strace >/dev/null || {
    echo "strace is needed" >&2
    exit 2
}

dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
trace=$dir/net.trace

CASKADE_TEST_NO_MULTICAST=1 strace -f -qq -e signal=none \
    -e trace=connect,sendto,sendmsg,sendmmsg -o "$trace" "$@"
status=$?

leaks=$(grep -oE 'inet_addr\("[^"]+"\)|inet_pton\(AF_INET6, "[^"]+"' "$trace" |
    grep -oE '"[^"]+"' | tr -d '"' |
    grep -vE '^(127\.|0\.0\.0\.0$|::1$|::$|::ffff:127\.|10\.254\.254\.254$)' |
    sort | uniq -c)
if [[ -n $leaks ]]; then
    echo "addressed beyond loopback (#135):" >&2
    echo "$leaks" >&2
    grep -E "$(echo "$leaks" | awk '{print $2}' | sed 's/\./\\./g' | paste -sd'|')" "$trace" |
        head -20 >&2
    exit 1
fi
exit "$status"
