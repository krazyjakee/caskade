#!/usr/bin/env python3
"""Build the genuine appimage-indie fixture with pinned appimagetool + type2 runtime."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import struct
import tempfile
import urllib.request
import zlib

spec = importlib.util.spec_from_file_location('windows_builders', Path(__file__).with_name('windows-fixtures.py'))
b = importlib.util.module_from_spec(spec)
spec.loader.exec_module(b)
SOURCE = b.FIXTURES / 'src/appimage'
ID = 'appimage-indie'
OUTPUT = 'Indie_Game-x86_64.AppImage'


def pinned(pin, prepare):
    path = b.CACHE / 'downloads' / pin['name']
    if not path.exists():
        if not prepare: raise RuntimeError(f"missing {pin['name']}; use --prepare")
        path.parent.mkdir(parents=True, exist_ok=True)
        data = urllib.request.urlopen(pin['url'], timeout=120).read()
        if hashlib.sha256(data).hexdigest() != pin['sha256']: raise RuntimeError(f"{pin['name']} SHA mismatch")
        path.write_bytes(data)
    if hashlib.sha256(path.read_bytes()).hexdigest() != pin['sha256']: raise RuntimeError(f"{pin['name']} SHA mismatch")
    return path


def png(size, rgba):
    """Deterministic solid-colour RGBA PNG."""
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    rows = b''.join(b'\0' + bytes(rgba) * size for _ in range(size))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', size, size, 8, 6, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(rows, 9)) + chunk(b'IEND', b''))


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--prepare', action='store_true')
    args = p.parse_args()
    pins = json.loads((SOURCE / 'builder.json').read_text())
    tool = pinned(pins['appimagetool'], args.prepare)
    runtime = pinned(pins['runtime'], args.prepare)
    extracted = b.CACHE / 'appimagetool-1.9.1'
    if not (extracted / 'AppRun').exists():
        shutil.rmtree(extracted, ignore_errors=True)
        with tempfile.TemporaryDirectory(prefix='caskade-appimagetool-') as t:
            copy = Path(t) / 'appimagetool.AppImage'
            shutil.copyfile(tool, copy); copy.chmod(0o755)
            # Self-extraction avoids FUSE; the tool's own squashfs is unpacked, not run.
            b.run([copy, '--appimage-extract'], cwd=t)
            shutil.move(Path(t) / 'squashfs-root', extracted)
    binary = b.FIXTURES / 'bin/appimage'; binary.mkdir(exist_ok=True)
    expected_dir = b.FIXTURES / 'expected/appimage'; expected_dir.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='caskade-appimage-build-') as t:
        job = Path(t); appdir = job / 'Indie.AppDir'
        (appdir / 'usr/bin').mkdir(parents=True)
        (appdir / 'usr/share/metainfo').mkdir(parents=True)
        shutil.copyfile(b.FIXTURES / 'bin/fixture-game-linux-x86_64', appdir / 'usr/bin/indie-game')
        (appdir / 'usr/bin/indie-game').chmod(0o755)
        os.symlink('usr/bin/indie-game', appdir / 'AppRun')
        shutil.copyfile(SOURCE / 'io.caskade.IndieGame.desktop', appdir / 'io.caskade.IndieGame.desktop')
        shutil.copyfile(SOURCE / 'io.caskade.IndieGame.appdata.xml',
                        appdir / 'usr/share/metainfo/io.caskade.IndieGame.appdata.xml')
        (appdir / 'indie-game.png').write_bytes(png(16, (20, 80, 160, 255)))
        os.symlink('indie-game.png', appdir / '.DirIcon')
        output = job / OUTPUT
        env = os.environ | {'ARCH': 'x86_64', 'SOURCE_DATE_EPOCH': '1700000000', 'NO_APPSTREAM': '1'}
        b.run([extracted / 'AppRun', '--no-appstream', '--runtime-file', runtime, appdir, output], cwd=job, env=env)
        data = output.read_bytes()
        if data[:4] != b'\x7fELF' or data[8:11] != b'AI\x02': raise RuntimeError('not a type 2 AppImage')
        if not data.startswith(runtime.read_bytes()[:4096]): raise RuntimeError('runtime prefix mismatch')
        # Prove the payload starts through the image's own runtime, without FUSE.
        output.chmod(0o755)
        token = job / 'token'
        b.run([output, '--token', token], cwd=job, env=os.environ | {'APPIMAGE_EXTRACT_AND_RUN': '1'})
        if not token.read_text().strip().endswith('indie-game'): raise RuntimeError('launch token mismatch')
        shutil.copyfile(output, binary / OUTPUT)
        (binary / OUTPUT).chmod(0o644)
    data = (binary / OUTPUT).read_bytes()
    tree = [dict(path=OUTPUT, kind='file', size=len(data), sha256=hashlib.sha256(data).hexdigest(), executable=True)]
    tree_name = f'appimage/{ID}.tree.json'
    (b.FIXTURES / 'expected' / tree_name).write_text(json.dumps(tree, indent=2) + '\n')
    q = json.dumps
    lines = ['schema_version = 1', f'id = {q(ID)}', 'title = "Indie Game"', f'layout = {q(ID)}',
             'format = "appimage"', 'platform = "linux"', f'input = "bin/appimage/{OUTPUT}"',
             f'parts = ["bin/appimage/{OUTPUT}"]', f'tree = {q(tree_name)}', f'launch_target = {q(OUTPUT)}',
             '', '[host.linux]', 'outcome = "Installed"', 'strategy = "Copy"', 'runtime = "Native"']
    (expected_dir / f'{ID}.toml').write_text('\n'.join(lines) + '\n')
    print(f'{ID}: {len(data)} bytes; runtime-launched payload --token passed')


if __name__ == '__main__': main()
