#!/bin/sh
# Generated synthetic recorder fallback. No host environment dump or eval.
set -eu
: "${CASKADE_FAKE_RECORD:?CASKADE_FAKE_RECORD is required}"
record=$CASKADE_FAKE_RECORD
mkdir -p "$(dirname "$record")"
: > "$record.argv"
for arg do printf '%s\000' "$arg" >> "$record.argv"; done
printf '%s\000' "$(pwd -P)" > "$record.cwd"
: > "$record.env"
if [ "${CASKADE_FAKE_MARKER+x}" = x ]; then printf '%s\000%s\000' CASKADE_FAKE_MARKER "$CASKADE_FAKE_MARKER" >> "$record.env"; fi
if [ "${CASKADE_FAKE_PHASE+x}" = x ]; then printf '%s\000%s\000' CASKADE_FAKE_PHASE "$CASKADE_FAKE_PHASE" >> "$record.env"; fi
if [ "${CASKADE_FAKE_CORE+x}" = x ]; then printf '%s\000%s\000' CASKADE_FAKE_CORE "$CASKADE_FAKE_CORE" >> "$record.env"; fi
printf '%s' "$$" > "$record.pid"
if [ -n "${CASKADE_FAKE_FORK:-}" ]; then
    mode=$CASKADE_FAKE_FORK
    case "$mode" in wait|detach) ;; *) exit 64 ;; esac
    unset CASKADE_FAKE_FORK
    if [ -n "${CASKADE_FAKE_CHILD_PROGRAM:-}" ]; then
        # A differently named copy, run directly so its process name is its own.
        program=$CASKADE_FAKE_CHILD_PROGRAM
        unset CASKADE_FAKE_CHILD_PROGRAM
        CASKADE_FAKE_RECORD="$record.child" CASKADE_FAKE_PHASE=child CASKADE_FAKE_SLEEP_MS="${CASKADE_FAKE_CHILD_SLEEP_MS:-0}" "$program" "$@" &
    else
        CASKADE_FAKE_RECORD="$record.child" CASKADE_FAKE_PHASE=child CASKADE_FAKE_SLEEP_MS="${CASKADE_FAKE_CHILD_SLEEP_MS:-0}" /bin/sh "$0" "$@" &
    fi
    child=$!
    if [ "$mode" = wait ]; then wait "$child"; fi
fi
milliseconds=${CASKADE_FAKE_SLEEP_MS:-0}
case "$milliseconds" in *[!0-9]*|'') exit 64 ;; esac
if [ "$milliseconds" -gt 30000 ]; then exit 64; fi
sleep "$((milliseconds / 1000)).$(printf '%03d' "$((milliseconds % 1000))")"
exit "${CASKADE_FAKE_EXIT_CODE:-0}"
