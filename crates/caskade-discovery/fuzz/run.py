#!/usr/bin/env python3
"""Seed generated metadata, build under heavy lane, run under fuzz lane."""
import argparse
import json
import tomllib
import os
from datetime import datetime, timezone
from pathlib import Path
import shutil
import re
import subprocess

HERE = Path(__file__).resolve().parent
FORMATS = HERE.parent / "tests/fixtures/formats"
SEEDS = {
    "binary_vdf": ["shortcuts.vdf", "appinfo-v28.vdf", "appinfo-v29.vdf"],
    "lnk": ["unicode.lnk"], "desktop": ["fixture.desktop"],
    "url": ["fixture.url"], "plist": ["Info.plist"],
    "protobuf": ["product.db"], "sqlite": ["pga.db", "GameInstallInfo.sqlite"],
    "text_vdf": [],
    "registry": ["registry66/wine/system.reg", "registry66/proton/user.reg"],
    "epic": [],
}
def main():
    parser = argparse.ArgumentParser()
    targets = sorted(item["name"] for item in tomllib.loads((HERE / "Cargo.toml").read_text())["bin"])
    parser.add_argument("target", nargs="?", choices=targets)
    parser.add_argument("--list-targets", action="store_true")
    parser.add_argument("--seconds", type=int, default=600)
    parser.add_argument("--skip-build", action="store_true")
    args = parser.parse_args()
    if args.list_targets:
        print(json.dumps(targets))
        return
    if args.target is None:
        parser.error("target is required unless --list-targets is set")
    if not 1 <= args.seconds <= 3600:
        parser.error("seconds must be in 1..=3600")
    if not args.skip_build:
        subprocess.run(["flock", "/tmp/caskade-heavy.lock", "cargo", "+nightly", "fuzz", "build", args.target, "--target-dir", str(HERE / "target")], cwd=HERE.parent, check=True)
    corpus = HERE / "corpus" / args.target
    corpus.mkdir(parents=True, exist_ok=True)
    for name in SEEDS.get(args.target, []):
        source = FORMATS / name
        filename = name.replace("/", "_")
        shutil.copyfile(source, corpus / filename)
        data = source.read_bytes()
        for divisor in (2, 4):
            (corpus / f"{filename}.prefix{divisor}").write_bytes(data[:len(data)//divisor])
    if args.target == "text_vdf":
        shutil.copyfile(HERE.parent / "tests/fixtures/src/linux/appmanifest.acf", corpus / "appmanifest.acf")
    if args.target == "epic":
        shutil.copyfile(HERE.parent / "tests/fixtures/src/windows/epic.item", corpus / "installed.item")
        (corpus / "incomplete.item").write_text('{"AppName":"Fixture","bIsIncompleteInstall":true}')
    artifacts = HERE / "artifacts" / args.target
    artifacts.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    logs = HERE / "logs" / args.target / stamp
    logs.mkdir(parents=True)
    host = subprocess.check_output(["rustc", "+nightly", "-vV"], text=True).split("host: ")[1].splitlines()[0]
    binary = HERE / "target" / host / "release" / args.target
    command = ["flock", "/tmp/caskade-fuzz.lock", "nice", "-n", "19", str(binary), str(corpus), f"-max_total_time={args.seconds}", "-max_len=65536", "-timeout=10", "-rss_limit_mb=1024", "-print_final_stats=1", f"-artifact_prefix={artifacts}/"]
    with (logs / "driver.log").open("w") as output:
        environment = dict(os.environ)
        # Match cargo-fuzz's runner default: C function-local static names in
        # SQLite's amalgamation otherwise produce false C++ ODR violations.
        environment["ASAN_OPTIONS"] = environment.get("ASAN_OPTIONS", "") + ":detect_odr_violation=0"
        result = subprocess.run(command, stdout=output, stderr=subprocess.STDOUT, env=environment)
    executed = re.search(r"stat::number_of_executed_units:\s*(\d+)", (logs / "driver.log").read_text(errors="replace"))
    summary = {
        "target": args.target, "requested_seconds": args.seconds,
        "commit": environment.get("GITHUB_SHA"),
        "exit_code": result.returncode,
        "executions": int(executed.group(1)) if executed else 0,
        "finished": datetime.now(timezone.utc).isoformat(),
        "corpus_files": sum(1 for path in corpus.iterdir() if path.is_file() and not path.is_symlink()),
    }
    (logs / "result.json").write_text(json.dumps(summary, indent=2) + "\n")
    (logs / "result.txt").write_text(f"target={args.target}\nrequested_seconds={args.seconds}\nexit_code={result.returncode}\nfinished={datetime.now(timezone.utc).isoformat()}\n")
    print(f"{args.target}: exit={result.returncode}; logs={logs}", flush=True)
    raise SystemExit(result.returncode)

if __name__ == "__main__":
    main()
