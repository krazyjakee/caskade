#!/bin/sh
# Builds the #132 spike backend (plugin.ts) into a component with jco.
# Needs Node 22+ and network access for npx. Usage: build.sh [out.wasm] [backend]
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out=${1:-"$here/plugin.wasm"}
backend=${2:-starlingmonkey}
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
# jco may rewrite the WIT folder it is given, so it only ever sees a copy.
cp -r "$here/../../../wit" "$scratch/wit"
npx --yes @bytecodealliance/jco@1.39.0 componentize "$here/plugin.ts" \
  --backend "$backend" --wit "$scratch/wit" --world-name plugin -o "$out"
