#!/usr/bin/env bash
# Regenerates vendor/blitz/crates: Blitz, anyrender and Stylo at the revisions
# in PINS with Caskade's patches (patches/<repo>/*.patch, in order), each crate
# as a standalone package (normalize.py). The root workspace (caskade-native)
# builds these through [patch] entries.
#
#   vendor/blitz/update.sh [work dir]
#
# The work dir (default ~/.cache/caskade-blitz-vendor) holds the upstream
# checkouts, each on a `caskade` branch with the series applied; it must be
# outside this repo. To change a patch: commit in the checkout, then
#   git -C <work>/<repo> format-patch --zero-commit -N --no-signature -o "$PWD/vendor/blitz/patches/<repo>" <pin>..caskade
# (after deleting the old files; -o absolute) and run this again. This resets
# the `caskade` branches to the exported series: export first. README.md has the
# pin-bump procedure.
set -euo pipefail
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(git -C "$here" rev-parse --show-toplevel)
work=${1:-${XDG_CACHE_HOME:-$HOME/.cache}/caskade-blitz-vendor}
mkdir -p "$work"
work=$(realpath "$work")
case "$work/" in "$repo_root/"*)
  echo "the work dir must be outside the repo" >&2
  exit 2
  ;;
esac

all_crates=()
meta=$work/meta
rm -rf "$meta"
mkdir -p "$meta/src"
: >"$meta/src/lib.rs"
deps=""
patches=""
while read -r repo url rev crates; do
  dir=$work/$repo
  [ -d "$dir/.git" ] || git clone -q "$url" "$dir"
  git -C "$dir" cat-file -e "$rev^{commit}" 2>/dev/null || git -C "$dir" fetch -q origin "$rev"
  git -C "$dir" checkout -q -B caskade "$rev"
  shopt -s nullglob
  series=("$here/patches/$repo"/*.patch)
  shopt -u nullglob
  if [ ${#series[@]} -gt 0 ]; then
    git -C "$dir" -c user.name=caskade -c user.email=caskade@localhost am -q --3way "${series[@]}"
  fi
  echo "$repo: ${rev:0:9} + ${#series[@]} patches"
  patches+="[patch.\"$url\"]"$'\n'
  for crate in $crates; do
    path=$(python3 -I - "$dir" "$crate" <<'EOF'
import glob, os, re, sys
root, name = sys.argv[1:]
for toml in sorted(glob.glob(f"{root}/**/Cargo.toml", recursive=True)):
    m = re.search(r'\[package\][^\[]*?\nname\s*=\s*"([^"]+)"', open(toml).read())
    if m and m.group(1) == name:
        print(os.path.dirname(toml))
        break
else:
    sys.exit(f"{name} not found in {root}")
EOF
)
    deps+="$crate = { path = \"$path\" }"$'\n'
    patches+="$crate = { path = \"$path\" }"$'\n'
    all_crates+=("$crate")
  done
  patches+=$'\n'
done < <(grep -v '^#' "$here/PINS")

cat >"$meta/Cargo.toml" <<EOF
[package]
name = "blitz-vendor-meta"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
$deps
$patches
EOF
cargo metadata --format-version 1 --manifest-path "$meta/Cargo.toml" >"$work/metadata.json"

rm -rf "$here/crates"
mkdir -p "$here/crates"
python3 -I "$here/normalize.py" "$work/metadata.json" "$here/crates" "${all_crates[@]}" >/dev/null
# Licences at the root of an upstream repo go with each of its crates.
shopt -s nullglob
while read -r repo _url _rev crates; do
  for crate in $crates; do
    for licence in "$work/$repo"/LICENSE*; do
      [ -e "$here/crates/$crate/$(basename "$licence")" ] || cp "$licence" "$here/crates/$crate/"
    done
  done
done < <(grep -v '^#' "$here/PINS")
shopt -u nullglob
echo "wrote $(ls "$here/crates" | wc -l) crates to vendor/blitz/crates ($(du -sh "$here/crates" | cut -f1))"
