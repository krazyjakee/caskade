#!/usr/bin/env python3
"""Launch a command on a private X display, let it settle, then report the
memory and idle CPU of its whole process tree. Kills only the tree it started.

usage: measure.py <label> <display> <settle_s> <idle_s> -- cmd args...
"""
import json, os, signal, subprocess, sys, time

CLK = os.sysconf("SC_CLK_TCK")


def tree(root):
    kids = {}
    for p in os.listdir("/proc"):
        if not p.isdigit():
            continue
        try:
            ppid = int(open(f"/proc/{p}/stat").read().rsplit(")", 1)[1].split()[1])
        except OSError:
            continue
        kids.setdefault(ppid, []).append(int(p))
    out, todo = [], [root]
    while todo:
        p = todo.pop()
        out.append(p)
        todo += kids.get(p, [])
    return out


def kb(path, key):
    try:
        for line in open(path):
            if line.startswith(key):
                return int(line.split()[1])
    except OSError:
        pass
    return 0


def name(p):
    try:
        return open(f"/proc/{p}/comm").read().strip()
    except OSError:
        return "?"


def cpu(p):
    try:
        f = open(f"/proc/{p}/stat").read().rsplit(")", 1)[1].split()
        return (int(f[11]) + int(f[12])) / CLK
    except OSError:
        return 0.0


def snapshot(root):
    procs = {}
    for p in tree(root):
        procs[p] = {
            "name": name(p),
            "pss_mb": kb(f"/proc/{p}/smaps_rollup", "Pss:") / 1024,
            "anon_mb": kb(f"/proc/{p}/status", "RssAnon:") / 1024,
            "rss_mb": kb(f"/proc/{p}/status", "VmRSS:") / 1024,
            "threads": kb(f"/proc/{p}/status", "Threads:"),
            "cpu_s": cpu(p),
        }
    return procs


def main():
    label, display, settle, idle = sys.argv[1], sys.argv[2], float(sys.argv[3]), float(sys.argv[4])
    cmd = sys.argv[sys.argv.index("--") + 1 :]
    env = dict(os.environ, DISPLAY=display)
    env.pop("WAYLAND_DISPLAY", None)
    t0 = time.time()
    proc = subprocess.Popen(cmd, env=env, stdout=subprocess.DEVNULL, stderr=open(f"/mnt/Rusty/caskade-blitz-spike/out/{label}.stderr", "w"), start_new_session=True)
    time.sleep(settle)
    subprocess.run(["import", "-display", display, "-window", "root", f"/mnt/Rusty/caskade-blitz-spike/out/{label}.png"], stderr=subprocess.DEVNULL)
    a = snapshot(proc.pid)
    time.sleep(idle)
    b = snapshot(proc.pid)
    pids = tree(proc.pid)
    for p in reversed(pids):
        try:
            os.kill(p, signal.SIGTERM)
        except OSError:
            pass
    try:
        proc.wait(5)
    except subprocess.TimeoutExpired:
        for p in pids:
            try:
                os.kill(p, signal.SIGKILL)
            except OSError:
                pass
    per = []
    for p, s in b.items():
        idle_cpu = s["cpu_s"] - a.get(p, {"cpu_s": 0})["cpu_s"]
        per.append({**s, "pid": p, "idle_cpu_s_per_min": round(idle_cpu * 60 / idle, 3)})
    tot = {k: round(sum(x[k] for x in per), 1) for k in ("pss_mb", "anon_mb", "rss_mb", "threads")}
    tot["idle_cpu_s_per_min"] = round(sum(x["idle_cpu_s_per_min"] for x in per), 3)
    res = {"label": label, "settle_s": settle, "idle_s": idle, "total": tot,
           "procs": [{k: (round(v, 1) if isinstance(v, float) else v) for k, v in x.items()} for x in per]}
    print(json.dumps(res))
    with open(f"/mnt/Rusty/caskade-blitz-spike/out/{label}.json", "w") as fh:
        json.dump(res, fh, indent=1)


main()
