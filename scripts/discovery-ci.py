#!/usr/bin/env python3
"""Discovery CI actions; serialize Cargo on Unix and Windows runner hosts."""
import argparse
import errno
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import time
import tomllib

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "crates/caskade-discovery/Cargo.toml"
PACKAGE = "caskade-discovery"


def commands(action):
    base = ["-p", PACKAGE, "--locked"]
    if action == "fmt":
        return [["cargo", "fmt", "-p", PACKAGE, "--", "--check"]]
    if action == "build":
        return [["cargo", "build", *base, "--all-features"]]
    if action == "clippy":
        return [["cargo", "clippy", *base, "--all-targets", "--all-features", "--", "-D", "warnings"]]
    if action == "test":
        return [["cargo", "nextest", "run", *base, "--all-features", "--profile", "discovery-ci", "--success-output", "immediate"]]
    if action == "doc":
        return [["cargo", "test", *base, "--all-features", "--doc"]]
    if action == "features":
        features = tomllib.loads(MANIFEST.read_text())["features"]
        return [["cargo", "build", *base, "--no-default-features"], *[
            ["cargo", "build", *base, "--no-default-features", "--features", feature]
            for feature in sorted(features) if feature != "default"
        ]]
    if action == "cross":
        features = tomllib.loads(MANIFEST.read_text())["features"]
        # sqlite bundles C; native macOS jobs build it with the real Apple SDK.
        mac_features = ",".join(sorted(set(features) - {"default", "sqlite"}))
        return [
            ["cargo", "clippy", *base, "--all-targets", "--all-features", "--target", "x86_64-pc-windows-gnu", "--", "-D", "warnings"],
            ["cargo", "clippy", *base, "--all-targets", "--target", "x86_64-apple-darwin", "--no-default-features", "--features", mac_features, "--", "-D", "warnings"],
        ]
    if action == "live":
        # A scaffold can have no ignored tests yet. Make that visible rather
        # than claiming a live source scan occurred; source owners add tests.
        return [["cargo", "nextest", "run", *base, "--all-features", "--profile", "discovery-live", "--run-ignored", "only", "--no-tests", "warn"]]
    if action == "fixtures":
        return [["cargo", "run", "-p", "xtask", "--locked", "--", "discovery", "check"]]
    raise ValueError(action)


def run_locked(command):
    print("+ " + shlex.join(command), flush=True)
    if sys.platform.startswith("linux"):
        return subprocess.run(["flock", "--close", "/tmp/caskade-heavy.lock", *command], cwd=ROOT).returncode
    if os.name != "nt":
        # macOS has flock(2), but no util-linux flock executable by default.
        import fcntl
        with open("/tmp/caskade-heavy.lock", "a+b") as stream:
            fcntl.flock(stream, fcntl.LOCK_EX)
            return subprocess.run(command, cwd=ROOT).returncode
    # Unix flock is not supplied by Windows. A byte lock shared by all actions
    # on this runner has equivalent exclusion; keep the file, never unlink it.
    import msvcrt
    lock = Path(tempfile.gettempdir()) / "caskade-heavy.lock"
    with lock.open("a+b") as stream:
        stream.seek(0, os.SEEK_END)
        if stream.tell() == 0:
            stream.write(b"\0")
            stream.flush()
        while True:
            stream.seek(0)
            try:
                msvcrt.locking(stream.fileno(), msvcrt.LK_NBLCK, 1)
                break
            except OSError as error:
                if error.errno not in (errno.EACCES, errno.EAGAIN, errno.EDEADLK):
                    raise
                time.sleep(0.2)
        try:
            child = subprocess.Popen(command, cwd=ROOT)
            try:
                return child.wait()
            except KeyboardInterrupt:
                child.terminate()
                child.wait()
                return 130
        finally:
            stream.seek(0)
            msvcrt.locking(stream.fileno(), msvcrt.LK_UNLCK, 1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["fmt", "build", "clippy", "test", "doc", "features", "cross", "live", "fixtures"])
    parser.add_argument("--dry-run", action="store_true", help="print commands without building or locking")
    args = parser.parse_args()
    if args.action == "live" and not args.dry_run:
        if os.environ.get("CASKADE_DISCOVERY_LIVE") != "1":
            parser.error("live tests need explicit CASKADE_DISCOVERY_LIVE=1")
    for command in commands(args.action):
        if args.dry_run:
            print(shlex.join(command))
        else:
            result = run_locked(command)
            if result:
                return result
    return 0


if __name__ == "__main__":
    sys.exit(main())
