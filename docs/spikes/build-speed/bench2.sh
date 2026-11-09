#!/bin/bash
# After-split bench: same profile/flags as bench.sh's "llvm" variant (nightly, LLVM).
# usage: bench2.sh <label> <file-to-edit> <crate-to-test>
set -u
WT=/home/krazy/.paseo/worktrees/3c4l1xfr/white-pony
OUT=/mnt/Rusty/caskade-blitz-spike/buildbench
label=$1; EDIT=$WT/$2; TESTPKG=$3
cd "$WT"
cp "$EDIT" "$OUT/edit.orig"
trap 'cp "$OUT/edit.orig" "$EDIT"' EXIT
A=(--profile bench-after --config 'profile.bench-after.inherits="dev"')
step() { local name=$1; shift
  flock /tmp/caskade-heavy.lock bash -c 's=$(date +%s.%N); CARGO_TERM_COLOR=never cargo +nightly "$@" > "'"$OUT/after-$label-$name.log"'" 2>&1; rc=$?; e=$(date +%s.%N); printf "%s %s %.1f rc=%s\n" "'"$label"'" "'"$name"'" "$(echo "$e - $s" | bc)" "$rc"' _ "$@" | tee -a "$OUT/results-after.txt"; }
edit() { cp "$OUT/edit.orig" "$EDIT"; printf '\n#[allow(dead_code)]\nfn bench_marker() -> u32 {\n    %s\n}\n' "$1" >> "$EDIT"; }
step cold-build build "${A[@]}" --timings -p caskade-core -p caskade-desktop
step cold-test test "${A[@]}" --no-run -p "$TESTPKG"
edit $RANDOM; step edit-build build "${A[@]}" --timings -p caskade-core -p caskade-desktop
edit $RANDOM; step edit-test test "${A[@]}" --no-run -p "$TESTPKG"
edit $RANDOM; step edit-build2 build "${A[@]}" -p caskade-core -p caskade-desktop
cp "$OUT/edit.orig" "$EDIT"
