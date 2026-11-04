#!/usr/bin/env python3
"""Pinned, opt-in maintainer builders; never called by read-only fixture checks."""
import argparse
import fnmatch
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parent.parent
FIXTURES = ROOT / 'crates/caskade-installer/tests/fixtures'
SOURCE = FIXTURES / 'src/windows'
CACHE = ROOT / 'target/fixture-builders'
BIN = FIXTURES / 'bin/windows'
EXPECTED = FIXTURES / 'expected/windows'
PINS = json.loads((SOURCE / 'builders.json').read_text())
PRODUCED = []
ONLY = ["*"]

def selected(id):
    return any(fnmatch.fnmatchcase(id, pattern) for pattern in ONLY)



def run(args, cwd=None, env=None):
    result = subprocess.run([str(a) for a in args], cwd=cwd, env=env,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    if result.returncode:
        raise RuntimeError(f'{args!r}: exit {result.returncode}\n{result.stdout}')
    return result.stdout


def winpath(path):
    path = str(Path(path).resolve())
    return path if os.name == 'nt' else 'Z:' + path.replace('/', '\\')


def wine(args, cwd=None):
    if os.name == 'nt':
        return run(args, cwd)
    env = os.environ | {'WINEPREFIX': str(CACHE / 'wine'), 'WINEDEBUG': '-all'}
    return run(['xvfb-run', '-a', 'wine', *args], cwd, env)


def download(name, prepare):
    pin = PINS[name]
    path = CACHE / 'downloads' / name
    if not path.exists():
        if not prepare:
            raise RuntimeError(f'missing {name}; rerun with --prepare for pinned downloads')
        path.parent.mkdir(parents=True, exist_ok=True)
        with urllib.request.urlopen(pin['url'], timeout=60) as response:
            data = response.read()
        if hashlib.sha256(data).hexdigest() != pin['sha256']:
            raise RuntimeError(f'{name}: download SHA-256 mismatch')
        path.write_bytes(data)
    if hashlib.sha256(path.read_bytes()).hexdigest() != pin['sha256']:
        raise RuntimeError(f'{name}: cached SHA-256 mismatch')
    return path


def provision(families, prepare):
    if 'inno' in families:
        for name in ['inno5', 'inno60', 'inno64']:
            installer = download(name + '.exe', prepare)
            compiler = CACHE / name / 'ISCC.exe'
            if not compiler.exists():
                wine([installer, '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART',
                      '/SP-', '/DIR=' + winpath(compiler.parent)])
            if name != 'inno64':
                shutil.copyfile(download('ISCrypt.dll', prepare), compiler.parent / 'ISCrypt.dll')
    if 'nsis' in families:
        for name in ['nsis251', 'nsis3']:
            archive = download(name + '.zip', prepare)
            folder = CACHE / name
            if not folder.exists():
                with zipfile.ZipFile(archive) as z:
                    z.extractall(folder)
    if 'msi' in families:
        if os.name != 'posix' or sys.platform != 'linux':
            raise RuntimeError('wixl fixtures are built by the Linux workflow job')
        for name in PINS:
            if name.endswith('.deb'):
                run(['dpkg-deb', '-x', download(name, prepare), CACHE / 'local'])
    if 'sfx' in families:
        if '26.05' not in run([sevenzip(), 'i']):
            raise RuntimeError('SFX builder requires the pinned official 7zz 26.05')
        archive = download('7z-setup.exe', prepare)
        stub = CACHE / '7z-setup/7z.sfx'
        if not stub.exists():
            run([sevenzip(), 'x', archive, '-o' + str(stub.parent), '-y'])


def sevenzip():
    value = os.environ.get('CASKADE_7ZZ', str(ROOT / 'target/sidecars/7zip/26.05/linux-x64/7zz'))
    return str((ROOT / value).resolve()) if not Path(value).is_absolute() else value


def payload(work, layout):
    directory = work / 'payload'
    directory.mkdir()
    with zipfile.ZipFile(FIXTURES / 'bin' / (layout + '.zip')) as z:
        z.extractall(directory)
    # Native installers create config.ini from [INI]/WriteINIStr. Its absolute
    # InstallPath and Code output are semantic expectations, separate from the
    # static payload golden tree, which is shared across hosts.
    return directory


def tree(directory):
    result = []
    for path in sorted(directory.rglob('*')):
        data = path.read_bytes() if path.is_file() else b''
        entry = dict(path=path.relative_to(directory).as_posix(),
                     kind='file' if path.is_file() else 'directory',
                     size=len(data), executable=False)
        if path.is_file():
            entry['sha256'] = hashlib.sha256(data).hexdigest()
        result.append(entry)
    return sorted(result, key=lambda entry: entry['path'])


def emit(id, layout, format, files, directory, builder, version, features=(), password=None,
         prefix='', native_generated=()):
    parts = []
    for file in files:
        target = BIN / id / file.name
        target.parent.mkdir(parents=True, exist_ok=True)
        if file.stat().st_size > 3_000_000:
            raise RuntimeError(f'{id}: {file.name} exceeds 3 MB')
        shutil.copyfile(file, target)
        parts.append(target.relative_to(FIXTURES).as_posix())
    launch = 'FixtureGame/Binaries/Win64/FixtureGame-Win64-Shipping.exe' if layout == 'unreal' else 'FixtureGame.exe'
    launch = prefix + launch
    entries = tree(directory)
    if prefix:
        for entry in entries:
            entry['path'] = prefix + entry['path']
        entries.insert(0, dict(path=prefix.rstrip('/'), kind='directory', size=0, executable=False))
    tree_name = f'windows/{id}.tree.json'
    (FIXTURES / 'expected' / tree_name).write_text(json.dumps(entries, indent=2) + '\n')
    q = json.dumps
    lines = ['schema_version = 1', f'id = {q(id)}', 'title = "Fixture Game"',
             f'layout = {q(layout)}', f'format = {q(format)}', 'platform = "windows"',
             f'input = {q(parts[0])}', f'parts = {q(parts)}', f'tree = {q(tree_name)}',
             f'launch_target = {q(launch)}', '', '[windows]', f'builder = {q(builder)}',
             f'version = {q(version)}', f'features = {q(list(features))}',
             f'native_generated = {q(list(native_generated))}']
    if password:
        lines.append(f'password = {q(password)}')
    for host, runtime in [('windows', 'Native'), ('linux', 'ProtonPrefix'), ('macos', 'WinePrefix')]:
        native = host == 'windows' and format in ('inno', 'nsis', 'msi')
        outcome = 'NeedsUser' if password else 'Installed'
        lines += ['', f'[host.{host}]', f'outcome = {q(outcome)}',
                  f'strategy = {q("RunSilent" if native else "Extract")}', f'runtime = {q(runtime)}']
    (EXPECTED / (id + '.toml')).write_text('\n'.join(lines) + '\n')
    PRODUCED.extend([*(FIXTURES / part for part in parts), FIXTURES / 'expected' / tree_name, EXPECTED / (id + '.toml')])
    print(f'{id}: {sum(f.stat().st_size for f in files)} bytes', flush=True)


def inno(work):
    for compiler, version in [('inno5', '5.6.1'), ('inno60', '6.0.5'), ('inno64', '6.4.3')]:
        for feature in ['basic', 'sliced', 'password', 'code', 'registry', 'ini']:
            id = f'{compiler}-{feature}'
            aliases = {('inno5', 'basic'): 'gog-inno5-legacy', ('inno64', 'basic'): 'gog-inno-unity',
                       ('inno64', 'sliced'): 'gog-inno-sliced', ('inno64', 'registry'): 'legacy-registry-path'}
            id = aliases.get((compiler, feature), id)
            if not selected(id): continue
            job = work / id
            job.mkdir(); (job / 'out').mkdir()
            directory = payload(job, 'gog' if id.startswith('gog') else 'unity')
            if feature == 'sliced':
                (directory / 'slice-padding.dat').write_bytes(hashlib.shake_256(b'caskade-slices-v1').digest(4100000))
            shutil.copyfile(SOURCE / 'installer.iss', job / 'installer.iss')
            definitions = [] if feature == 'basic' else [feature.upper()]
            if id == 'gog-inno-unity':
                definitions += ['GOG', 'REGISTRY', 'INI', 'CODE', 'UNINSTALL']
            if id == 'legacy-registry-path':
                definitions += ['INI', 'UNINSTALL']
            flags = ['/D' + value for value in sorted(set(definitions))]
            wine([CACHE / compiler / 'ISCC.exe', '/Q', *flags, 'installer.iss'], job)
            files = [job / 'out/setup.exe'] + sorted((job / 'out').glob('*.bin'))
            generated = ['config.ini:Game/InstallPath={app}'] if 'INI' in definitions else []
            if 'UNINSTALL' in definitions:
                generated += ['unins000.exe', 'unins000.dat']
            if 'CODE' in definitions:
                generated.append('code-path.txt={app}')
            emit(id, 'gog' if id.startswith('gog') else 'unity', 'inno', files, directory,
                 'ISCC', version, definitions, 'caskade-fixture' if feature == 'password' else None,
                 native_generated=generated)


def nsis(work):
    for name, version in [('nsis251', '2.51'), ('nsis3', '3.11')]:
        for unicode in ([False] if name == 'nsis251' else [False, True]):
            for solid in [False, True]:
                id = f'{name}-{"unicode" if unicode else "ansi"}-{"solid" if solid else "nonsolid"}'
                if name == 'nsis3' and unicode and solid:
                    id = 'nsis-indie'
                if not selected(id): continue
                job = work / id; job.mkdir(); (job / 'out').mkdir()
                directory = payload(job, 'unity')
                shutil.copyfile(SOURCE / 'installer.nsi', job / 'installer.nsi')
                tool = next((CACHE / name).glob('*/makensis.exe'))
                flags = ['/DCOMPRESSOR=' + ('/SOLID lzma' if solid else 'zlib')]
                if name == 'nsis3':
                    flags += ['/DNSIS3', '/DUNICODE=' + ('true' if unicode else 'false')]
                wine([tool, '/V2', *flags, 'installer.nsi'], job)
                emit(id, 'unity', 'nsis', [job / 'out/setup.exe'], directory, 'makensis', version,
                     ['unicode' if unicode else 'ansi', 'solid-lzma' if solid else 'nonsolid-zlib', 'shortcut', 'ini'],
                     native_generated=['config.ini:Game/InstallPath={app}'])


def msi(work):
    local = CACHE / 'local/usr'
    env = os.environ | {'LD_LIBRARY_PATH': str(local / 'lib/x86_64-linux-gnu')}
    for id, embedded, custom in [('msi-embedded', True, False), ('msi-external', False, False),
                                 ('msi-plus-cab', False, True)]:
        if not selected(id): continue
        job = work / id; job.mkdir()
        directory = payload(job, 'unity')
        # wixl's directory/component graph is generated deterministically from the
        # same real payload as every other family, never hand-crafted MSI bytes.
        import xml.etree.ElementTree as ET
        ns = 'http://schemas.microsoft.com/wix/2006/wi'; ET.register_namespace('', ns)
        def node(parent, tag, **attrs):
            return ET.SubElement(parent, '{' + ns + '}' + tag, attrs)
        root = ET.Element('{' + ns + '}Wix')
        product_id = 'E8AEE435-3746-51E3-B090-78112E5A78A7' if custom else '*'
        product = node(root, 'Product', Id=product_id, Name='Caskade Fixture Game', Language='1033',
                       Version='1.0.0', Manufacturer='Caskade', UpgradeCode='C865996A-444F-4A46-BDDD-EA90118908F7')
        node(product, 'Package', InstallerVersion='200', Compressed='yes',
             InstallScope='perMachine' if custom else 'perUser')
        node(product, 'Media', Id='1', Cabinet='fixture.cab', EmbedCab='yes' if embedded else 'no')
        target = node(product, 'Directory', Id='TARGETDIR', Name='SourceDir')
        install = node(node(target, 'Directory', Id='LocalAppDataFolder'), 'Directory', Id='INSTALLDIR', Name='CaskadeFixture')
        dirs = {'.': install}; feature = node(product, 'Feature', Id='Main', Level='1')
        for i, path in enumerate(sorted(directory.rglob('*'))):
            relative = path.relative_to(directory)
            parent = dirs[relative.parent.as_posix()]
            if path.is_dir():
                dirs[relative.as_posix()] = node(parent, 'Directory', Id=f'D{i}', Name=path.name)
            else:
                component = node(parent, 'Component', Id=f'C{i}', Guid='*')
                node(component, 'File', Id=f'F{i}', Name=path.name, Source=str(path), KeyPath='yes')
                node(feature, 'ComponentRef', Id=f'C{i}')
        if custom:
            # This moving-test mini alone records its actual native path. The
            # other MSI fixtures keep their original x86/per-user authoring.
            prop = node(product, 'Property', Id='INSTALLDIR')
            node(prop, 'RegistrySearch', Id='RememberInstallDir', Root='HKLM',
                 Key=r'Software\CaskadeFixture\FixtureGame', Name='InstallPath', Type='raw', Win64='yes')
            component = node(install, 'Component', Id='InstallPath',
                             Guid='449A2DCB-A326-52CE-9A27-00F47182BDCF', Win64='yes')
            node(component, 'RegistryValue', Root='HKLM', Key=r'Software\CaskadeFixture\FixtureGame',
                 Name='InstallPath', Type='string', Value='[INSTALLDIR]', KeyPath='yes')
            node(component, 'IniFile', Id='InstallConfig', Name='config.ini', Directory='INSTALLDIR',
                 Action='createLine', Section='Game', Key='InstallPath', Value='[INSTALLDIR]')
            node(feature, 'ComponentRef', Id='InstallPath')
            node(product, 'CustomAction', Id='RecordFixturePath', Property='CASKADE_FIXTURE_PATH', Value='[INSTALLDIR]')
            # Its Uninstall registration names the folder, as the other MSI
            # fixtures' do, so detection can own it after a move (#38).
            node(product, 'CustomAction', Id='SetArpInstallLocation', Property='ARPINSTALLLOCATION', Value='[INSTALLDIR]')
            sequence = node(product, 'InstallExecuteSequence')
            node(sequence, 'Custom', Action='RecordFixturePath', After='CostFinalize').text = 'NOT Installed'
            node(sequence, 'Custom', Action='SetArpInstallLocation', After='RecordFixturePath').text = 'NOT Installed'
        script = job / 'installer.wxs'; ET.ElementTree(root).write(script, encoding='utf-8', xml_declaration=True)
        run([local / 'bin/wixl', *(['-a', 'x64'] if custom else []), '-o', 'setup.msi', script], job, env)
        if not embedded:
            cabinet = job / 'cabinet'; cabinet.mkdir()
            names = []
            for file_node in root.iter('{' + ns + '}File'):
                name = file_node.attrib['Id']; names.append(name)
                shutil.copyfile(file_node.attrib['Source'], cabinet / name)
            run([local / 'bin/gcab', '-c', '-z', job / 'fixture.cab', *names], cabinet, env)
        files = [job / 'setup.msi'] + ([] if embedded else [job / 'fixture.cab'])
        emit(id, 'unity', 'msi', files, directory, 'wixl', '0.103',
             ['embedded-cab' if embedded else 'external-cab'] + (['custom-action-type51', 'ini', 'registry64', 'native-uninstall'] if custom else []),
             native_generated=['config.ini:Game/InstallPath={app}'] if custom else [])


def archives(work, sfx):
    for id, layout, prefix in [('humble-win-zip', 'unity', 'FixtureGame-1.2.3/'),
                                ('unreal-zip', 'unreal', ''), ('steamlike-commonredist', 'unity', '')]:
        if not selected(id): continue
        job = work / id; job.mkdir(); directory = payload(job, layout)
        if id == 'steamlike-commonredist':
            (directory / '_CommonRedist').mkdir()
            shutil.copyfile(FIXTURES / 'bin/fixture-game-windows-x86_64.exe', directory / '_CommonRedist/vcredist_x64.exe')
            (directory / 'steam_appid.txt').write_text('480\n')
        output = job / 'payload.zip'
        with zipfile.ZipFile(output, 'w', zipfile.ZIP_DEFLATED) as z:
            for path in sorted(directory.rglob('*')):
                z.write(path, prefix + path.relative_to(directory).as_posix())
        emit(id, layout, 'zip', [output], directory, 'Python zipfile', 'stdlib', prefix=prefix)
    if not sfx:
        return
    job = work / 'sfx'; job.mkdir(); directory = payload(job, 'unity')
    output = job / 'sevenzip-sfx.exe'
    run([sevenzip(), 'a', '-t7z', '-sfx' + str(CACHE / '7z-setup/7z.sfx'), output, '.'], directory)
    emit('sevenzip-sfx', 'unity', '7z-sfx', [output], directory, '7zz + official 7z.sfx', '26.05 / 26.01')
    stub = job / 'zip-stub.exe'
    cc = os.environ.get('CASKADE_MINGW_CC', 'x86_64-w64-mingw32-gcc')
    run([cc, '-Os', '-s', '-municode', '-o', stub, SOURCE / 'zip-sfx.c'])
    output = job / 'zip-sfx.exe'; shutil.copyfile(stub, output)
    with zipfile.ZipFile(output, 'a', zipfile.ZIP_DEFLATED) as z:
        for path in sorted(directory.rglob('*')):
            z.write(path, path.relative_to(directory).as_posix())
    emit('zip-sfx', 'unity', 'zip-sfx', [output], directory, 'mingw + Python zipfile', '13-posix / stdlib',
         ['powershell-dotnet-extractor'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--only', default='*', help='comma-separated fixture ID glob patterns')
    parser.add_argument('--prepare', action='store_true', help='allow SHA-pinned builder downloads')
    parser.add_argument('--artifact', type=Path, help='stage only files built in this invocation')
    parser.add_argument('--families', default='inno,nsis,msi,sfx,archives')
    args = parser.parse_args()
    global ONLY
    ONLY = args.only.split(',')
    families = set(args.families.split(','))
    if not families <= {'inno', 'nsis', 'msi', 'sfx', 'archives'}:
        parser.error('unknown family')
    BIN.mkdir(parents=True, exist_ok=True); EXPECTED.mkdir(parents=True, exist_ok=True)
    provision(families, args.prepare)
    with tempfile.TemporaryDirectory(prefix='caskade-windows-fixtures-') as temp:
        work = Path(temp)
        if 'inno' in families: inno(work)
        if 'nsis' in families: nsis(work)
        if 'msi' in families: msi(work)
        if families & {'archives', 'sfx'}: archives(work, 'sfx' in families)
    if args.artifact:
        for source in PRODUCED:
            target = args.artifact / source.relative_to(FIXTURES)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, target)


if __name__ == '__main__':
    main()
