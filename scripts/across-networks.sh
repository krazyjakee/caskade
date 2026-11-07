#!/usr/bin/env bash
# T1 and T2 between two networks that really are two (#17): this PC behind its
# router, and a GitHub runner in a data centre, with n0's relays and lookup.
#
#   scripts/across-networks.sh [ubuntu-24.04|macos-latest]
#
# It starts .github/workflows/across-networks.yml, which shares a game from the
# runner and uploads the links, and runs this PC's half of
# crates/caskade-core/tests/across_networks.rs. The links hold only the
# runner's addresses, so this PC's address stays out of a public log.
set -euo pipefail
cd "$(dirname "$0")/.."

runner=${1:-ubuntu-24.04}
built=$(flock /tmp/caskade-heavy.lock cargo test -p caskade-core --test it --no-run 2>&1) \
  || { echo "$built"; exit 1; }
binary=$(sed -n 's/.*Executable .*(\(.*\))$/\1/p' <<<"$built" | tail -1)
[ -n "$binary" ] || { echo "$built"; exit 1; }

notes=$(mktemp -d)
trap 'rm -rf "$notes"' EXIT
cp "$binary" "$notes/it"

since=$(date -u +%Y-%m-%dT%H:%M:%SZ)
gh workflow run across-networks.yml -f runner="$runner"
run=
until [ -n "$run" ]; do
  sleep 5
  run=$(gh run list --workflow across-networks.yml --event workflow_dispatch \
    --created ">=$since" --limit 1 --json databaseId --jq '.[0].databaseId // empty')
done
echo "run $run"

until gh run download "$run" --name links --dir "$notes/links" 2>/dev/null; do
  state=$(gh run view "$run" --json status --jq .status)
  [ "$state" != completed ] || { echo "the run ended with no links" >&2; exit 1; }
  sleep 15
done

export CASKADE_SHARE_LINKS=$notes/links/links RUST_LOG=${RUST_LOG:-}
"$notes/it" --ignored --exact --nocapture across_networks::receives_and_plays
"$notes/it" --ignored --exact --nocapture across_networks::a_stop_fails_the_copy
gh run watch "$run" --exit-status
