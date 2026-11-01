#!/usr/bin/env python3
"""Build genuine macOS-only fixtures into a separate runner artifact inventory."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import stat
import struct
import subprocess
import tempfile
from xml.etree import ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / 'crates/caskade-installer/tests/fixtures'
SOURCE = FIXTURES / 'src/macos-native'


def run(command, **kwargs):
    return subprocess.run([str(value) for value in command], check=True, **kwargs)


def sla_resources(text):
    # Macintosh LPic: default language, count, (language, resource-5000, wide).
    # STR#: big-endian count followed by six Pascal MacRoman strings.
    labels = ['English', 'Agree', 'Disagree', 'Print', 'Save', 'Agree to mount this original Caskade test fixture.']
    strings = struct.pack('>H', len(labels))
    for label in labels:
        raw = label.encode('mac_roman')
        if len(raw) > 255:
            raise ValueError('SLA label exceeds a Pascal byte string')
        strings += bytes([len(raw)]) + raw
    resource = lambda data, name: dict(Attributes='0x0000', Data=data, ID='5000', Name=name)
    return {'LPic': [resource(struct.pack('>HHHHH', 0, 1, 0, 0, 0), '')],
            'STR#': [resource(strings, 'English')], 'TEXT': [resource(text.encode('mac_roman'), 'English SLA')]}


def source_check():
    pin = json.loads((SOURCE / 'builder.json').read_text())
    required = {'mac-native-app-zip', 'mac-dmg-udzo', 'mac-dmg-ulfo', 'mac-dmg-sla',
                'mac-native-pkg', 'mac-native-pkg-system', 'mac-native-product-pkg'}
    if pin['schema_version'] != 1 or set(pin['ids']) != required:
        raise ValueError('native fixture family catalogue mismatch')
    resource = plistlib.loads(plistlib.dumps(sla_resources((SOURCE / 'license.txt').read_text())))
    if struct.unpack('>HHHHH', resource['LPic'][0]['Data']) != (0, 1, 0, 0, 0):
        raise ValueError('SLA language mapping mismatch')
    if resource['STR#'][0]['Data'][:2] != b'\0\6' or not resource['TEXT'][0]['Data']:
        raise ValueError('SLA strings/body are incomplete')
    if not (SOURCE / 'framework.c').is_file():
        raise ValueError('missing original framework source')
    print('macOS fixture source/catalogue/SLA resource validation: ok')
    return pin


def tree(root):
    entries = []
    for path in sorted(root.rglob('*')):
        metadata = path.lstat(); name = path.relative_to(root).as_posix()
        if stat.S_ISLNK(metadata.st_mode):
            entry = dict(path=name, kind='symlink', size=0, executable=False, symlink_target=os.readlink(path))
        elif path.is_dir():
            entry = dict(path=name, kind='directory', size=0, executable=False)
        else:
            entry = dict(path=name, kind='file', size=metadata.st_size,
                         executable=bool(metadata.st_mode & 0o111), sha256=hashlib.sha256(path.read_bytes()).hexdigest())
        entries.append(entry)
    return entries


def expectation(output, id, format, stage, delegate=False):
    expected = output / 'expected/macos-native'; expected.mkdir(parents=True, exist_ok=True)
    extension = 'zip' if format == 'zip' else ('dmg' if format == 'dmg' else 'pkg')
    relative = f'bin/macos-native/{id}.{extension}'
    (expected / (id + '.tree.json')).write_text(json.dumps(tree(stage), indent=2) + '\n')
    lines = ['schema_version = 1', f'id = "{id}"', 'title = "Fixture Game"', 'layout = "macos-native"',
             f'format = "{format}"', 'platform = "macos"', f'input = "{relative}"', f'parts = ["{relative}"]',
             f'tree = "macos-native/{id}.tree.json"', 'launch_target = "FixtureGame.app/Contents/MacOS/FixtureGame"']
    for host in ['windows', 'linux', 'macos']:
        lines += ['', f'[host.{host}]', f'outcome = "{"Delegated" if delegate and host == "macos" else "Installed" if host == "macos" else "Unsupported"}"',
                  f'strategy = "{"Delegate" if delegate else "Extract"}"', 'runtime = "Native"']
    (expected / (id + '.toml')).write_text('\n'.join(lines) + '\n')


def native_bundle(stage, build, pin):
    run(['cargo', 'build', '--release', '--locked', '--manifest-path', ROOT / 'crates/caskade-installer/tests/fixture-game/Cargo.toml', '--target-dir', build])
    app = stage / 'FixtureGame.app'; executable = app / 'Contents/MacOS/FixtureGame'; executable.parent.mkdir(parents=True)
    shutil.copyfile(build / 'release/caskade-fixture-game', executable); executable.chmod(0o755)
    if executable.read_bytes()[:4] not in [b'\xcf\xfa\xed\xfe', b'\xfe\xed\xfa\xcf', b'\xca\xfe\xba\xbe']:
        raise RuntimeError('native fixture-game is not an actual Mach-O binary')
    info = {'CFBundleName': 'Fixture Game', 'CFBundleDisplayName': 'Fixture Game', 'CFBundleExecutable': 'FixtureGame',
            'CFBundleIdentifier': pin['identifier'], 'CFBundleVersion': '1', 'CFBundleShortVersionString': '1.0', 'CFBundlePackageType': 'APPL'}
    (app / 'Contents/Info.plist').write_bytes(plistlib.dumps(info))
    framework = app / 'Contents/Frameworks/X.framework'; version = framework / 'Versions/A'
    (version / 'Resources').mkdir(parents=True)
    run(['xcrun', 'clang', '-dynamiclib', '-Os', SOURCE / 'framework.c', '-install_name', '@rpath/X.framework/Versions/A/X', '-o', version / 'X'])
    (version / 'Resources/Info.plist').write_bytes(plistlib.dumps({'CFBundleName': 'X', 'CFBundleIdentifier': 'org.caskade.fixture.framework', 'CFBundleExecutable': 'X', 'CFBundlePackageType': 'FMWK', 'CFBundleVersion': '1'}))
    (framework / 'Versions/Current').symlink_to('A'); (framework / 'X').symlink_to('Versions/Current/X')
    (framework / 'Resources').symlink_to('Versions/Current/Resources')
    run(['codesign', '--force', '--sign', '-', '--timestamp=none', framework])
    run(['codesign', '--force', '--sign', '-', '--timestamp=none', app])
    run(['codesign', '--verify', '--deep', '--strict', app])
    token = build / 'native-token'; run([executable, '--token', token])
    if not token.is_file() or not token.read_text().strip().endswith('/FixtureGame'):
        raise RuntimeError('actual Mach-O launch proof failed')
    return app


def build(output, pin):
    if platform.system() != 'Darwin':
        raise RuntimeError('genuine hdiutil/pkgbuild/productbuild fixtures require macOS; use --check elsewhere')
    output.mkdir(parents=True, exist_ok=True); binary = output / 'bin/macos-native'; binary.mkdir(parents=True, exist_ok=True)
    for old in binary.glob('*'):
        if old.is_file(): old.unlink()
    with tempfile.TemporaryDirectory(prefix='caskade-native-build-') as temp:
        job = Path(temp); stage = job / 'stage'; stage.mkdir(); app = native_bundle(stage, job / 'build', pin)
        run(['ditto', '-c', '-k', '--norsrc', '--keepParent', app, binary / 'mac-native-app-zip.zip'])
        expectation(output, 'mac-native-app-zip', 'zip', stage)
        resources = job / 'sla.plist'; resources.write_bytes(plistlib.dumps(sla_resources((SOURCE / 'license.txt').read_text())))
        volume = job / 'volume'; shutil.copytree(stage, volume, symlinks=True); (volume / 'Applications').symlink_to('/Applications')
        for id, format in [('mac-dmg-udzo', 'UDZO'), ('mac-dmg-ulfo', 'ULFO'), ('mac-dmg-sla', 'UDZO')]:
            image = binary / (id + '.dmg')
            run(['hdiutil', 'create', '-srcfolder', volume, '-volname', 'CaskadeFixture', '-fs', 'HFS+', '-format', format, '-ov', image])
            if id == 'mac-dmg-sla':
                run(['hdiutil', 'udifrez', '-xml', resources, image])
                exported = run(['hdiutil', 'udifderez', '-xml', image], stdout=subprocess.PIPE).stdout
                if b'LPic' not in exported or b'TEXT' not in exported:
                    raise RuntimeError('SLA resources missing from genuine image')
            run(['hdiutil', 'verify', image]); expectation(output, id, 'dmg', stage)
        for id, location in [('mac-native-pkg', '/Applications'), ('mac-native-pkg-system', pin['system_location'])]:
            run(['pkgbuild', '--root', stage, '--identifier', 'org.caskade.fixture.' + id, '--version', '1.0', '--install-location', location, binary / (id + '.pkg')])
            expectation(output, id, 'pkg', stage, id.endswith('system'))
        component = binary / 'mac-native-pkg.pkg'
        distribution = ET.Element('installer-gui-script', {'minSpecVersion': '1'})
        ET.SubElement(distribution, 'title').text = 'Caskade Fixture Game'
        ET.SubElement(distribution, 'options', {'customize': 'never', 'require-scripts': 'false'})
        outline = ET.SubElement(distribution, 'choices-outline'); ET.SubElement(outline, 'line', {'choice': 'fixture'})
        choice = ET.SubElement(distribution, 'choice', {'id': 'fixture', 'visible': 'false'}); ET.SubElement(choice, 'pkg-ref', {'id': 'org.caskade.fixture.mac-native-pkg'})
        ET.SubElement(distribution, 'pkg-ref', {'id': 'org.caskade.fixture.mac-native-pkg', 'version': '1.0'}).text = component.name
        definition = job / 'Distribution.xml'; definition.write_bytes(ET.tostring(distribution, encoding='utf-8', xml_declaration=True))
        run(['productbuild', '--distribution', definition, '--package-path', binary, binary / 'mac-native-product-pkg.pkg'])
        expectation(output, 'mac-native-product-pkg', 'pkg', stage, True)
    inventory = {p.relative_to(output).as_posix(): {'bytes': p.stat().st_size, 'sha256': hashlib.sha256(p.read_bytes()).hexdigest()}
                 for directory in ['bin/macos-native', 'expected/macos-native'] for p in sorted((output / directory).glob('*')) if p.is_file()}
    total = sum(item['bytes'] for name, item in inventory.items() if name.startswith('bin/'))
    if total > pin['max_artifact_binary_bytes']:
        raise RuntimeError(f'native binary artifact budget exceeded: {total}')
    versions = {}
    for name, command in {'macos': ['sw_vers'], 'hdiutil': ['hdiutil', 'help'], 'pkgbuild': ['pkgbuild', '--version'], 'productbuild': ['productbuild', '--version'], 'rustc': ['rustc', '-Vv'], 'clang': ['xcrun', 'clang', '--version']}.items():
        result = subprocess.run(command, capture_output=True, text=True)
        versions[name] = (result.stdout + result.stderr)[:8192]
    (output / 'native-build.json').write_text(json.dumps({'schema_version': 1, 'platform': platform.platform(), 'architecture': platform.machine(), 'binary_bytes': total, 'tools': versions, 'outputs': inventory}, indent=2) + '\n')
    print(f'native macOS artifact: {len(inventory)} outputs; {total}/{pin["max_artifact_binary_bytes"]} binary bytes')


def main():
    parser = argparse.ArgumentParser(description=__doc__); parser.add_argument('--artifact', type=Path); parser.add_argument('--check', action='store_true')
    args = parser.parse_args(); pin = source_check()
    if args.check: return
    if args.artifact is None: parser.error('--artifact is required; generated native binaries never overwrite committed inputs directly')
    build(args.artifact.resolve(), pin)


if __name__ == '__main__': main()
