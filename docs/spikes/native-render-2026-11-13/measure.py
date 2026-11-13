#!/usr/bin/env python3
"""Short Linux rendering probe; run inside private-desktop.sh and unshare -rn.

Build with bench,smoke,dioxus-native/log-times. This samples only the main
process, not the all-process or hour-long acceptance budget. See ../native-render-2026-11-13.md.
"""

import argparse
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import time


def snapshot(pid):
    try:
        root = Path(f"/proc/{pid}")
        fields = (root / "stat").read_text().rsplit(")", 1)[1].split()
        status = (root / "status").read_text()
        memory = {}
        for key in ("VmRSS", "RssAnon", "VmSwap"):
            match = re.search(rf"^{key}:\s+(\d+)", status, re.MULTILINE)
            if match is None:
                return None  # A process exiting may already be a zombie.
            memory[key] = int(match[1]) / 1024
        return int(fields[11]) + int(fields[12]), memory
    except (OSError, ProcessLookupError):
        return None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("app", type=Path)
    parser.add_argument("mode", choices=("default", "reduced", "stepped", "flat", "grid", "list"))
    parser.add_argument("out", type=Path)
    parser.add_argument("--width", type=int, default=1280)
    parser.add_argument("--height", type=int, default=800)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[3]
    if not os.environ.get("CASKADE_PRIVATE_DESKTOP"):
        parser.error("run under scripts/private-desktop.sh")
    subprocess.run([repo / "scripts/private-desktop.sh", "--guard"], check=True)
    interfaces = {
        line.split(":", 1)[0].strip()
        for line in Path("/proc/net/dev").read_text().splitlines() if ":" in line
    }
    if interfaces - {"lo"}:
        parser.error("run inside an isolated network namespace (unshare -rn)")

    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith("CASKADE_")}
    for key, directory in {
        "XDG_DATA_HOME": "data", "XDG_CONFIG_HOME": "config",
        "XDG_CACHE_HOME": "cache", "XDG_STATE_HOME": "state",
    }.items():
        path = out / directory
        path.mkdir()
        env[key] = str(path)
    config = out / "config/caskade"
    config.mkdir()
    (config / "native-window.json").write_text(json.dumps({
        "x": 0, "y": 0, "width": args.width, "height": args.height, "maximized": False,
    }))
    env.update(CASKADE_FIXTURES="1", CASKADE_FIXTURE_COUNT="10000",
               CASKADE_NATIVE_NO_TRAY="1", RUST_LOG="warn")
    prefs = {}
    if args.mode == "reduced":
        prefs["motion"] = "reduced"
    if args.mode == "stepped":
        prefs["customCss"] = (
            '[data-cs="hero"] [data-part="fx"]::after { animation-timing-function: steps(14); } '
            '[data-cs="hero"] [data-part="glow"] { animation-timing-function: steps(36); }'
        )
    if args.mode == "flat":
        prefs["customCss"] = (
            '[data-cs="backdrop"]::before, [data-cs="backdrop"]::after { content: none; } '
            '[data-cs="hero"] [data-part="fx"]::after, '
            '[data-cs="hero"] [data-part="glow"] { animation-play-state: paused; }'
        )
    if prefs:
        env["CASKADE_NATIVE_SMOKE_APPEARANCE"] = json.dumps(prefs)
    scrolling = args.mode in ("grid", "list")
    if scrolling:
        env.update(CASKADE_RENDERER="cpu", CASKADE_LIBRARY_BENCH=str(out / "bench.json"),
                   CASKADE_LIBRARY_BENCH_LAYOUT=args.mode)

    samples = []
    with (out / "app.log").open("w") as log:
        process = subprocess.Popen([str(args.app.resolve())], env=env, stdout=log,
                                   stderr=subprocess.STDOUT)
        start = previous_time = time.monotonic()
        previous = snapshot(process.pid)
        try:
            while process.poll() is None and time.monotonic() - start < (300 if scrolling else 45):
                time.sleep(1)
                now = time.monotonic()
                current = snapshot(process.pid)
                if current is None:
                    break
                if previous is not None:
                    cpu = (current[0] - previous[0]) / os.sysconf("SC_CLK_TCK")
                    samples.append({
                        "seconds": now - start,
                        "cpuPercent": cpu / (now - previous_time) * 100,
                        "rssMiB": current[1]["VmRSS"],
                        "anonMiB": current[1]["RssAnon"],
                        "swapMiB": current[1]["VmSwap"],
                    })
                previous, previous_time = current, now
        finally:
            if process.poll() is None:
                process.terminate()
            try:
                process.wait(timeout=8)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()

    steady = [sample for sample in samples if sample["seconds"] >= 25]
    report = {
        "app": str(args.app.resolve()), "mode": args.mode,
        "window": [args.width, args.height], "exitCode": process.returncode,
        "appearanceOverride": prefs, "samples": samples,
        "steadyCpuPercent": statistics.fmean(s["cpuPercent"] for s in steady) if steady else None,
    }
    if scrolling:
        if not (out / "bench.json").exists():
            (out / "samples.json").write_text(json.dumps(report, indent=2) + "\n")
            raise SystemExit(f"no scroll report; inspect {out / 'app.log'}")
        report["scroll"] = json.loads((out / "bench.json").read_text())
    (out / "samples.json").write_text(json.dumps(report, indent=2) + "\n")
    if process.returncode not in (0, -15):
        raise SystemExit(f"app exited with {process.returncode}; inspect {out / 'app.log'}")
    print(json.dumps({key: value for key, value in report.items() if key != "samples"}, indent=2))


if __name__ == "__main__":
    main()
