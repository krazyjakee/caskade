#!/bin/bash
# Build-speed bench for Caskade: LLVM vs Cranelift vs mold, each in its own cargo
# profile so artifacts land in /mnt/Rusty/cargo-target/<profile>/ and never
# overwrite anyone's dev artifacts. Every cargo step takes the shared heavy lock.
# usage: bench.sh <variant>...   (variants: llvm cl mold)
set -u
WT=/home/krazy/.paseo/worktrees/3c4l1xfr/white-pony
OUT=/mnt/Rusty/caskade-blitz-spike/buildbench
MOLD_BIN=/mnt/Rusty/caskade-blitz-spike/tools/mold-dl/mold-3.0.0-x86_64-linux/bin
EDIT=$WT/crates/caskade-core/src/sessions/sqlite.rs
cd "$WT"
cp "$EDIT" "$OUT/sqlite.rs.orig"
trap 'cp "$OUT/sqlite.rs.orig" "$EDIT"' EXIT

set_args() {
  case $1 in
    llvm) A=(--profile bench-llvm --config 'profile.bench-llvm.inherits="dev"') ;;
    cl) A=(-Zcodegen-backend --profile bench-cl --config 'profile.bench-cl.inherits="dev"' --config 'profile.bench-cl.codegen-backend="cranelift"' --config 'profile.bench-cl.build-override.codegen-backend="llvm"') ;;
    mold) A=(-Zprofile-rustflags --profile bench-mold --config 'profile.bench-mold.inherits="dev"' --config "profile.bench-mold.rustflags=[\"-Clinker-features=-lld\",\"-Clink-self-contained=-linker\",\"-Zunstable-options\",\"-Clink-arg=-fuse-ld=mold\",\"-Clink-arg=-B$MOLD_BIN\"]") ;;
    thr) A=(-Zprofile-rustflags --profile bench-thr --config 'profile.bench-thr.inherits="dev"' --config 'profile.bench-thr.rustflags=["-Zthreads=8"]') ;;
  esac
}

step() { # variant label cargo-args...
  local v=$1 label=$2; shift 2
  local log="$OUT/$v-$label.log"
  flock /tmp/caskade-heavy.lock bash -c '
    s=$(date +%s.%N)
    CARGO_TERM_COLOR=never cargo +nightly "$@" > "'"$log"'" 2>&1; rc=$?
    e=$(date +%s.%N)
    printf "%s %s %.1f rc=%s\n" "'"$v"'" "'"$label"'" "$(echo "$e - $s" | bc)" "$rc"
  ' _ "$@" | tee -a "$OUT/results.txt"
}

edit() { cp "$OUT/sqlite.rs.orig" "$EDIT"; printf '\n#[allow(dead_code)]\nfn bench_marker() -> u32 {\n    %s\n}\n' "$1" >> "$EDIT"; }

for v in "$@"; do
  set_args "$v"
  cp "$OUT/sqlite.rs.orig" "$EDIT"
  step "$v" cold-build build "${A[@]}" --timings -p caskade-core -p caskade-desktop
  step "$v" cold-test test "${A[@]}" --no-run -p caskade-core
  edit $RANDOM
  step "$v" edit-build build "${A[@]}" --timings -p caskade-core -p caskade-desktop
  edit $RANDOM
  step "$v" edit-test test "${A[@]}" --no-run -p caskade-core
  edit $RANDOM
  step "$v" edit-build2 build "${A[@]}" -p caskade-core -p caskade-desktop
  cp "$OUT/sqlite.rs.orig" "$EDIT"
done
