#!/usr/bin/env python3
"""Build genuine Debian packages; Rust rpm-rs builds the matching RPM payload."""
import argparse
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

spec = importlib.util.spec_from_file_location('builders', Path(__file__).with_name('windows-fixtures.py'))
b = importlib.util.module_from_spec(spec)
spec.loader.exec_module(b)


def metadata(fixtures, ident, extension, expected):
    q = json.dumps
    directory = fixtures / 'expected/packages'
    directory.mkdir(parents=True, exist_ok=True)
    tree = f'packages/{ident}.tree.json'
    (fixtures / 'expected' / tree).write_text(json.dumps(expected, indent=2) + '\n')
    lines = ['schema_version = 1', f'id = {q(ident)}', 'title = "Caskade Fixture Game"',
             'layout = "linux-package"', f'format = {q(extension)}', 'platform = "linux"',
             f'input = "bin/packages/{ident}.{extension}"', f'parts = ["bin/packages/{ident}.{extension}"]',
             f'tree = {q(tree)}', 'launch_target = "opt/caskade-fixture/start.sh"']
    for host in ['windows', 'linux', 'macos']:
        lines += ['', f'[host.{host}]', f'outcome = {q("Installed" if host == "linux" else "Unsupported")}',
                  f'strategy = {q("Delegate" if ident in ["deb-missing-dep", "deb-absolute-path"] else "Extract")}', 'runtime = "Native"']
    (directory / (ident + '.toml')).write_text('\n'.join(lines) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument('--rpm-expectation-only', action='store_true')
    args = parser.parse_args()
    fixtures = args.root / 'crates/caskade-installer/tests/fixtures'
    binary = fixtures / 'bin/packages'; binary.mkdir(parents=True, exist_ok=True)
    for ident in (['rpm-self-contained'] if args.rpm_expectation_only else ['deb-self-contained', 'deb-missing-dep', 'deb-absolute-path']):
        with tempfile.TemporaryDirectory(prefix='caskade-packages-') as temporary:
            stage = Path(temporary)
            game = stage / 'opt/caskade-fixture'; game.mkdir(parents=True)
            shutil.copyfile(fixtures / 'bin/fixture-game-linux-x86_64', game / 'FixtureGame.x86_64')
            shutil.copyfile(fixtures / 'src/layouts/linux/start.sh', game / 'start.sh')
            if ident == 'deb-absolute-path':
                (game / 'start.sh').write_text('#!/bin/sh\nexec /opt/caskade-fixture/FixtureGame.x86_64 "$@"\n')
            for name in ['FixtureGame.x86_64', 'start.sh']: (game / name).chmod(0o755)
            shutil.copyfile(fixtures / 'src/packages/icon.svg', game / 'icon.svg')
            desktop = stage / 'usr/share/applications'; desktop.mkdir(parents=True)
            shutil.copyfile(fixtures / 'src/packages/fixture.desktop', desktop / 'caskade-fixture.desktop')
            expected = b.tree(stage)
            for entry in expected: entry['executable'] = entry['path'] in ['opt/caskade-fixture/start.sh', 'opt/caskade-fixture/FixtureGame.x86_64']
            if not args.rpm_expectation_only:
                control = stage / 'DEBIAN'; control.mkdir()
                text = (fixtures / 'src/packages/control').read_text().replace('@NAME@', 'caskade-' + ident)
                if ident == 'deb-missing-dep': text += 'Depends: hello\n'
                if ident == 'deb-absolute-path': text += 'X-Caskade-Absolute-Paths: yes\n'
                (control / 'control').write_text(text)
                shutil.copyfile(fixtures / 'src/packages/postinst', control / 'postinst'); (control / 'postinst').chmod(0o755)
                shutil.copyfile(fixtures / 'src/packages/postrm', control / 'postrm'); (control / 'postrm').chmod(0o755)
                command = ['dpkg-deb', '--root-owner-group', '-Zgzip', '-z6', '--build', str(stage), str(binary / (ident + '.deb'))]
                subprocess.run(command, check=True, env={**__import__('os').environ, 'SOURCE_DATE_EPOCH': '1700000000'})
            metadata(fixtures, ident, 'rpm' if args.rpm_expectation_only else 'deb', expected)


if __name__ == '__main__': main()
