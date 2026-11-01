#!/usr/bin/env python3
"""Build real ISO9660 PC/PS1-style discs using genisoimage; no copyrighted data."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
FIX = ROOT / 'crates/caskade-installer/tests/fixtures'
SRC = FIX / 'src/disc'
BIN = FIX / 'bin/disc'
EXPECTED = FIX / 'expected/disc'

def tree(directory):
    entries = []
    for p in sorted(directory.rglob('*')):
        rel = p.relative_to(directory).as_posix()
        entry = dict(path=rel, kind='directory' if p.is_dir() else 'file', size=0 if p.is_dir() else p.stat().st_size, executable=False)
        if p.is_file():
            entry['sha256'] = hashlib.sha256(p.read_bytes()).hexdigest()
        entries.append(entry)
    return entries

def expected(name, platform, launch, parts, golden):
    hosts = '\n'.join(f'[host.{host}]\noutcome = "{outcome}"\nstrategy = "{strategy}"\nruntime = "{runtime}"\n' for host, outcome, strategy, runtime in (
        ('windows', 'handoff' if platform == 'sony_playstation' else 'installed', 'handoff' if platform == 'sony_playstation' else 'run_silent', 'none' if platform == 'sony_playstation' else 'native_windows'),
        ('linux', 'handoff' if platform == 'sony_playstation' else 'installed', 'handoff' if platform == 'sony_playstation' else 'extract', 'none'),
        ('macos', 'handoff' if platform == 'sony_playstation' else 'installed', 'handoff' if platform == 'sony_playstation' else 'extract', 'none')))
    text = f'''schema_version = 1
id = "{name}"
title = "Caskade Disc Fixture"
layout = "disc"
format = "{'cue' if platform == 'sony_playstation' else 'iso'}"
platform = "{platform}"
input = "{parts[0]}"
parts = {json.dumps(parts)}
tree = "disc/{name}.tree.json"
launch_target = "{launch}"
{hosts}'''
    (EXPECTED / f'{name}.toml').write_text(text)
    (EXPECTED / f'{name}.tree.json').write_text(json.dumps(golden, indent=2) + '\n')

def build():
    tool = shutil.which('genisoimage')
    if not tool:
        raise SystemExit('genisoimage 1.1.11 is required for maintainer rebuilds; committed tests need no ISO builder')
    version = subprocess.check_output([tool, '--version'], stderr=subprocess.STDOUT).decode().strip()
    if not version.startswith('genisoimage 1.1.11 '):
        raise SystemExit(f'unsupported ISO builder version: {version}')
    BIN.mkdir(parents=True, exist_ok=True)
    EXPECTED.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='caskade-disc-') as temp:
        work = Path(temp)
        pc = work / 'pc'; pc.mkdir()
        shutil.copyfile(FIX / 'bin/windows/inno60-basic/setup.exe', pc / 'setup.exe')
        shutil.copyfile(SRC / 'autorun.inf', pc / 'autorun.inf')
        # Golden installation is the actual nested Inno fixture payload, not the disc's installer files.
        golden = json.loads((FIX / 'expected/windows/inno60-basic.tree.json').read_text())
        for name, extra in [('iso-pc-disc', []), ('udf-pc-disc', ['-udf'])]:
            subprocess.run([tool, '-quiet', '-R', '-J', *extra, '-V', 'CASKADE_PC', '-o', str(BIN / f'{name}.iso'), str(pc)], check=True)
            expected(name, 'windows', 'FixtureGame.exe', [f'bin/disc/{name}.iso'], golden)
        ps = work / 'ps1'; ps.mkdir()
        shutil.copyfile(SRC / 'SYSTEM.CNF', ps / 'SYSTEM.CNF')
        # Original test-only PS-X EXE header and inert bytes; no Sony/game content.
        exe = bytearray(4096); exe[:8] = b'PS-X EXE'
        exe[0x10:0x14] = (0x80010000).to_bytes(4, 'little')
        exe[0x18:0x1c] = (0x80010000).to_bytes(4, 'little')
        exe[0x1c:0x20] = (2048).to_bytes(4, 'little')
        exe[2048:2056] = bytes.fromhex('0000000000000010')
        (ps / 'CASKADE.EXE').write_bytes(exe)
        subprocess.run([tool, '-quiet', '-iso-level', '1', '-V', 'CASKADE_PS1', '-o', str(BIN / 'ps1-cue-bin.bin'), str(ps)], check=True)
        shutil.copyfile(SRC / 'ps1-cue-bin.cue', BIN / 'ps1-cue-bin.cue')
        expected('ps1-cue-bin', 'sony_playstation', 'CASKADE.EXE', ['bin/disc/ps1-cue-bin.cue', 'bin/disc/ps1-cue-bin.bin'], tree(ps))
    print('built genuine ISO/UDF PC discs and PS1-style ISO9660 MODE1/2048 CUE/BIN')

if __name__ == '__main__':
    build()
