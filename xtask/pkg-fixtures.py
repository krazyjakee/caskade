#!/usr/bin/env python3
"""Build real cpio/Bom/XAR flat macOS package fixtures without Apple tools."""
import argparse
import gzip
import hashlib
import io
import json
import lzma
import os
from pathlib import Path
import shutil
import stat
import struct
import subprocess
import tarfile
import tempfile
import urllib.request
from xml.etree import ElementTree as ET
import zlib

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / 'crates/caskade-installer/tests/fixtures'
SOURCE = FIXTURES / 'src/macos-pkg'
CACHE = ROOT / 'target/pkg-builders'


def run(args, **kwargs):
    return subprocess.run([str(a) for a in args], check=True, **kwargs)


def tools(prepare):
    pin = json.loads((SOURCE / 'builder.json').read_text())
    CACHE.mkdir(parents=True, exist_ok=True)
    archive = CACHE / 'bomutils.tar.gz'
    if not archive.exists():
        if not prepare:
            raise RuntimeError('missing pinned bomutils; use --prepare')
        archive.write_bytes(urllib.request.urlopen(pin['bomutils_url'], timeout=60).read())
    if hashlib.sha256(archive.read_bytes()).hexdigest() != pin['bomutils_sha256']:
        raise RuntimeError('bomutils download SHA mismatch')
    source = CACHE / ('bomutils-' + pin['bomutils_commit'])
    if not source.exists():
        with tarfile.open(archive) as tar:
            tar.extractall(CACHE, filter='data')
    binary = source / 'build/bin/mkbom'
    if not binary.exists() or not (binary.parent / 'dumpbom').exists():
        # The parent cargo xtask command holds the machine-wide heavy lane.
        run(['make', '-C', source, '-j2', 'mkbom', 'dumpbom'])
    return binary


def cpio(directory, kind):
    paths = ['.'] + sorted('./' + p.relative_to(directory).as_posix() for p in directory.rglob('*'))
    return run(['cpio', '--quiet', '--create', '--null', '--reproducible', '--owner=0:0', '--format=' + kind],
               cwd=directory, input=('\0'.join(paths) + '\0').encode(), stdout=subprocess.PIPE).stdout


def pbzx(raw):
    block = 512  # Deliberately several chunks, including raw and XZ chunks.
    output = bytearray(b'pbzx' + struct.pack('>Q', block))
    for index, start in enumerate(range(0, len(raw), block)):
        chunk = raw[start:start + block]
        packed = chunk if index % 2 == 0 else lzma.compress(chunk, format=lzma.FORMAT_XZ, check=lzma.CHECK_CRC64)
        if len(packed) == len(chunk):
            packed = chunk
        output += struct.pack('>QQ', len(chunk), len(packed)) + packed
    return bytes(output)


def xar(files):
    """XAR v1: zlib TOC, SHA256(compressed TOC), checked disjoint heap members."""
    root = ET.Element('xar'); toc = ET.SubElement(root, 'toc')
    checksum = ET.SubElement(toc, 'checksum', style='sha256')
    ET.SubElement(checksum, 'offset').text = '0'; ET.SubElement(checksum, 'size').text = '32'
    heap = bytearray(32); directories = {}; identifier = 0
    for path, raw in sorted(files.items()):
        parent = toc
        segments = path.split('/')
        for index, name in enumerate(segments[:-1]):
            key = '/'.join(segments[:index + 1])
            if key not in directories:
                identifier += 1
                directory = ET.SubElement(parent, 'file', id=str(identifier))
                ET.SubElement(directory, 'name').text = name
                ET.SubElement(directory, 'type').text = 'directory'
                directories[key] = directory
            parent = directories[key]
        identifier += 1
        file = ET.SubElement(parent, 'file', id=str(identifier))
        ET.SubElement(file, 'name').text = segments[-1]
        ET.SubElement(file, 'type').text = 'file'
        ET.SubElement(file, 'mode').text = '0644'
        ET.SubElement(file, 'uid').text = '0'; ET.SubElement(file, 'gid').text = '0'
        data = ET.SubElement(file, 'data')
        packed = zlib.compress(raw, 6) if path.endswith(('PackageInfo', 'Bom')) else raw
        ET.SubElement(data, 'length').text = str(len(packed))
        ET.SubElement(data, 'offset').text = str(len(heap))
        ET.SubElement(data, 'size').text = str(len(raw))
        ET.SubElement(data, 'encoding', style='application/x-gzip' if packed is not raw else 'application/octet-stream')
        ET.SubElement(data, 'archived-checksum', style='sha256').text = hashlib.sha256(packed).hexdigest()
        ET.SubElement(data, 'extracted-checksum', style='sha256').text = hashlib.sha256(raw).hexdigest()
        heap += packed
    xml = ET.tostring(root, encoding='utf-8', xml_declaration=True)
    packed = zlib.compress(xml, 9); heap[:32] = hashlib.sha256(packed).digest()
    return struct.pack('>4sHHQQI', b'xar!', 28, 1, len(packed), len(xml), 3) + packed + heap


def tree(directory):
    result = []
    for path in sorted(directory.rglob('*')):
        metadata = path.lstat(); name = path.relative_to(directory).as_posix()
        if stat.S_ISLNK(metadata.st_mode):
            entry = dict(path=name, kind='symlink', size=0, executable=False, symlink_target=os.readlink(path))
        elif path.is_dir():
            entry = dict(path=name, kind='directory', size=0, executable=False)
        else:
            entry = dict(path=name, kind='file', size=metadata.st_size,
                         sha256=hashlib.sha256(path.read_bytes()).hexdigest(), executable=bool(metadata.st_mode & 0o111))
        result.append(entry)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__); parser.add_argument('--prepare', action='store_true')
    args = parser.parse_args(); mkbom = tools(args.prepare)
    binary = FIXTURES / 'bin/macos-pkg'; binary.mkdir(parents=True, exist_ok=True)
    expected = FIXTURES / 'expected/macos-pkg'; expected.mkdir(parents=True, exist_ok=True)
    ids = ['mac-pkg', 'mac-pkg-pbzx', 'mac-pkg-unused-scripts', 'mac-pkg-scripts', 'mac-pkg-system']
    epoch = json.loads((SOURCE / 'builder.json').read_text())['epoch']
    for id in ids:
        with tempfile.TemporaryDirectory(prefix='caskade-pkg-build-') as temp:
            job = Path(temp); stage = job / 'stage'; stage.mkdir()
            with tarfile.open(FIXTURES / 'bin/macos-app.tar.gz') as tar:
                tar.extractall(stage, filter='data')
            for path in [stage, *stage.rglob('*')]:
                os.utime(path, (epoch, epoch), follow_symlinks=False)
            bom = job / 'Bom'
            run([mkbom, '-u', '0', '-g', '0', stage, bom])
            if not bom.read_bytes().startswith(b'BOMStore'):
                raise RuntimeError('invalid native Bill of Materials')
            listing = run([mkbom.parent / 'dumpbom', bom], stdout=subprocess.PIPE).stdout
            if b'FixtureGame' not in listing or b'Versions' not in listing:
                raise RuntimeError('Bill of Materials path tree missing app/framework entries')
            cpio_raw = cpio(stage, 'newc' if id == 'mac-pkg-pbzx' else 'odc')
            payload = pbzx(cpio_raw) if id == 'mac-pkg-pbzx' else gzip.compress(cpio_raw, compresslevel=6, mtime=epoch)
            location = '/Library/CaskadePkgFixture' if id == 'mac-pkg-system' else '/Applications'
            info = ET.Element('pkg-info', {'format-version': '2', 'identifier': 'org.caskade.fixture.' + id,
                                          'version': '1.0', 'install-location': location, 'auth': 'root',
                                          'relocatable': 'false', 'overwrite-permissions': 'true'})
            ET.SubElement(info, 'payload', {'numberOfFiles': str(len(tree(stage))), 'installKBytes': '16'})
            if id == 'mac-pkg-scripts':
                scripts = ET.SubElement(info, 'scripts'); ET.SubElement(scripts, 'postinstall', file='./postinstall')
            prefix = 'FixtureGame.pkg/' if id == 'mac-pkg-pbzx' else ''
            files = {prefix + 'Bom': bom.read_bytes(), prefix + 'Payload': payload,
                     prefix + 'PackageInfo': ET.tostring(info, encoding='utf-8', xml_declaration=True)}
            if id in ['mac-pkg-scripts', 'mac-pkg-unused-scripts']:
                scripts = job / 'scripts'; scripts.mkdir()
                (scripts / 'postinstall').write_text('#!/bin/sh\n# Original fixture script; never runs during expansion.\nexit 0\n')
                (scripts / 'postinstall').chmod(0o755); os.utime(scripts / 'postinstall', (epoch, epoch)); os.utime(scripts, (epoch, epoch))
                files[prefix + 'Scripts'] = gzip.compress(cpio(scripts, 'odc'), mtime=epoch)
            output = binary / (id + '.pkg'); output.write_bytes(xar(files))
            golden = tree(stage)
            (expected / (id + '.tree.json')).write_text(json.dumps(golden, indent=2) + '\n')
            q = json.dumps
            lines = ['schema_version = 1', f'id = {q(id)}', 'title = "Fixture Game"', 'layout = "macos-pkg"',
                     'format = "pkg"', 'platform = "macos"', f'input = "bin/macos-pkg/{id}.pkg"',
                     f'parts = ["bin/macos-pkg/{id}.pkg"]', f'tree = "macos-pkg/{id}.tree.json"',
                     'launch_target = "FixtureGame.app/Contents/MacOS/FixtureGame"']
            for host in ['windows', 'linux', 'macos']:
                delegate = id in ['mac-pkg-system', 'mac-pkg-scripts']
                strategy = 'Delegate' if delegate else 'Extract'
                outcome = 'Delegated' if delegate and host == 'macos' else ('Unsupported' if delegate else 'Installed')
                lines += ['', f'[host.{host}]', f'outcome = {q(outcome)}', f'strategy = {q(strategy)}', 'runtime = "Native"']
            (expected / (id + '.toml')).write_text('\n'.join(lines) + '\n')
            print(f'{id}: {output.stat().st_size} bytes; real cpio + Bom + checksummed XAR')


if __name__ == '__main__':
    main()
