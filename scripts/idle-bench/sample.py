#!/usr/bin/env python3
"""Samples the idle memory and CPU of every app process in this network
namespace (the benchmark's sandbox), then writes samples.csv, summary.json
and summary.md into --out. Run by run.sh inside the sandbox.

Processes are the app's when their executable's name matches --exe and they
share this process's network namespace, so a Caskade running on the desktop
outside the sandbox is never counted, and an app that restarts itself (for
another renderer) still is.
"""
import argparse, csv, json, os, re, shutil, statistics, subprocess, time

CLK = os.sysconf("SC_CLK_TCK")


def own_netns():
    return os.readlink("/proc/self/ns/net")


def app_pids(pattern, netns):
    pids = []
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        try:
            if os.readlink(f"/proc/{entry}/ns/net") != netns:
                continue
            exe = os.path.basename(os.readlink(f"/proc/{entry}/exe"))
        except OSError:
            continue
        if pattern.fullmatch(exe):
            pids.append((int(entry), exe))
    return pids


def kb(path, key):
    try:
        with open(path) as fh:
            for line in fh:
                if line.startswith(key):
                    return int(line.split()[1])
    except OSError:
        pass
    return 0


def ticks(pid):
    try:
        with open(f"/proc/{pid}/stat") as fh:
            fields = fh.read().rsplit(")", 1)[1].split()
        return int(fields[11]) + int(fields[12])
    except OSError:
        return None


def snapshot(pattern, netns):
    rows = []
    for pid, exe in app_pids(pattern, netns):
        cpu = ticks(pid)
        if cpu is None:
            continue
        status = f"/proc/{pid}/status"
        rows.append({
            "pid": pid,
            "exe": exe,
            "anon_kb": kb(status, "RssAnon:"),
            "swap_kb": kb(status, "VmSwap:"),
            "pss_kb": kb(f"/proc/{pid}/smaps_rollup", "Pss:"),
            "threads": kb(status, "Threads:"),
            "cpu_ticks": cpu,
        })
    return rows


def summarise(samples, interval):
    """samples: list of (t, rows)."""
    totals = []
    for t, rows in samples:
        totals.append({
            "t": t,
            "anon_swap_mb": sum(r["anon_kb"] + r["swap_kb"] for r in rows) / 1024,
            "pss_mb": sum(r["pss_kb"] for r in rows) / 1024,
            "processes": len(rows),
            "threads": sum(r["threads"] for r in rows),
        })
    # CPU: the ticks each process gained between consecutive samples, so
    # processes that come and go are counted while they live.
    cpu_by_exe, prev = {}, {}
    for _, rows in samples:
        now = {r["pid"]: r for r in rows}
        for pid, r in now.items():
            if pid in prev:
                gained = max(0, r["cpu_ticks"] - prev[pid]["cpu_ticks"])
                cpu_by_exe[r["exe"]] = cpu_by_exe.get(r["exe"], 0) + gained
        prev = now
    span = samples[-1][0] - samples[0][0]
    per_min = {exe: round(t / CLK * 60 / span, 3) for exe, t in sorted(cpu_by_exe.items())} if span > 0 else {}
    last = samples[-1][1]
    by_exe = {}
    for r in last:
        e = by_exe.setdefault(r["exe"], {"processes": 0, "anon_swap_mb": 0.0, "pss_mb": 0.0})
        e["processes"] += 1
        e["anon_swap_mb"] += (r["anon_kb"] + r["swap_kb"]) / 1024
        e["pss_mb"] += r["pss_kb"] / 1024
    series = lambda key: [s[key] for s in totals]
    stat = lambda xs: {
        "mean": round(statistics.fmean(xs), 1),
        "median": round(statistics.median(xs), 1),
        "min": round(min(xs), 1),
        "max": round(max(xs), 1),
        "last": round(xs[-1], 1),
    }
    return {
        "samples": len(samples),
        "measured_s": round(span, 1),
        "interval_s": interval,
        "anon_swap_mb": stat(series("anon_swap_mb")),
        "pss_mb": stat(series("pss_mb")),
        "cpu_s_per_min": round(sum(per_min.values()), 3),
        "cpu_s_per_min_by_exe": per_min,
        "last_by_exe": {k: {kk: round(vv, 1) if isinstance(vv, float) else vv for kk, vv in v.items()} for k, v in by_exe.items()},
        "processes_last": totals[-1]["processes"],
        "threads_last": totals[-1]["threads"],
    }


def markdown(summary, meta):
    a, p = summary["anon_swap_mb"], summary["pss_mb"]
    lines = [
        f"**{meta.get('label', '')}** — {meta.get('app', '')}",
        "",
        "| Measure | Mean | Median | Min | Max |",
        "| --- | ---: | ---: | ---: | ---: |",
        f"| Anonymous + swap, all app processes (MB) | {a['mean']} | {a['median']} | {a['min']} | {a['max']} |",
        f"| PSS, all app processes (MB) | {p['mean']} | {p['median']} | {p['min']} | {p['max']} |",
        "",
        f"Idle CPU: **{summary['cpu_s_per_min']} CPU-s/min** over {summary['measured_s']:.0f} s "
        f"({summary['samples']} samples every {summary['interval_s']} s); "
        f"{summary['processes_last']} processes, {summary['threads_last']} threads at the end.",
        "",
        "| Process | Count | Anon + swap (MB) | PSS (MB) | CPU-s/min |",
        "| --- | ---: | ---: | ---: | ---: |",
    ]
    for exe, v in sorted(summary["last_by_exe"].items()):
        lines.append(f"| {exe} | {v['processes']} | {v['anon_swap_mb']} | {v['pss_mb']} | {summary['cpu_s_per_min_by_exe'].get(exe, 0)} |")
    return "\n".join(lines) + "\n"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--exe", default=r"caskade|WebKit\w*Process")
    ap.add_argument("--warmup", type=float, default=300)
    ap.add_argument("--duration", type=float, default=1800)
    ap.add_argument("--interval", type=float, default=10)
    ap.add_argument("--meta", default="{}")
    args = ap.parse_args()
    pattern, netns = re.compile(args.exe), own_netns()
    meta = json.loads(args.meta)

    deadline = time.monotonic() + args.warmup
    while time.monotonic() < deadline:
        if not app_pids(pattern, netns):
            raise SystemExit("no app process found during warm-up: did it start?")
        time.sleep(min(5, max(0, deadline - time.monotonic())))

    # What the app shows as measuring starts: the sandbox's own Xvfb, as
    # evidence the library (not onboarding or an error) is on screen.
    if shutil.which("import") and os.environ.get("DISPLAY"):
        subprocess.run(["import", "-display", os.environ["DISPLAY"], "-window", "root",
                        os.path.join(args.out, "screen.png")], stderr=subprocess.DEVNULL, check=False)

    samples, start = [], time.monotonic()
    with open(os.path.join(args.out, "samples.csv"), "w", newline="") as fh:
        writer = csv.writer(fh)
        writer.writerow(["t", "pid", "exe", "anon_kb", "swap_kb", "pss_kb", "threads", "cpu_ticks"])
        while True:
            t = time.monotonic() - start
            rows = snapshot(pattern, netns)
            if not rows:
                raise SystemExit(f"the app exited after {t:.0f} s of measuring")
            samples.append((t, rows))
            for r in rows:
                writer.writerow([round(t, 1), r["pid"], r["exe"], r["anon_kb"], r["swap_kb"], r["pss_kb"], r["threads"], r["cpu_ticks"]])
            fh.flush()
            if t >= args.duration:
                break
            time.sleep(max(0, args.interval - (time.monotonic() - start - t)))

    summary = summarise(samples, args.interval)
    summary["meta"] = meta
    with open(os.path.join(args.out, "summary.json"), "w") as fh:
        json.dump(summary, fh, indent=1)
    with open(os.path.join(args.out, "summary.md"), "w") as fh:
        fh.write(markdown(summary, meta))
    print(markdown(summary, meta))


main()
