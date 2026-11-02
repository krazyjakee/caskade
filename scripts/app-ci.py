#!/usr/bin/env python3
"""Emulation's generated-fixture contract for the trusted app workflow (#114).

No live user data, emulator installation or builder download is used here.
Each Cargo step holds the heavy lane separately, including the fake binary
build. Cargo JSON supplies the artifact path instead of assuming target/.
"""
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def cargo(arguments, *, env=None, capture=False):
    command = ["cargo", *arguments]
    if sys.platform == "linux":
        if not shutil.which("flock"):
            raise RuntimeError("Linux emulation CI needs flock for the heavy lane")
        command = ["flock", "/tmp/caskade-heavy.lock", *command]
    print("+", shlex.join(command), flush=True)
    return subprocess.run(
        command, cwd=ROOT, env=env, check=True, text=True,
        stdout=subprocess.PIPE if capture else None,
    ).stdout


def binary_artifact(messages):
    artifacts = [
        json.loads(line) for line in messages.splitlines() if line.strip()
    ]
    paths = [Path(item["executable"]) for item in artifacts
             if item.get("reason") == "compiler-artifact"
             and item.get("target", {}).get("name") == "fake-emulator"
             and item.get("executable")]
    if len(paths) != 1 or not paths[0].is_file():
        raise RuntimeError("Cargo did not report one native fake-emulator binary")
    return paths[0].resolve()


def emulation():
    output = cargo([
        "build", "-p", "caskade-emulation-fixtures", "--bin", "fake-emulator",
        "--locked", "--message-format=json",
    ], capture=True)
    binary = binary_artifact(output)
    environment = dict(os.environ, CASKADE_TEST_EMULATOR=str(binary))
    # Later core/adapter workflow steps use the same once-built native helper.
    for filename in set(filter(None, [os.environ.get("GITHUB_ENV"), os.environ.get("GITEA_ENV")])):
        with Path(filename).open("a", encoding="utf-8") as file:
            file.write(f"CASKADE_TEST_EMULATOR={binary}\n")
    report = json.loads(cargo([
        "run", "-p", "xtask", "--locked", "--", "emulation", "check",
    ], env=environment, capture=True))
    if report["rom_files"] != 1000 or report["machines"] != 3:
        raise RuntimeError("emulation fixture catalogue is incomplete")
    artifact = ROOT / "target/emulation-fixtures-report.json"
    artifact.parent.mkdir(parents=True, exist_ok=True)
    artifact.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    cargo(["test", "-p", "caskade-emulation-fixtures", "--locked"], env=environment)
    # The workflow's combined Rust app suite runs every core test, including
    # emulation. Running that subset here too compiled a different feature
    # selection and executed the same tests twice.
    # Includes shipped-definition validation, detection and scanner tests as
    # their owners land them; never a filter that silently skips new modules.
    cargo(["test", "-p", "caskade-sources", "--locked"], env=environment)


def main():
    if sys.argv[1:] != ["emulation"]:
        raise SystemExit("usage: python3 scripts/app-ci.py emulation")
    emulation()


if __name__ == "__main__":
    main()
