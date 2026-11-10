#!/usr/bin/env bash
# BLITZ-3 (#82): checks out Blitz, anyrender and Stylo at the revisions in
# patches/PINS (the ones Cargo.lock pins unpatched), applies Caskade's patches
# (patches/<repo>/*.patch, in order) on a `caskade` branch, and writes
# .cargo/config.toml so the prototype builds against them. Without that file
# the prototype builds the unpatched upstream revisions.
#
#   patches/apply.sh [vendor dir]   (default: /mnt/Rusty/caskade-native-proto/vendor)
#
# The vendor dir must be outside any Cargo workspace (so not inside this
# repo): Cargo resolves the vendored crates' `workspace = true` fields against
# an enclosing workspace otherwise. Re-running resets each checkout to its
# pinned revision and applies the patches again (commit work there first).
set -euo pipefail
proto=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
vendor=${1:-/mnt/Rusty/caskade-native-proto/vendor}
mkdir -p "$vendor"
vendor=$(realpath "$vendor")
case "$vendor/" in "$(git -C "$proto" rev-parse --show-toplevel)/"*)
  echo "the vendor dir must be outside the repo" >&2
  exit 2
  ;;
esac

config=$(mktemp)
grep -v '^#' "$proto/patches/PINS" | while read -r repo url rev crates; do
  dir=$vendor/$repo
  [ -d "$dir/.git" ] || git clone -q "$url" "$dir"
  git -C "$dir" cat-file -e "$rev^{commit}" 2>/dev/null || git -C "$dir" fetch -q origin "$rev"
  git -C "$dir" checkout -q -B caskade "$rev"
  shopt -s nullglob
  patches=("$proto/patches/$repo"/*.patch)
  shopt -u nullglob
  if [ ${#patches[@]} -gt 0 ]; then
    git -C "$dir" -c user.name=caskade -c user.email=caskade@localhost am -q --3way "${patches[@]}"
  fi
  echo "$repo: ${rev:0:9} + ${#patches[@]} patches"
  # Every crate the prototype takes from this repo, so no type exists twice.
  echo "[patch.\"$url\"]" >>"$config"
  python3 -I "$proto/patches/crate-paths.py" "$dir" $crates >>"$config"
  echo >>"$config"
done
mkdir -p "$proto/.cargo"
{
  echo "# Written by patches/apply.sh: builds against the patched checkouts in $vendor."
  cat "$config"
} >"$proto/.cargo/config.toml"
rm -f "$config"
echo "wrote $proto/.cargo/config.toml"
