#!/usr/bin/env python3
"""Stage the SHA-pinned official 7-Zip sidecar for the app bundles of a target.

Reuses the installer downloader and its release manifest. No binaries are
committed and no downloaded installer is executed. Pass --target for cross
builds; --dev also puts it beside the host's debug native app (`just dev`).
"""
import argparse
import importlib.util
import json
from pathlib import Path
import platform
import shutil
import subprocess

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location("sevenzip_download", ROOT / "scripts/7zz-download.py")
downloader = importlib.util.module_from_spec(spec)
spec.loader.exec_module(downloader)
# Where apps/native/Tauri.toml's externalBin and resources point.
BINARIES = ROOT / "apps/native/binaries"


def native_target():
    arch = {"x86_64": "x86_64", "amd64": "x86_64", "arm64": "aarch64", "aarch64": "aarch64"}[platform.machine().lower()]
    suffix = {"Linux": "unknown-linux-gnu", "Darwin": "apple-darwin", "Windows": "pc-windows-msvc"}[platform.system()]
    return f"{arch}-{suffix}"


def prepare(target):
    arch = "arm64" if target.startswith("aarch64-") else "x64" if target.startswith("x86_64-") else None
    family = "windows" if "-windows-" in target else "macos" if target.endswith("-apple-darwin") else "linux" if "-linux-" in target else None
    if not arch or not family or any(c not in "abcdefghijklmnopqrstuvwxyz0123456789_-" for c in target):
        raise ValueError(f"unsupported sidecar target: {target}")
    cache = ROOT / "target/sidecars/7zip" / downloader.PINS["version"]
    record = downloader.install(cache, f"{family}-{arch}", None)
    source = Path(record["program"])
    binaries = BINARIES
    binaries.mkdir(parents=True, exist_ok=True)
    suffix = ".exe" if family == "windows" else ""
    destination = binaries / f"7zz-{target}{suffix}"
    shutil.copy2(source, destination)
    shutil.copy2(source.with_name("License.txt"), binaries / "7zip-License.txt")
    if family == "windows":
        # Every Windows bundle is staged independently. Do not combine x64 and
        # ARM64 companion DLLs in a single package.
        shutil.copy2(source.with_name("7z.dll"), binaries / "7z.dll")
    return {"target": target, "program": str(destination), "verified": record}


def stage_dev(record, target_dir):
    """Put the verified extractor beside the host's debug native executable."""
    source = Path(record["program"])
    destination = Path(target_dir) / "debug"
    destination.mkdir(parents=True, exist_ok=True)
    windows = "-windows-" in record["target"]
    shutil.copy2(source, destination / ("7zz.exe" if windows else "7zz"))
    if windows:
        shutil.copy2(source.with_name("7z.dll"), destination / "7z.dll")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", default=native_target())
    parser.add_argument("--dev", action="store_true", help="also stage beside the host debug native app (run under the Cargo lock)")
    args = parser.parse_args()
    if args.dev and args.target != native_target():
        parser.error("--dev stages a host build; omit --target")
    record = prepare(args.target)
    if args.dev:
        metadata = json.loads(subprocess.check_output(
            ["cargo", "metadata", "--format-version", "1", "--no-deps"], cwd=ROOT))
        stage_dev(record, metadata["target_directory"])
    print(json.dumps(record, indent=2))
