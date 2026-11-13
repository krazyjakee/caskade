#!/usr/bin/env python3
"""Stage the built native app for the desktop bundles (run under the Cargo lock).

Both local `just build-app` and release CI use native-app.conf.json. Cargo's
metadata locates custom target directories; --target selects a cross build.
"""
import argparse
import json
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def stage(target_dir, binaries, profile, target=None, host=None):
    triple = target or host
    if not triple or any(c not in "abcdefghijklmnopqrstuvwxyz0123456789_-" for c in triple):
        raise ValueError(f"unsupported native target: {triple}")
    suffix = ".exe" if "-windows-" in triple else ""
    built = Path(target_dir) / target if target else Path(target_dir)
    source = built / profile / f"caskade-native{suffix}"
    # Check before touching an existing staged binary. A missing native app
    # must fail the bundle build rather than ship a stale one.
    if not source.is_file():
        raise FileNotFoundError(f"Build caskade-native before bundling: {source}")
    binaries = Path(binaries)
    binaries.mkdir(parents=True, exist_ok=True)
    destination = binaries / f"caskade-native-{triple}{suffix}"
    shutil.copy2(source, destination)
    return destination


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target")
    parser.add_argument("--profile", choices=("debug", "release"), default="debug")
    args = parser.parse_args()
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"], cwd=ROOT))
    host = next(line.removeprefix("host: ") for line in subprocess.check_output(
        ["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
    print(stage(metadata["target_directory"], ROOT / "apps/desktop/src-tauri/binaries",
                args.profile, args.target, host))


if __name__ == "__main__":
    main()
