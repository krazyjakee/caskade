#!/usr/bin/env python3
"""#31 spike: install genuine Inno/NSIS/MSI fixtures under wine through a
symlink alias and look for real-path references in files and the registry."""
import os, shutil, subprocess, sys, json
from pathlib import Path
FIX = Path(sys.argv[1])
WORK = Path('/tmp/eng5-spike'); shutil.rmtree(WORK, ignore_errors=True)
PREFIX = WORK / 'prefix'
env = os.environ | {'WINEPREFIX': str(PREFIX), 'WINEDEBUG': '-all', 'WINEDLLOVERRIDES': 'mscoree,mshtml='}
def wine(*args, timeout=300):
    r = subprocess.run(['xvfb-run', '-a', 'wine', *map(str, args)], env=env, capture_output=True, timeout=timeout)
    return r.returncode
subprocess.run(['xvfb-run', '-a', 'wineboot', '-i'], env=env, capture_output=True, timeout=600)
subprocess.run(['wineserver', '-w'], env=env, timeout=600)
def win(p): return 'Z:' + str(p).replace('/', '\\')
needles = []
def hits(blob, real):
    found = []
    for form in {real, real.replace('/', '\\'), win(real), win(real).lower(), win(real).upper()}:
        for enc in ('utf-8', 'utf-16-le'):
            if form.encode(enc) in blob: found.append(f'{form} ({enc})')
    # registry escapes backslashes
    reg = win(real).replace('\\', '\\\\')
    if reg.encode() in blob or reg.lower().encode() in blob: found.append(f'{reg} (reg)')
    return found
cases = [
    ('inno60-ini', 'inno60-ini/setup.exe', lambda d: ['/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/SP-','/CURRENTUSER', f'/DIR={d}']),
    ('inno60-registry', 'inno60-registry/setup.exe', lambda d: ['/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/SP-','/ALLUSERS', f'/DIR={d}']),
    ('nsis3-unicode', 'nsis3-unicode-nonsolid/setup.exe', lambda d: ['/S', f'/D={d}']),
    ('msi-embedded', 'msi-embedded/setup.msi', None),
]
results = {}
for name, rel, args in cases:
    real = WORK / 'real' / name; real.mkdir(parents=True)
    alias = WORK / 'alias' / name; alias.parent.mkdir(parents=True, exist_ok=True); alias.symlink_to(real)
    before = {p: p.read_bytes() for p in PREFIX.glob('*.reg')}
    if args:
        code = wine(FIX / rel, *args(win(alias)))
    else:
        code = wine('msiexec', '/i', win(FIX / rel), '/qn', f'INSTALLDIR={win(alias)}\\', f'TARGETDIR={win(alias)}\\')
    subprocess.run(['wineserver', '-w'], env=env, timeout=600)
    files = sorted(str(p.relative_to(real)) for p in real.rglob('*') if p.is_file())
    refs, alias_refs = {}, {}
    scan = [p for p in real.rglob('*') if p.is_file()] + list((PREFIX / 'drive_c').rglob('*.lnk')) + list((PREFIX / 'drive_c').rglob('*.ini')) + list(PREFIX.glob('*.reg'))
    for p in scan:
        if p.stat().st_size > 64 << 20: continue
        blob = p.read_bytes()
        if (h := hits(blob, str(real))): refs[str(p)] = h
        if (h := hits(blob, str(alias))): alias_refs[str(p)] = h
    results[name] = dict(exit=code, installed_files=files, real_path_refs=refs, alias_refs=alias_refs)
    print(name, json.dumps(results[name], indent=1), flush=True)
(WORK / 'results.json').write_text(json.dumps(results, indent=1))
