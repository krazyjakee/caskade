"""Prints a Cargo [patch] entry for each named crate found in a checkout.

  crate-paths.py <checkout> <crate>...
"""
import glob
import os
import re
import sys

root, names = sys.argv[1], set(sys.argv[2:])
for toml in sorted(glob.glob(f"{root}/**/Cargo.toml", recursive=True)):
    with open(toml) as fh:
        m = re.search(r'\[package\][^\[]*?\nname\s*=\s*"([^"]+)"', fh.read())
    if m and m.group(1) in names:
        print(f'{m.group(1)} = {{ path = "{os.path.dirname(toml)}" }}')
        names.discard(m.group(1))
if names:
    sys.exit(f"not found in {root}: {sorted(names)}")
