#!/usr/bin/env python3
"""Check the app's version, its bundles' identity and the updater trust key.

The version is caskade-native's (apps/native/Cargo.toml and Cargo.lock);
apps/native/Tauri.toml, the bundle config, takes it from there. Pass the
release version (without v) to check a tag. Installed copies rely on the
bundle identity and the trust key staying as they are. Reads only committed
metadata and never accesses signing secrets.
"""
import argparse
from pathlib import Path
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
PUBLIC_KEY = "crates/caskade-app/updater.pub"
MANIFEST = "apps/native/Cargo.toml"
BUNDLE_CONFIG = "apps/native/Tauri.toml"
# What installs, updates and uninstalls of earlier releases find Caskade by:
# the install folder, uninstall key, deb package, app bundle, shortcuts and
# the caskade:// handler.
IDENTITY = {"productName": "Caskade", "identifier": "dev.caskade.app", "mainBinaryName": "caskade"}
SCHEME = "caskade"


def check(root, version=None):
    found = {MANIFEST: tomllib.loads((root / MANIFEST).read_text())["package"]["version"]}
    locked = tomllib.loads((root / "Cargo.lock").read_text())["package"]
    found["Cargo.lock (caskade-native)"] = next(p["version"] for p in locked if p["name"] == "caskade-native")
    config = tomllib.loads((root / BUNDLE_CONFIG).read_text())
    if "version" in config:
        found[BUNDLE_CONFIG] = config["version"]
    expected = version or found[MANIFEST]
    errors = [f"{path} says {value}, expected {expected}"
              for path, value in found.items() if value != expected]
    errors += [f"{BUNDLE_CONFIG}: {field} must stay {value!r}; installed copies are found by it"
               for field, value in IDENTITY.items() if config.get(field) != value]
    plugins = config.get("plugins", {})
    if SCHEME not in plugins.get("deep-link", {}).get("desktop", {}).get("schemes", []):
        errors.append(f"{BUNDLE_CONFIG}: plugins.deep-link.desktop.schemes must keep {SCHEME!r}")
    key = (root / PUBLIC_KEY).read_text().strip()
    pubkey = plugins.get("updater", {}).get("pubkey", "")
    if not key:
        errors.append(f"{PUBLIC_KEY} is empty")
    elif not pubkey or (root / BUNDLE_CONFIG).parent.joinpath(pubkey).resolve() != (root / PUBLIC_KEY).resolve():
        errors.append(f"{BUNDLE_CONFIG}: plugins.updater.pubkey must be the path of {PUBLIC_KEY}")
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
    print(f"Version {version} in caskade-native's manifest and lockfile; the bundle identity and updater trust key are unchanged.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
