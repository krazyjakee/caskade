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
import shutil
import sqlite3
import statistics
import subprocess
import time
import xml.etree.ElementTree as ET


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


def copy_library(database, artwork, out, appearance_override=None):
    """Copy the database and card/icon thumbnails; never modify the source."""
    data = out / "data/caskade"
    data.mkdir()
    source = sqlite3.connect(database.resolve().as_uri() + "?mode=ro", uri=True)
    copied = sqlite3.connect(data / "library.sqlite")
    source.backup(copied)
    source.close()
    changes = {
        "core.discovery": {"enabled": False},
        "core.streaming": {"hosting": False},
        "core.metadata": {"online": False},
        "core.onboarding": {"completed": True, "sounds": False},
        "core.appearance": {"theme": "caskade.default", "customCssEnabled": False},
    }
    changes["core.appearance"].update(appearance_override or {})
    for namespace in (*changes, "core.launchers"):
        key = f"settings.section.{namespace}"
        row = copied.execute("SELECT value_json FROM settings WHERE key = ?", (key,)).fetchone()
        if row is None:
            continue
        section = json.loads(row[0])
        if namespace == "core.launchers":
            section["value"]["enabled"] = {key: False for key in section["value"]["enabled"]}
        else:
            section["value"].update(changes[namespace])
        copied.execute("UPDATE settings SET value_json = ? WHERE key = ?", (json.dumps(section), key))
    copied.commit()
    # The copy's account chooser would measure sign-in instead of Home.
    # Keep one anonymised, unlocked account so the core's normal single-account
    # startup signs in. The source database is read-only and unchanged.
    account = copied.execute("SELECT id FROM accounts ORDER BY role = 'admin' DESC LIMIT 1").fetchone()
    if account:
        copied.execute("DELETE FROM accounts WHERE id != ?", account)
        copied.execute(
            "UPDATE accounts SET name = 'Benchmark', pin_hash = NULL, max_age = NULL, "
            "pin_failures = 0, pin_lockouts = 0, pin_locked_until_ms = 0, unrated_needs_pin = 0 WHERE id = ?",
            account,
        )
        copied.commit()
    cache = out / "cache/caskade/artwork"
    cache.mkdir(parents=True)
    root = artwork.resolve()
    files = set()
    for (record,) in copied.execute("SELECT record_json FROM artwork"):
        for thumbnail in json.loads(record).get("thumbnails", []):
            if thumbnail["size"] not in (96, 320):
                continue
            relative = Path(thumbnail["path"])
            source_path = (root / relative).resolve()
            if relative.is_absolute() or not source_path.is_relative_to(root) or not source_path.is_file():
                continue
            if relative not in files:
                destination = cache / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source_path, destination)
                files.add(relative)
    games = copied.execute("SELECT count(*) FROM games").fetchone()[0]
    copied.close()
    return {"games": games, "thumbnailFilesCopied": len(files), "account": "single unlocked benchmark account"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("app", type=Path)
    parser.add_argument("mode", choices=("default", "reduced", "stepped", "flat", "grid", "list"))
    parser.add_argument("out", type=Path)
    parser.add_argument("--renderer", choices=("cpu", "auto"), default="cpu",
                        help="grid/list renderer; Home uses auto")
    parser.add_argument("--adapter")
    parser.add_argument("--present-mode", choices=("default", "immediate"), default="default")
    parser.add_argument("--gpu-timestamps", action="store_true")
    parser.add_argument("--library", type=Path)
    parser.add_argument("--artwork", type=Path)
    parser.add_argument("--width", type=int, default=1280)
    parser.add_argument("--height", type=int, default=800)
    args = parser.parse_args()
    if bool(args.library) != bool(args.artwork):
        parser.error("--library and --artwork must be supplied together")
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
    if "customCss" in prefs:
        prefs["customCssEnabled"] = True

    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith("CASKADE_")}
    for key in ("ANYRENDER_GPU_TIMINGS", "ANYRENDER_PRESENT_MODE", "ANYRENDER_FRAME_LIMIT_HZ"):
        env.pop(key, None)
    for key, directory in {
        "XDG_DATA_HOME": "data", "XDG_CONFIG_HOME": "config",
        "XDG_CACHE_HOME": "cache", "XDG_STATE_HOME": "state",
    }.items():
        path = out / directory
        path.mkdir()
        env[key] = str(path)
    config = out / "config/caskade"
    config.mkdir()
    library_copy = copy_library(args.library, args.artwork, out, prefs) if args.library else None
    (config / "native-window.json").write_text(json.dumps({
        "x": 0, "y": 0, "width": args.width, "height": args.height, "maximized": False,
    }))
    if args.adapter:
        env["WGPU_ADAPTER_NAME"] = args.adapter
        env["WGPU_BACKEND"] = "vulkan"
    if args.present_mode != "default":
        env["ANYRENDER_PRESENT_MODE"] = args.present_mode
    if args.gpu_timestamps:
        env["ANYRENDER_GPU_TIMINGS"] = "1"
    env.update(CASKADE_FIXTURES="1", CASKADE_FIXTURE_COUNT="10000",
               CASKADE_NATIVE_NO_TRAY="1", RUST_LOG="warn")
    if library_copy:
        env["CASKADE_FIXTURES"] = "0"
    if prefs and not library_copy:
        env["CASKADE_NATIVE_SMOKE_APPEARANCE"] = json.dumps(prefs)
    scrolling = args.mode in ("grid", "list")
    if scrolling:
        env.update(CASKADE_RENDERER=args.renderer, CASKADE_LIBRARY_BENCH=str(out / "bench.json"),
                   CASKADE_LIBRARY_BENCH_LAYOUT=args.mode)

    samples = []
    gpu_processes = []
    steady_marked = False
    with (out / "app.log").open("w") as log:
        process = subprocess.Popen([str(args.app.resolve())], env=env, stdout=log,
                                   stderr=subprocess.STDOUT)
        start = previous_time = time.monotonic()
        previous = snapshot(process.pid)
        try:
            while process.poll() is None and time.monotonic() - start < (300 if scrolling else 45):
                time.sleep(1)
                now = time.monotonic()
                if args.adapter and not gpu_processes and now - start >= 3:
                    result = subprocess.run(["nvidia-smi", "-q", "-x"], capture_output=True,
                                            text=True, timeout=5, check=True)
                    nvidia = ET.fromstring(result.stdout)
                    for gpu in nvidia.findall("gpu"):
                        for entry in gpu.findall("processes/process_info"):
                            if entry.findtext("pid") == str(process.pid):
                                gpu_processes.append({
                                    "device": gpu.findtext("product_name"),
                                    "driver": nvidia.findtext("driver_version"),
                                    "process": {field.tag: field.text for field in entry},
                                })
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
                if not steady_marked and now - start >= 25:
                    log.write("probe: steady window begins\n")
                    log.flush()
                    steady_marked = True
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
        "renderer": args.renderer if scrolling else "auto", "adapterRequested": args.adapter,
        "presentMode": args.present_mode,
        "gpuProcesses": gpu_processes, "hardwareGpuTimestamps": args.gpu_timestamps,
        "libraryCopy": library_copy,
        "appearanceOverride": prefs, "samples": samples,
        "steadyCpuPercent": statistics.fmean(s["cpuPercent"] for s in steady) if steady else None,
    }
    app_log = (out / "app.log").read_text()
    report["adapterLog"] = [line for line in app_log.splitlines() if line.startswith("GPU adapter:")]
    if args.gpu_timestamps:
        report["gpuFrameMs"] = [float(value) for value in re.findall(r"gpu_frame: ([\d.]+)ms", app_log)]
        steady_log = app_log.split("probe: steady window begins\n", 1)[1] if steady_marked else ""
        report["steadyGpuFrameMs"] = [float(value) for value in re.findall(r"gpu_frame: ([\d.]+)ms", steady_log)]
        tail = sorted(report["gpuFrameMs"][-100:])
        if tail:
            report["gpuFrameSummary"] = {
                "frames": len(report["gpuFrameMs"]), "tailFrames": len(tail),
                "p50ms": statistics.median(tail), "p95ms": tail[round((len(tail) - 1) * .95)],
            }
    if scrolling:
        if not (out / "bench.json").exists():
            (out / "samples.json").write_text(json.dumps(report, indent=2) + "\n")
            raise SystemExit(f"no scroll report; inspect {out / 'app.log'}")
        report["scroll"] = json.loads((out / "bench.json").read_text())
    (out / "samples.json").write_text(json.dumps(report, indent=2) + "\n")
    if process.returncode not in (0, -15):
        raise SystemExit(f"app exited with {process.returncode}; inspect {out / 'app.log'}")
    if args.gpu_timestamps and not any(value > 0 for value in report["gpuFrameMs"]):
        raise SystemExit(f"no valid hardware timestamps; inspect {out / 'app.log'}")
    print(json.dumps({key: value for key, value in report.items() if key not in ("samples", "gpuFrameMs", "steadyGpuFrameMs")}, indent=2))


if __name__ == "__main__":
    main()
