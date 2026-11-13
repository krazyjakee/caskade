#!/usr/bin/env python3
"""Check app versions and the transition shell's copy of the updater trust key.

Pass the release version (without v) to check a tag; otherwise use native's
version. Reads only committed metadata and never accesses signing secrets.
"""
import argparse
import json
from pathlib import Path
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
PUBLIC_KEY = "crates/caskade-app/updater.pub"


def check(root, version=None):
    config_path = "apps/desktop/src-tauri/tauri.conf.json"
    config = json.loads((root / config_path).read_text())
    found = {config_path: config["version"]}
    crates = {
        "caskade-native": "apps/native/Cargo.toml",
        "caskade-desktop": "apps/desktop/src-tauri/Cargo.toml",
    }
    for path in crates.values():
        found[path] = tomllib.loads((root / path).read_text())["package"]["version"]
    package_path = "apps/desktop/package.json"
    package = json.loads((root / package_path).read_text())
    if "version" in package:
        found[package_path] = package["version"]
    locked = tomllib.loads((root / "Cargo.lock").read_text())["package"]
    for name in crates:
        found[f"Cargo.lock ({name})"] = next(p["version"] for p in locked if p["name"] == name)
    expected = version or found[crates["caskade-native"]]
    errors = [f"{path} says {value}, expected {expected}"
              for path, value in found.items() if value != expected]
    key = (root / PUBLIC_KEY).read_text().strip()
    if not key or config.get("plugins", {}).get("updater", {}).get("pubkey") != key:
        errors.append(f"{config_path}: plugins.updater.pubkey must match {PUBLIC_KEY}")
    return expected, errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version", nargs="?")
    args = parser.parse_args()
    try:
        version, errors = check(ROOT, args.version)
    except (OSError, ValueError, KeyError, StopIteration) as error:
        print(f"Release metadata could not be read: {error}", file=sys.stderr)
        return 1
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        print("See docs/RELEASING.md before tagging.", file=sys.stderr)
        return 1
    print(f"Version {version} everywhere; the native and legacy updater trust keys match.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
