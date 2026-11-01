#!/usr/bin/env python3
"""Download pinned official console binaries; never install or execute an installer.

On Windows use pinned 7zr.exe only to unpack the signed-release installer as an
archive. Cross-packaging Windows on Linux/macOS uses an explicitly supplied
--bootstrap or the already verified native console under the same output root.
Only selected known members are extracted from verified upstream packages.
"""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import subprocess
import tarfile
import urllib.request

SCRIPT_ROOT = Path(__file__).resolve().parent
PINS = json.loads((SCRIPT_ROOT / "7zz-pins.json").read_text())


def host_platform():
    system = {"Linux": "linux", "Darwin": "macos", "Windows": "windows"}.get(platform.system())
    machine = platform.machine().lower()
    arch = {"x86_64": "x64", "amd64": "x64", "arm64": "arm64", "aarch64": "arm64"}.get(machine)
    if not system or not arch:
        raise ValueError(f"unsupported sidecar host: {platform.system()}/{machine}")
    return f"{system}-{arch}"


def verified_package(root, package):
    destination = root / "downloads" / package["name"]
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.exists():
        data = destination.read_bytes()
    else:
        request = urllib.request.Request(PINS["base_url"] + package["name"], headers={"User-Agent": "caskade-sidecar-bootstrap"})
        with urllib.request.urlopen(request, timeout=60) as response:
            data = response.read(32 * 1024 * 1024 + 1)
        if len(data) > 32 * 1024 * 1024:
            raise ValueError("upstream sidecar package exceeds 32 MiB")
    actual = hashlib.sha256(data).hexdigest()
    if actual != package["sha256"]:
        raise ValueError(f"SHA-256 mismatch for {package['name']}: {actual}")
    temporary = destination.with_suffix(destination.suffix + ".part")
    temporary.write_bytes(data)
    temporary.replace(destination)
    return destination, data


def install(root, target, bootstrap):
    package = PINS["packages"][target]
    archive_path, data = verified_package(root, package)
    destination = root / target
    destination.mkdir(parents=True, exist_ok=True)
    if target.startswith("windows-"):
        if bootstrap:
            tool = Path(bootstrap).resolve()
        elif os.name == "nt":
            tool, _ = verified_package(root, PINS["windows_bootstrap"])
        else:
            native = host_platform()
            tool = root / native / "7zz"
            install(root, native, None)
        subprocess.run([str(tool), "x", "-y", f"-o{destination}", "--", str(archive_path), "7z.exe", "7z.dll", "License.txt"], check=True, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=60)
        program = destination / "7zz.exe"
        (destination / "7z.exe").replace(program)
        if not (destination / "7z.dll").is_file():
            raise ValueError("official Windows console requires 7z.dll")
    else:
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:xz") as archive:
            for name in ("7zz", "License.txt", "readme.txt"):
                member = archive.getmember(name)
                if not member.isfile() or member.size > 16 * 1024 * 1024:
                    raise ValueError(f"unexpected upstream member: {name}")
                source = archive.extractfile(member)
                if source is None:
                    raise ValueError(f"missing upstream file: {name}")
                (destination / name).write_bytes(source.read())
        program = destination / "7zz"
        program.chmod(0o755)
    files = [program] + ([destination / "7z.dll"] if target.startswith("windows-") else [])
    record = {"version": PINS["version"], "platform": target, "archive_sha256": package["sha256"], "program": str(program), "files": {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in files}}
    (destination / "verified.json").write_text(json.dumps(record, indent=2) + "\n")
    return record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=sorted(PINS["packages"]), default=host_platform())
    parser.add_argument("--output", type=Path, default=SCRIPT_ROOT.parent / "target" / "sidecars" / "7zip" / PINS["version"])
    parser.add_argument("--bootstrap", help="explicit extractor for a verified Windows installer package")
    parser.add_argument("--all", action="store_true", help="prepare all supported package targets")
    args = parser.parse_args()
    root = args.output.resolve()
    targets = list(PINS["packages"]) if args.all else [args.platform]
    records = [install(root, target, args.bootstrap) for target in targets]
    print(json.dumps(records if args.all else records[0], indent=2))


if __name__ == "__main__":
    main()
