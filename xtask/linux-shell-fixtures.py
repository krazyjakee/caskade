#!/usr/bin/env python3
"""Build genuine makeself containers and a structural GOG/MojoSetup shell+ZIP."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import tempfile
import urllib.request
import zipfile

spec = importlib.util.spec_from_file_location('windows_builders', Path(__file__).with_name('windows-fixtures.py'))
b = importlib.util.module_from_spec(spec)
spec.loader.exec_module(b)
SOURCE = b.FIXTURES / 'src/linux-shell'


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--prepare', action='store_true')
    args = p.parse_args()
    pin = json.loads((SOURCE / 'builder.json').read_text())
    package = b.CACHE / 'downloads' / pin['name']
    if not package.exists():
        if not args.prepare: raise RuntimeError('missing makeself package; use --prepare')
        package.parent.mkdir(parents=True, exist_ok=True)
        data = urllib.request.urlopen(pin['url'], timeout=60).read()
        if hashlib.sha256(data).hexdigest() != pin['sha256']: raise RuntimeError('makeself SHA mismatch')
        package.write_bytes(data)
    if hashlib.sha256(package.read_bytes()).hexdigest() != pin['sha256']: raise RuntimeError('makeself SHA mismatch')
    b.run(['dpkg-deb', '-x', package, b.CACHE / 'local'])
    binary = b.FIXTURES / 'bin/linux-shell'; binary.mkdir(exist_ok=True)
    expected_dir = b.FIXTURES / 'expected/linux-shell'; expected_dir.mkdir(exist_ok=True)
    for id in ['makeself-generic', 'gog-linux-mojosetup']:
        with tempfile.TemporaryDirectory(prefix='caskade-shell-build-') as t:
            job = Path(t); payload = job / 'payload'; payload.mkdir()
            shutil.copyfile(b.FIXTURES / 'bin/fixture-game-linux-x86_64', payload / 'FixtureGame.x86_64')
            shutil.copyfile(b.FIXTURES / 'src/layouts/linux/start.sh', payload / 'start.sh')
            shutil.copyfile(SOURCE / 'gameinfo', payload / 'gameinfo')
            (payload / 'FixtureGame.x86_64').chmod(0o755); (payload / 'start.sh').chmod(0o755)
            expected = b.tree(payload)
            for entry in expected: entry['executable'] = entry['path'] in ['start.sh', 'FixtureGame.x86_64']
            output = binary / (id + '.sh')
            container = payload
            label = 'Caskade makeself fixture'
            if id.startswith('gog'):
                container = job / 'mojo'; container.mkdir()
                (container / 'mojosetup').write_text('#!/bin/sh\n# Structural MojoSetup marker fixture; static ZIP extraction is tested.\nexit 0\n')
                (container / 'mojosetup').chmod(0o755)
                label = 'GOG.com MojoSetup structural fixture'
            # makeself records its command line in the header: run it from the
            # repository root with relative paths so no local path is embedded.
            local = b.CACHE.relative_to(b.ROOT) / 'local/usr'
            b.run(['sh', local / 'bin/makeself', '--header', local / 'share/makeself/makeself-header.sh',
                   '--gzip', '--nox11', '--notemp', '--quiet', container, output.relative_to(b.ROOT), label,
                   './mojosetup' if id.startswith('gog') else './start.sh'], cwd=b.ROOT)
            if id.startswith('gog'):
                with zipfile.ZipFile(output, 'a', zipfile.ZIP_DEFLATED) as archive:
                    for file in sorted(payload.iterdir()): archive.write(file, 'data/noarch/' + file.name)
            # Prove the payload actually starts, independently of shell parsing.
            token = job / 'token'; b.run([payload / 'start.sh', '--token', token])
            if not token.read_text().strip().endswith('/FixtureGame.x86_64'): raise RuntimeError('launch token mismatch')
            tree_name = f'linux-shell/{id}.tree.json'
            (b.FIXTURES / 'expected' / tree_name).write_text(json.dumps(expected, indent=2) + '\n')
            q = json.dumps
            lines = ['schema_version = 1', f'id = {q(id)}', 'title = "Caskade Fixture Game"',
                     f'layout = {q(id)}', f'format = {q("gog-sh" if id.startswith("gog") else "makeself")}',
                     'platform = "linux"', f'input = "bin/linux-shell/{id}.sh"', f'parts = ["bin/linux-shell/{id}.sh"]',
                     f'tree = {q(tree_name)}', 'launch_target = "start.sh"']
            for host in ['windows', 'linux', 'macos']:
                lines += ['', f'[host.{host}]', 'outcome = "Installed"', 'strategy = "Extract"', 'runtime = "Native"']
            (expected_dir / (id + '.toml')).write_text('\n'.join(lines) + '\n')
            print(f'{id}: {output.stat().st_size} bytes; real payload --token passed')


if __name__ == '__main__': main()
