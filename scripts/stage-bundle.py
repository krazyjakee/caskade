#!/usr/bin/env python3
"""Stage a built caskade-native for `tauri bundle` (run under the Cargo lock).

The bundles' executable is caskade-native under apps/native/Tauri.toml's
mainBinaryName, beside it in Cargo's output folder, where the bundler looks.
The pinned 7-Zip goes to apps/native/binaries (scripts/7zz-sidecar.py).
Both local `just build-app` and release CI run this. Cargo's metadata
locates custom target directories; --target selects a cross build.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import re
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
CONFIG = ROOT / "apps/native/Tauri.toml"


def main_binary_name(config):
    # Read without tomllib: the Linux release leg's Python is 3.10.
    found = re.search(r'^mainBinaryName\s*=\s*"([A-Za-z0-9_-]+)"\s*$', Path(config).read_text(), re.MULTILINE)
    if not found:
        raise ValueError(f"no mainBinaryName in {config}")
    return found.group(1)


def stage(target_dir, profile, name, target=None, host=None):
    triple = target or host
    if not triple or any(c not in "abcdefghijklmnopqrstuvwxyz0123456789_-" for c in triple):
        raise ValueError(f"unsupported native target: {triple}")
    suffix = ".exe" if "-windows-" in triple else ""
    built = (Path(target_dir) / target if target else Path(target_dir)) / profile
    source = built / f"caskade-native{suffix}"
    # Check before touching an existing staged binary. A missing native app
    # must fail the bundle build rather than ship a stale one.
    if not source.is_file():
        raise FileNotFoundError(f"Build caskade-native before bundling: {source}")
    destination = built / f"{name}{suffix}"
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
    print(stage(metadata["target_directory"], args.profile, main_binary_name(CONFIG), args.target, host))
    spec = importlib.util.spec_from_file_location("sevenzip_sidecar", ROOT / "scripts/7zz-sidecar.py")
    sidecar = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(sidecar)
    print(json.dumps(sidecar.prepare(args.target or host), indent=2))


if __name__ == "__main__":
    main()
