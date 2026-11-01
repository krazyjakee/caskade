#!/usr/bin/env python3
"""Independent maintainer checks against real builders/extractors, outside CI checks.

Default verifies 7zz-extractable NSIS/SFX/ZIP and msiextract-extractable MSI.
--wine-inno also runs our compiled Inno installers in the private builder Wine
prefix. This is Linux/Wine evidence, not native Windows/macOS T4 coverage (#22).
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import tomllib

spec = importlib.util.spec_from_file_location('builders', Path(__file__).with_name('windows-fixtures.py'))
b = importlib.util.module_from_spec(spec)
spec.loader.exec_module(b)


def compare(root, expected, ignored=()):
    actual = b.tree(root)
    actual = [entry for entry in actual if entry['path'] not in ignored]
    if actual != expected:
        wanted = {entry['path']: entry for entry in expected}
        found = {entry['path']: entry for entry in actual}
        differences = [path for path in sorted(wanted.keys() | found.keys()) if wanted.get(path) != found.get(path)]
        raise RuntimeError(f'golden tree mismatch: {differences}')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--wine-inno', action='store_true')
    p.add_argument('--only', default='*', help='comma-separated fixture ID glob patterns')
    args = p.parse_args()
    b.ONLY = args.only.split(',')
    counts = {}
    for meta in sorted(b.EXPECTED.glob('*.toml')):
        fixture = tomllib.loads(meta.read_text())
        if not b.selected(fixture['id']):
            continue
        family = fixture['format']
        if family == 'inno' and not args.wine_inno:
            continue
        with tempfile.TemporaryDirectory(prefix='caskade-windows-verify-') as temp:
            out = Path(temp) / 'game'
            source = b.FIXTURES / fixture['input']
            expected = json.loads((b.FIXTURES / 'expected' / fixture['tree']).read_text())
            ignored = []
            if family == 'inno':
                flags = ['/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', '/DIR=' + b.winpath(out)]
                if fixture['windows'].get('password'):
                    flags += ['/PASSWORD=' + fixture['windows']['password']]
                b.wine([source, *flags])
                ignored = [v.split(':')[0].split('=')[0] for v in fixture['windows']['native_generated']]
                for name in ignored:
                    if name.lower().endswith(('.exe', '.dat')):
                        if not (out / name).is_file():
                            raise RuntimeError(f'{fixture["id"]}: missing generated {name}')
                        continue
                    data = (out / name).read_text()
                    if b.winpath(out) not in data:
                        raise RuntimeError(f'{fixture["id"]}: generated path missing in {name}')
                if fixture['id'] in ('legacy-registry-path', 'gog-inno-unity'):
                    token = Path(temp) / 'token.txt'
                    b.wine([out / 'FixtureGame.exe', '--token', b.winpath(token), '--self-check'])
                    if token.read_text().strip() != 'ok':
                        raise RuntimeError('fixture game --self-check failed')
            elif family == 'msi':
                out.mkdir()
                env = os.environ | {'LD_LIBRARY_PATH': str(b.CACHE / 'local/usr/lib/x86_64-linux-gnu')}
                b.run([b.CACHE / 'local/usr/bin/msiextract', '-C', out, source], env=env)
                # MSI target directories are installation metadata, not game-root
                # content. Locate the INSTALLDIR from the actual extracted tree.
                matches = list(out.rglob('FixtureGame.exe'))
                if len(matches) != 1: raise RuntimeError(f'{fixture["id"]}: missing MSI payload')
                out = matches[0].parent
            else:
                b.run([b.sevenzip(), 'x', '-y', '-o' + str(out), source])
            compare(out, expected, ignored)
            counts[family] = counts.get(family, 0) + 1
            print(f'{fixture["id"]}: real {family} payload matches golden', flush=True)
    print(counts)


if __name__ == '__main__':
    main()
