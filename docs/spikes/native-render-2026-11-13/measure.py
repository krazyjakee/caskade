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
    parser.add_argument("mode", choices=("default", "reduced", "stepped", "flat", "backdrop-only",
                                         "grid", "list"))
    parser.add_argument("out", type=Path)
    parser.add_argument("--renderer", choices=("cpu", "auto"), default="cpu",
                        help="grid/list renderer; Home uses auto")
    parser.add_argument("--adapter")
    parser.add_argument("--present-mode", choices=("default", "immediate"), default="default")
    parser.add_argument("--gpu-timestamps", action="store_true")
    parser.add_argument("--library", type=Path)
    parser.add_argument("--artwork", type=Path)
    parser.add_argument("--backdrop", choices=("auto", "still", "css"), default="auto",
                        help="CASKADE_BACKDROP: the GPU backdrop (#149), or the CSS one still or animated")
    parser.add_argument("--hide-after", type=float,
                        help="hide the window to the tray after this many seconds and sample it hidden")
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
    if args.mode == "backdrop-only":
        # Home's hero loops paused: only the backdrop moves.
        prefs["customCss"] = (
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
               CASKADE_NATIVE_NO_TRAY="1", RUST_LOG="warn",
               CASKADE_BACKDROP=args.backdrop,
               CASKADE_NATIVE_SMOKE_FRAMES=str(out / "frames.json"))
    if args.hide_after is not None:
        env["CASKADE_NATIVE_SMOKE_HIDE"] = str(args.hide_after)
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
    gpu_memory = []
    frame_counts = []
    steady_marked = False
    hidden_marked = False
    duration = 300 if scrolling else (45 if args.hide_after is None else args.hide_after + 25)
    with (out / "app.log").open("w") as log:
        process = subprocess.Popen([str(args.app.resolve())], env=env, stdout=log,
                                   stderr=subprocess.STDOUT)
        start = previous_time = time.monotonic()
        previous = snapshot(process.pid)
        try:
            while process.poll() is None and time.monotonic() - start < duration:
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
                if args.adapter and now - start >= 3:
                    result = subprocess.run(["nvidia-smi", "-q", "-x"], capture_output=True,
                                            text=True, timeout=5, check=True)
                    for entry in ET.fromstring(result.stdout).iter("process_info"):
                        if entry.findtext("pid") == str(process.pid):
                            used = (entry.findtext("used_memory") or "").split()
                            if used and used[0].isdigit():
                                gpu_memory.append({"seconds": now - start, "usedMiB": int(used[0])})
                try:
                    counts = json.loads((out / "frames.json").read_text())
                    frame_counts.append({"seconds": now - start, **counts})
                except (OSError, ValueError):
                    pass
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
                if (args.hide_after is not None and not hidden_marked
                        and now - start >= args.hide_after + 5):
                    log.write("probe: hidden window begins\n")
                    log.flush()
                    hidden_marked = True
        finally:
            if process.poll() is None:
                process.terminate()
            try:
                process.wait(timeout=8)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()

    # Shown: from 25 s to the hide (or the end); hidden: from 5 s after it.
    shown_end = args.hide_after if args.hide_after is not None else float("inf")
    steady = [sample for sample in samples if 25 <= sample["seconds"] < shown_end]
    hidden = ([sample for sample in samples if sample["seconds"] >= args.hide_after + 5]
              if args.hide_after is not None else [])

    def window(series, low, high):
        return [entry for entry in series if low <= entry["seconds"] < high]

    def rates(series):
        if len(series) < 2:
            return None
        first, last = series[0], series[-1]
        span = last["seconds"] - first["seconds"]
        return {
            "seconds": span,
            "documentFramesPerSecond": (last["document"] - first["document"]) / span,
            "backdropFramesPerSecond": (last["backdrop"] - first["backdrop"]) / span,
            "gpuBackdrop": last["gpu"], "released": last.get("released"),
        }
    report = {
        "app": str(args.app.resolve()), "mode": args.mode,
        "window": [args.width, args.height], "exitCode": process.returncode,
        "renderer": args.renderer if scrolling else "auto", "adapterRequested": args.adapter,
        "presentMode": args.present_mode,
        "gpuProcesses": gpu_processes, "hardwareGpuTimestamps": args.gpu_timestamps,
        "libraryCopy": library_copy,
        "appearanceOverride": prefs, "samples": samples,
        "steadyCpuPercent": statistics.fmean(s["cpuPercent"] for s in steady) if steady else None,
        "steadyAnonMiB": statistics.fmean(s["anonMiB"] for s in steady) if steady else None,
        "backdrop": args.backdrop,
        "steadyFrames": rates(window(frame_counts, 25, shown_end)),
        "gpuMemoryMiB": gpu_memory,
    }
    if args.hide_after is not None:
        report["hiddenCpuPercent"] = statistics.fmean(s["cpuPercent"] for s in hidden) if hidden else None
        report["hiddenAnonMiB"] = statistics.fmean(s["anonMiB"] for s in hidden) if hidden else None
        report["hiddenFrames"] = rates(window(frame_counts, args.hide_after + 5, float("inf")))
        shown_gpu = window(gpu_memory, 25, shown_end)
        hidden_gpu = window(gpu_memory, args.hide_after + 5, float("inf"))
        report["shownGpuMemoryMiB"] = shown_gpu[-1]["usedMiB"] if shown_gpu else None
        report["hiddenGpuMemoryMiB"] = hidden_gpu[-1]["usedMiB"] if hidden_gpu else None
    app_log = (out / "app.log").read_text()
    report["adapterLog"] = [line for line in app_log.splitlines() if line.startswith("GPU adapter:")]
    steady_text = app_log.split("probe: steady window begins\n", 1)[1] if steady_marked else ""
    steady_text = steady_text.split("probe: hidden window begins\n", 1)[0]
    units = {"ms": 1.0, "µs": 0.001, "us": 0.001, "s": 1000.0}

    def timings(pattern):
        found = [float(value) * units[unit]
                 for value, unit in re.findall(pattern, steady_text, re.MULTILINE)]
        if not found:
            return None
        found.sort()
        return {"frames": len(found), "p50ms": statistics.median(found),
                "p95ms": found[round((len(found) - 1) * .95)]}

    # CPU-side: a backdrop frame's encode, submit and present; a document
    # frame's resolve. GPU: the backdrop's own pass.
    report["steadyBackdropCpu"] = timings(r"vello_hybrid backdrop: ([\d.]+)(ms|µs|us|s) ")
    report["steadyResolves"] = len(re.findall(r"^Resolve\(", steady_text, re.MULTILINE))
    report["steadyDocumentFrameCpu"] = timings(r"^vello_hybrid: ([\d.]+)(ms|µs|us|s) ")
    report["steadyGpuBackdrop"] = timings(r"gpu_backdrop: ([\d.]+)(ms)")
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
