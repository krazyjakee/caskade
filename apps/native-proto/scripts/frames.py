#!/usr/bin/env python3
"""Runs the prototype's built-in benchmark (CASKADE_PROTO_BENCH) and reports
frame pacing and CPU between its "bench: start" and "bench: end" marks.

  frames.py --out <dir> [--refresh 59.95] -- <prototype binary>

The binary should be built with `--features log-frame-times,log-phase-times`
for frame and phase timings; without them only CPU is reported. The bench
drives the grid itself (no synthetic input), so it can run on the session's
real display. Set CASKADE_PROTO_HOME and the CASKADE_PROTO_BENCH* variables
in the environment (README.md).
"""
import argparse, json, os, re, statistics, subprocess, sys, time

CLK = os.sysconf("SC_CLK_TCK")
FRAME = re.compile(r"^(vello_hybrid|vello|softbuffer|skia|pixels): ([\d.]+)(ns|us|ms|s) \((.*)\)")
RESOLVE = re.compile(r"^Resolve\(\d+\): ([\d.]+)(ns|us|ms|s) \((.*)\)")
MARK = re.compile(r"^proto: ([\d.]+) ms (.*)")
UNIT = {"ns": 1e-6, "us": 1e-3, "ms": 1.0, "s": 1000.0}


def ms(value, unit):
    return float(value) * UNIT[unit]


def parts(text):
    out = {}
    for item in text.split(", "):
        name, _, val = item.partition(": ")
        m = re.match(r"([\d.]+)(ns|us|ms|s)", val)
        if m:
            out[name] = ms(*m.groups())
    return out


def cpu_s(pid):
    try:
        with open(f"/proc/{pid}/stat") as fh:
            f = fh.read().rsplit(")", 1)[1].split()
        return (int(f[11]) + int(f[12])) / CLK
    except OSError:
        return None


def mem(pid):
    out = {}
    try:
        for line in open(f"/proc/{pid}/status"):
            if line.startswith(("RssAnon:", "VmSwap:", "VmRSS:")):
                out[line.split(":")[0]] = int(line.split()[1]) // 1024
    except OSError:
        pass
    return out


def pct(xs, p):
    xs = sorted(xs)
    return round(xs[min(len(xs) - 1, int(round(p / 100 * (len(xs) - 1))))], 2) if xs else None


def stats(xs):
    if not xs:
        return None
    return {"n": len(xs), "mean": round(statistics.fmean(xs), 2), "p50": pct(xs, 50), "p95": pct(xs, 95),
            "p99": pct(xs, 99), "max": round(max(xs), 2)}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--refresh", type=float, default=59.95)
    ap.add_argument("--timeout", type=float, default=600)
    ap.add_argument("cmd", nargs="+")
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)
    log = open(os.path.join(args.out, "app.log"), "w")
    t_launch = time.monotonic()
    proc = subprocess.Popen(args.cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1)
    frames, resolves, marks = [], [], {}
    in_bench, cpu_start, cpu_end, mem_end = False, None, None, None
    first_frame = None
    for line in proc.stdout:
        now = time.monotonic() - t_launch
        log.write(f"{now:.4f} {line}")
        line = line.strip()
        if m := FRAME.match(line):
            if first_frame is None:
                first_frame = now
            if in_bench:
                frames.append((now, ms(m.group(2), m.group(3)), parts(m.group(4))))
        elif m := RESOLVE.match(line):
            if in_bench:
                resolves.append((now, ms(m.group(1), m.group(2)), parts(m.group(3))))
        elif m := MARK.match(line):
            what = m.group(2)
            marks.setdefault(what.split(":")[0] if what.startswith("first page") else what, now)
            if what == "bench: start":
                in_bench, cpu_start = True, (now, cpu_s(proc.pid))
            elif what == "bench: end":
                in_bench, cpu_end, mem_end = False, (now, cpu_s(proc.pid)), mem(proc.pid)
        if time.monotonic() - t_launch > args.timeout:
            proc.kill()
            break
    proc.wait()
    log.close()
    result = {"cmd": args.cmd, "launch_monotonic": t_launch, "env": {k: v for k, v in os.environ.items() if k.startswith("CASKADE_PROTO")},
              "refresh_hz": args.refresh, "marks_s": {k: round(v, 3) for k, v in marks.items()},
              "first_frame_s": round(first_frame, 3) if first_frame else None, "memory_at_end_mb": mem_end}
    if cpu_start and cpu_end and cpu_start[1] is not None and cpu_end[1] is not None:
        span = cpu_end[0] - cpu_start[0]
        used = cpu_end[1] - cpu_start[1]
        result["bench_s"] = round(span, 2)
        result["cpu_percent_of_one_core"] = round(100 * used / span, 2)
        result["cpu_s_per_min"] = round(60 * used / span, 3)
    if frames:
        span = result.get("bench_s") or (frames[-1][0] - frames[0][0])
        intervals = [1000 * (b[0] - a[0]) for a, b in zip(frames, frames[1:])]
        budget = 1000 / args.refresh
        result["frames"] = {
            "count": len(frames), "per_s": round(len(frames) / span, 2),
            "interval_ms": stats(intervals),
            # A frame that came more than half a refresh late missed its vsync.
            "late_intervals": sum(1 for i in intervals if i > 1.5 * budget),
            "late_percent": round(100 * sum(1 for i in intervals if i > 1.5 * budget) / max(1, len(intervals)), 2),
            "render_total_ms": stats([f[1] for f in frames]),
            "render_parts_ms": {k: stats([f[2].get(k, 0) for f in frames]) for k in frames[0][2]},
        }
    if resolves:
        result["resolve"] = {"count": len(resolves), "total_ms": stats([r[1] for r in resolves]),
                             "parts_ms": {k: stats([r[2].get(k, 0) for r in resolves])
                                          for k in ("style", "construct", "layout", "paint_tree")}}
    with open(os.path.join(args.out, "frames.json"), "w") as fh:
        json.dump(result, fh, indent=1)
    print(json.dumps(result, indent=1))


main()
