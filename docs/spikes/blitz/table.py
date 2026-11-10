import json, sys
for label in sys.argv[1:]:
    r = json.load(open(f"out/{label}.json")); t = r["total"]
    procs = " | ".join(f"{p['name']}:{p['anon_mb']}a/{p['pss_mb']}p/{p['idle_cpu_s_per_min']}c" for p in r["procs"])
    print(f"{label:<24} anon={t['anon_mb']:>6} pss={t['pss_mb']:>6} rss={t['rss_mb']:>6} thr={t['threads']:>4} idle={t['idle_cpu_s_per_min']:>6}  {procs}")
