#!/usr/bin/env python3
"""Samples the memory and CPU of every process an app runs (run.sh starts it).

The app's processes are the started one and its descendants, plus, with
--exe, every process in this network namespace whose executable matches (run
in its own namespace, so WebKit's helpers count even if reparented, and
nothing outside does). After --settle seconds (with optional clicks on the
private display from half way, to open a view), it samples every --interval
seconds for --duration, writes samples.csv and summary.json into --out, and
stops the processes it measured.
"""
import argparse, csv, json, os, re, signal, statistics, subprocess, time

CLK = os.sysconf("SC_CLK_TCK")


def children_map():
    kids = {}
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        try:
            with open(f"/proc/{entry}/stat") as fh:
                ppid = int(fh.read().rsplit(")", 1)[1].split()[1])
        except OSError:
            continue
        kids.setdefault(ppid, []).append(int(entry))
    return kids


def exe_name(pid):
    try:
        return os.path.basename(os.readlink(f"/proc/{pid}/exe"))
    except OSError:
        return None


def app_pids(root, pattern, netns):
    found, todo, kids = set(), [root], children_map()
    while todo:
        pid = todo.pop()
        if pid in found:
            continue
        found.add(pid)
        todo += kids.get(pid, [])
    if pattern:
        for entry in os.listdir("/proc"):
            if entry.isdigit():
                try:
                    if os.readlink(f"/proc/{entry}/ns/net") != netns:
                        continue
                except OSError:
                    continue
                name = exe_name(entry)
                if name and pattern.fullmatch(name):
                    found.add(int(entry))
    # Measuring tools and the display server aren't the app.
    skip = re.compile(r"python3.*|Xvfb|dbus-.*|gnome-keyring-daemon|bash|xdotool|import")
    return [p for p in found if (n := exe_name(p)) and not skip.fullmatch(n)]


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
            f = fh.read().rsplit(")", 1)[1].split()
        return int(f[11]) + int(f[12])
    except OSError:
        return None


def snapshot(root, pattern, netns):
    rows = []
    for pid in app_pids(root, pattern, netns):
        cpu = ticks(pid)
        if cpu is None:
            continue
        status = f"/proc/{pid}/status"
        rows.append({
            "pid": pid, "exe": exe_name(pid) or "?",
            "anon_kb": kb(status, "RssAnon:"), "swap_kb": kb(status, "VmSwap:"),
            "rss_kb": kb(status, "VmRSS:"), "pss_kb": kb(f"/proc/{pid}/smaps_rollup", "Pss:"),
            "threads": kb(status, "Threads:"), "cpu_ticks": cpu,
        })
    return rows


def summarise(samples):
    stat = lambda xs: {"mean": round(statistics.fmean(xs), 1), "median": round(statistics.median(xs), 1),
                       "min": round(min(xs), 1), "max": round(max(xs), 1)}
    anon = [sum(r["anon_kb"] + r["swap_kb"] for r in rows) / 1024 for _, rows in samples]
    pss = [sum(r["pss_kb"] for r in rows) / 1024 for _, rows in samples]
    cpu, prev = {}, {}
    for _, rows in samples:
        now = {r["pid"]: r for r in rows}
        for pid, r in now.items():
            if pid in prev:
                cpu[r["exe"]] = cpu.get(r["exe"], 0) + max(0, r["cpu_ticks"] - prev[pid]["cpu_ticks"])
        prev = now
    span = samples[-1][0] - samples[0][0]
    per_min = {e: round(t / CLK * 60 / span, 3) for e, t in sorted(cpu.items())} if span else {}
    by_exe = {}
    for r in samples[-1][1]:
        e = by_exe.setdefault(r["exe"], {"processes": 0, "anon_swap_mb": 0.0, "pss_mb": 0.0, "threads": 0})
        e["processes"] += 1
        e["anon_swap_mb"] = round(e["anon_swap_mb"] + (r["anon_kb"] + r["swap_kb"]) / 1024, 1)
        e["pss_mb"] = round(e["pss_mb"] + r["pss_kb"] / 1024, 1)
        e["threads"] += r["threads"]
    return {"samples": len(samples), "measured_s": round(span, 1), "anon_swap_mb": stat(anon),
            "pss_mb": stat(pss), "cpu_s_per_min": round(sum(per_min.values()), 3),
            "cpu_s_per_min_by_exe": per_min, "last_by_exe": by_exe,
            "processes": len(samples[-1][1]), "threads": sum(r["threads"] for r in samples[-1][1])}


def stop(pids):
    for sig in (signal.SIGTERM, signal.SIGKILL):
        for pid in pids:
            try:
                os.kill(pid, sig)
            except OSError:
                pass
        time.sleep(2)


def boot_s():
    return time.clock_gettime(time.CLOCK_BOOTTIME)


def started_s(pid):
    """When the process started, on the boot clock."""
    with open(f"/proc/{pid}/stat") as fh:
        return int(fh.read().rsplit(")", 1)[1].split()[19]) / CLK


def startup(pid, seconds, out):
    """Grabs the display from the start: when content first shows, and when
    the screen last changed by more than faint steps (the ambient background's)."""
    from PIL import ImageChops, ImageGrab
    t0, frames = started_s(pid), []
    while boot_s() - t0 < seconds:
        try:
            frames.append((boot_s() - t0, ImageGrab.grab(xdisplay=os.environ["DISPLAY"]).convert("L")))
        except OSError:
            time.sleep(0.02)
    def changed(a, b):
        d = ImageChops.difference(a, b).point(lambda v: 255 if v > 40 else 0)
        return d.histogram()[255] / (a.width * a.height)
    blank = frames[0][1]
    first = next((t for t, im in frames if changed(blank, im) > 0.05), None)
    last = None
    for (_, a), (t, b) in zip(frames, frames[1:]):
        if changed(a, b) > 0.005:
            last = t
    result = {"grabs": len(frames), "first_content_s": round(first, 3) if first is not None else None,
              "visually_complete_s": round(last, 3) if last is not None else None}
    json.dump(result, open(os.path.join(out, "startup.json"), "w"), indent=1)
    print(json.dumps(result))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", required=True)
    ap.add_argument("--pid", type=int, required=True)
    ap.add_argument("--settle", type=float, default=60)
    ap.add_argument("--duration", type=float, default=120)
    ap.add_argument("--interval", type=float, default=5)
    ap.add_argument("--click", default="")
    ap.add_argument("--exe", default="")
    ap.add_argument("--startup", type=float, default=0, help="grab the display for this long from the start")
    args = ap.parse_args()
    pattern = re.compile(args.exe) if args.exe else None
    netns = os.readlink("/proc/self/ns/net")
    if args.startup:
        startup(args.pid, args.startup, args.out)
    start = time.monotonic()
    # Clicks from half way through settling, 10 s apart.
    clicks = [c.split(",") for c in args.click.split(";") if c]
    while time.monotonic() - start < args.settle:
        if not os.path.exists(f"/proc/{args.pid}"):
            raise SystemExit("the app exited while settling (see app.log)")
        if clicks and time.monotonic() - start >= args.settle / 2 + 10 * (len(args.click.split(";")) - len(clicks)):
            x, y = clicks.pop(0)
            subprocess.run(["xdotool", "mousemove", x, y, "click", "1"], check=False)
            subprocess.run(["xdotool", "mousemove", "1", "1"], check=False)
        time.sleep(0.5)
    if os.environ.get("DISPLAY"):
        subprocess.run(["import", "-display", os.environ["DISPLAY"], "-window", "root",
                        os.path.join(args.out, "screen.png")], stderr=subprocess.DEVNULL, check=False)
    samples, t0 = [], time.monotonic()
    with open(os.path.join(args.out, "samples.csv"), "w", newline="") as fh:
        w = csv.writer(fh)
        w.writerow(["t", "pid", "exe", "anon_kb", "swap_kb", "rss_kb", "pss_kb", "threads", "cpu_ticks"])
        while True:
            t = time.monotonic() - t0
            rows = snapshot(args.pid, pattern, netns)
            if not rows:
                raise SystemExit(f"the app exited after {t:.0f} s of measuring")
            samples.append((t, rows))
            for r in rows:
                w.writerow([round(t, 1)] + [r[k] for k in ("pid", "exe", "anon_kb", "swap_kb", "rss_kb", "pss_kb", "threads", "cpu_ticks")])
            fh.flush()
            if t >= args.duration:
                break
            time.sleep(max(0, args.interval - (time.monotonic() - t0 - t)))
    summary = summarise(samples)
    summary["meta"] = json.load(open(os.path.join(args.out, "meta.json")))
    with open(os.path.join(args.out, "summary.json"), "w") as fh:
        json.dump(summary, fh, indent=1)
    print(json.dumps({k: summary[k] for k in ("anon_swap_mb", "pss_mb", "cpu_s_per_min", "cpu_s_per_min_by_exe", "processes", "threads")}))
    stop(app_pids(args.pid, pattern, netns))


main()
