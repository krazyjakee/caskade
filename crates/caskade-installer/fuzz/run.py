#!/usr/bin/env python3
"""Build under the heavy lock, then release it for a single low-priority fuzz run."""
import argparse
from datetime import datetime, timezone
import os
from pathlib import Path
import subprocess
from seed import seed

HERE = Path(__file__).resolve().parent

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("target", choices=sorted(p.stem for p in (HERE / "fuzz_targets").glob("*.rs")))
    parser.add_argument("--seconds", type=int, default=600)
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--build-only", action="store_true")
    group.add_argument("--skip-build", action="store_true")
    args = parser.parse_args()
    if args.seconds < 1:
        parser.error("--seconds must be positive")
    target_dir = Path(os.environ.get("CARGO_TARGET_DIR", HERE / "target")).resolve()
    if not args.skip_build:
        subprocess.run(["flock", "/tmp/caskade-heavy.lock", "cargo", "+nightly", "fuzz", "build", args.target, "--target-dir", str(target_dir)], cwd=HERE.parent, check=True)
    if args.build_only:
        return
    host = next(line.split(": ", 1)[1] for line in subprocess.check_output(["rustc", "+nightly", "-vV"], text=True).splitlines() if line.startswith("host: "))
    binary = target_dir / host / "release" / args.target
    if not binary.is_file():
        parser.error(f"missing built fuzzer {binary}")
    corpus = seed(args.target)
    logs = HERE / "logs" / args.target
    artifacts = HERE / "artifacts" / args.target
    logs.mkdir(parents=True, exist_ok=True)
    artifacts.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    run_dir = logs / stamp
    run_dir.mkdir()
    command = ["flock", "/tmp/caskade-fuzz.lock", "nice", "-n", "19", str(binary), f"-max_total_time={args.seconds}", "-jobs=1", "-workers=1", "-max_len=1048576", "-timeout=30", "-rss_limit_mb=1024", "-print_final_stats=1", f"-artifact_prefix={artifacts}/", str(corpus)]
    print(f"{stamp} {args.target}: logs={run_dir}", flush=True)
    with (run_dir / "driver.log").open("w") as output:
        result = subprocess.run(command, cwd=run_dir, stdout=output, stderr=subprocess.STDOUT)
    (run_dir / "result.txt").write_text(f"target={args.target}\nrequested_seconds={args.seconds}\nexit_code={result.returncode}\nfinished={datetime.now(timezone.utc).isoformat()}\n")
    print(f"{args.target}: exit={result.returncode}; evidence={run_dir}", flush=True)
    raise SystemExit(result.returncode)

if __name__ == "__main__":
    main()
