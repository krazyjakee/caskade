#!/usr/bin/env python3
"""Generate synthetic discovery machines from reviewable sources; no live data."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import shutil
import sqlite3
import struct
import tempfile
import uuid
from windows_registry import registry

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = Path('crates/caskade-discovery/tests/fixtures')
MAX_FILE = 2_000_000


def confined(root, relative):
    path = PurePosixPath(relative)
    if not path.parts or path.is_absolute() or any(p in ('..', '.') for p in path.parts) or '\\' in relative or ':' in relative:
        raise ValueError(f'unsafe fixture path: {relative!r}')
    result = root.joinpath(*path.parts)
    # Inputs and outputs are regular files; never dereference a fixture link.
    if any(parent.is_symlink() for parent in [result, *result.parents]):
        raise ValueError(f'fixture path traverses a link: {relative}')
    return result


def cstring(value):
    if '\0' in value:
        raise ValueError('binary VDF strings cannot contain NUL')
    return value.encode('utf-8') + b'\0'


def vdf(entries, table=None):
    """Valve binary KeyValues: explicit ordered entries preserve duplicate keys."""
    out = bytearray()
    tags = {'object': 0, 'string': 1, 'int32': 2, 'float32': 3, 'uint64': 7}
    for entry in entries:
        kind, value = entry['type'], entry['value']
        key = cstring(entry['key']) if table is None else struct.pack('<I', table.index(entry['key']))
        out += bytes([tags[kind]]) + key
        if kind == 'object': out += vdf(value, table)
        elif kind == 'string': out += cstring(value)
        elif kind == 'int32': out += struct.pack('<i', value)
        elif kind == 'float32': out += struct.pack('<f', value)
        else: out += struct.pack('<Q', value)
    return out + b'\x08'


def appinfo(spec):
    version = spec['version']
    if version not in (28, 29): raise ValueError('unsupported appinfo version')
    table = []
    def names(entries):
        for entry in entries:
            if entry['key'] not in table: table.append(entry['key'])
            if entry['type'] == 'object': names(entry['value'])
    for app in spec['apps']: names(app['entries'])
    entries = bytearray()
    for app in spec['apps']:
        data = vdf(app['entries'], table if version == 29 else None)
        digest = hashlib.sha1(data, usedforsecurity=False).digest()
        header = struct.pack('<IIQ20sI20s', 2, 1700000000, 0, digest, 1, digest)
        entries += struct.pack('<II', app['app_id'], len(header) + len(data)) + header + data
    entries += struct.pack('<I', 0)
    header = struct.pack('<II', 0x07564400 | (0x28 if version == 28 else 0x29), 1)
    if version == 29:
        header += struct.pack('<Q', 16 + len(entries))
        entries += struct.pack('<I', len(table)) + b''.join(cstring(name) for name in table)
    return header + entries


def lnk(spec):
    """MS-SHLLINK header, Unicode StringData and optional KnownFolderDataBlock."""
    fields = [('description', 4), ('relative_path', 0x08), ('working_dir', 0x10), ('args', 0x20), ('icon', 0x40)]
    flags = 0x80 | sum(flag for name, flag in fields if name in spec)
    if 'target' in spec: flags |= 2
    if 'known_folder' in spec: flags |= 1
    header = bytearray(76)
    struct.pack_into('<I', header, 0, 76)
    header[4:20] = uuid.UUID('00021401-0000-0000-c000-000000000046').bytes_le
    struct.pack_into('<I', header, 20, flags)
    struct.pack_into('<I', header, 60, 1)  # SW_SHOWNORMAL
    result = header
    if 'known_folder' in spec:
        # A root-folder Shell Item followed by the terminal ItemID. The extra
        # block references its first item; there is no arbitrary opaque blob.
        item = struct.pack('<HBB', 20, 0x1F, 0x50) + uuid.UUID(spec['known_folder']).bytes_le + b'\0\0'
        result += struct.pack('<H', len(item)) + item
    if 'target' in spec:
        volume = struct.pack('<IIII', 20, 3, 0, 16) + b'\0' * 4
        base = spec['target'].encode('cp1252', errors='replace') + b'\0'
        wide = spec['target'].encode('utf-16-le') + b'\0\0'
        base_offset = 36 + len(volume)
        suffix_offset = base_offset + len(base)
        wide_offset = suffix_offset + 1
        wide_suffix_offset = wide_offset + len(wide)
        info = struct.pack('<IIIIIIIII', wide_suffix_offset + 2, 36, 1, 36,
                           base_offset, 0, suffix_offset, wide_offset, wide_suffix_offset)
        result += info + volume + base + b'\0' + wide + b'\0\0'
    for name, _ in fields:
        if name in spec:
            text = spec[name].encode('utf-16-le')
            result += struct.pack('<H', len(text) // 2) + text
    if 'known_folder' in spec:
        result += struct.pack('<II', 28, 0xA000000B)
        result += uuid.UUID(spec['known_folder']).bytes_le + struct.pack('<I', 0)
    return result + struct.pack('<I', 0)


def varint(value):
    if value < 0 or value >= 1 << 64: raise ValueError('protobuf value out of range')
    data = bytearray()
    while value > 127:
        data.append((value & 127) | 128)
        value >>= 7
    return data + bytes([value])


def protobuf(fields):
    result = bytearray()
    for field in fields:
        number, wire = field['number'], field['wire']
        if not 0 < number < (1 << 29): raise ValueError('invalid protobuf field number')
        if wire == 'varint':
            result += varint(number << 3) + varint(field['value'])
        else:
            if wire == 'message': data = protobuf(field['fields'])
            elif wire == 'string': data = field['value'].encode('utf-8')
            elif wire == 'bytes': data = bytes.fromhex(field['hex'])
            else: raise ValueError(f'unsupported protobuf wire: {wire}')
            result += varint((number << 3) | 2) + varint(len(data)) + data
    return result


def generate_file(source, target, kind):
    target.parent.mkdir(parents=True, exist_ok=True)
    if kind == 'copy':
        shutil.copyfile(source, target)
    elif kind == 'sqlite':
        with sqlite3.connect(target) as db:
            db.executescript('PRAGMA page_size=4096; PRAGMA auto_vacuum=NONE; ' + source.read_text())
            db.commit()
            db.execute('VACUUM')
        with sqlite3.connect(f'file:{target}?mode=ro', uri=True) as db:
            if db.execute('PRAGMA integrity_check').fetchone()[0] != 'ok':
                raise ValueError('generated SQLite database failed integrity check')
    else:
        spec = json.loads(source.read_text())
        if kind == 'binary-vdf': data = vdf(spec['entries'])
        elif kind == 'appinfo': data = appinfo(spec)
        elif kind == 'lnk': data = lnk(spec)
        elif kind == 'protobuf': data = protobuf(spec['fields'])
        elif kind == 'registry': data = registry(spec)
        elif kind == 'gaming-root':
            paths = spec['paths']
            if not 0 < len(paths) <= 16 or any(not isinstance(p, str) or '\0' in p or len(p) > 1024 for p in paths):
                raise ValueError('invalid synthetic GamingRoot paths')
            data = b'RGBX' + struct.pack('<I', len(paths))
            data += b''.join(p.encode('utf-16-le') + b'\0\0' for p in paths)
        else: raise ValueError(f'unsupported fixture kind: {kind}')
        target.write_bytes(data)
    if target.stat().st_size > MAX_FILE: raise ValueError(f'fixture exceeds size budget: {target}')


def machine_toml(spec):
    quote = json.dumps
    lines = ['schema_version = 1', 'os = ' + quote(spec['os']), 'now_unix_secs = 1700000000']
    for table in ['env', 'known_folders']:
        if spec.get(table):
            lines += ['', '[' + table + ']']
            lines += [quote(key) + ' = ' + quote(value) for key, value in sorted(spec[table].items())]
    for volume in spec.get('volumes', []):
        lines += ['', '[[volumes]]', 'path = ' + quote(volume['path']), 'kind = ' + quote(volume['kind']),
                  'case_sensitive = ' + str(volume['case_sensitive']).lower()]
    return '\n'.join(lines) + '\n'


def build(root):
    fixtures = root / FIXTURES
    source = fixtures / 'src'
    catalogue = json.loads((source / 'catalogue.json').read_text())
    if catalogue['schema_version'] != 1: raise ValueError('unknown fixture catalogue schema')
    with tempfile.TemporaryDirectory(prefix='caskade-discovery-build-') as temporary:
        staged = Path(temporary)
        for spec in catalogue['machines']:
            scenario = confined(staged / 'machines', spec['id'])
            (scenario / 'root').mkdir(parents=True)
            (scenario / 'registry').mkdir()
            (scenario / 'machine.toml').write_text(machine_toml(spec))
            for entry in spec['files']:
                generate_file(confined(source, entry['source']), confined(scenario, entry['path']), entry['kind'])
        for entry in catalogue.get('formats', []):
            generate_file(confined(source, entry['source']), confined(staged / 'formats', entry['path']), entry['kind'])
        for directory in ['machines', 'formats']:
            output = fixtures / directory
            if output.exists(): shutil.rmtree(output)
            shutil.copytree(staged / directory, output)
    print(f'Generated {len(catalogue["machines"])} synthetic machines; Python SQLite {sqlite3.sqlite_version}')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=ROOT)
    build(parser.parse_args().root)
