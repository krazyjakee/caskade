#!/usr/bin/env python3
"""T7 native acceptance installers; reuse the committed, SHA-pinned builders."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import shutil
import tempfile
import uuid
import xml.etree.ElementTree as ET

spec = importlib.util.spec_from_file_location('windows_builders', Path(__file__).with_name('windows-fixtures.py'))
b = importlib.util.module_from_spec(spec)
spec.loader.exec_module(b)
SRC = b.FIXTURES / 'src/native-windows'
BIN = b.FIXTURES / 'bin/native-windows'
EXPECTED = b.FIXTURES / 'expected/native-windows'
NAMESPACE = uuid.UUID('9ad74f7b-2e9a-4ad0-9c55-49cb5c062e35')


def guid(name):
    return '{' + str(uuid.uuid5(NAMESPACE, name)).upper() + '}'


def emit(id, family, files, payload, layout, builder, version, generated, features):
    parts = []
    for source in files:
        out = BIN / id / source.name
        out.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, out)
        if out.stat().st_size > 3_000_000:
            raise RuntimeError(f'{id}: output exceeds the individual 3 MB limit')
        parts.append(out.relative_to(b.FIXTURES).as_posix())
    q = json.dumps
    lines = ['schema_version = 1', f'id = {q(id)}', 'title = "Fixture Game"',
             f'layout = {q(layout)}', f'format = {q(family)}', 'platform = "windows"',
             f'input = {q(parts[0])}', f'parts = {q(parts)}',
             f'tree = "native-windows/{layout}.tree.json"', 'launch_target = "FixtureGame.exe"',
             '', '[windows]', f'builder = {q(builder)}', f'version = {q(version)}',
             f'features = {q(["native-windows", "native-uninstall", *features])}',
             f'native_generated = {q(generated)}']
    for host, runtime in [('windows', 'Native'), ('linux', 'ProtonPrefix'), ('macos', 'WinePrefix')]:
        lines += ['', f'[host.{host}]', 'outcome = "Installed"',
                  f'strategy = {q("RunSilent" if host == "windows" else "Extract")}', f'runtime = {q(runtime)}']
    (EXPECTED / f'{id}.toml').write_text('\n'.join(lines) + '\n')
    tree = b.tree(payload)
    golden = EXPECTED / f'{layout}.tree.json'
    if golden.exists() and json.loads(golden.read_text()) != tree:
        raise RuntimeError(f'{id}: payload differs from the shared {layout} golden')
    golden.write_text(json.dumps(tree, indent=2) + '\n')
    print(id, sum(file.stat().st_size for file in files), flush=True)


def msi(job, payload, id, embedded):
    ns = 'http://schemas.microsoft.com/wix/2006/wi'
    ET.register_namespace('', ns)

    def node(parent, tag, **attrs):
        return ET.SubElement(parent, '{' + ns + '}' + tag, attrs)

    root = ET.Element('{' + ns + '}Wix')
    product = node(root, 'Product', Id=guid(id), Name='Caskade T7 ' + id,
                   Language='1033', Version='1.0.0', Manufacturer='Caskade', UpgradeCode=guid(id + '/upgrade'))
    node(product, 'Package', InstallerVersion='200', Compressed='yes', InstallScope='perUser')
    node(product, 'Media', Id='1', Cabinet='fixture.cab', EmbedCab='yes' if embedded else 'no')
    key = 'Software\\CaskadeT7\\' + id
    prop = node(product, 'Property', Id='INSTALLDIR')
    node(prop, 'RegistrySearch', Id='RememberInstallDir', Root='HKCU', Key=key, Name='InstallPath', Type='raw')
    node(product, 'Property', Id='CASKADE_NOSHORTCUTS', Secure='yes')
    target = node(product, 'Directory', Id='TARGETDIR', Name='SourceDir')
    node(target, 'Directory', Id='ProgramMenuFolder')
    install = node(node(target, 'Directory', Id='LocalAppDataFolder'), 'Directory', Id='INSTALLDIR', Name=id)
    dirs = {'.': install}
    feature = node(product, 'Feature', Id='Main', Level='1')
    for index, path in enumerate(sorted(payload.rglob('*'))):
        relative = path.relative_to(payload)
        parent = dirs[relative.parent.as_posix()]
        if path.is_dir():
            dirs[relative.as_posix()] = node(parent, 'Directory', Id=f'D{index}', Name=path.name)
        else:
            component = node(parent, 'Component', Id=f'C{index}', Guid=guid(id + '/' + relative.as_posix()))
            node(component, 'File', Id=f'F{index}', Name=path.name, Source=str(path), KeyPath='yes')
            node(feature, 'ComponentRef', Id=f'C{index}')
    component = node(install, 'Component', Id='Registry', Guid=guid(id + '/registry'))
    node(component, 'RegistryValue', Root='HKCU', Key=key, Name='InstallPath', Type='string', Value='[INSTALLDIR]', KeyPath='yes')
    node(feature, 'ComponentRef', Id='Registry')
    component = node(install, 'Component', Id='Shortcuts', Guid=guid(id + '/shortcuts'))
    node(component, 'Shortcut', Id='StartMenu', Directory='ProgramMenuFolder', Name='Caskade T7 ' + id,
         Target='[INSTALLDIR]FixtureGame.exe', WorkingDirectory='INSTALLDIR')
    node(component, 'RegistryValue', Root='HKCU', Key=key, Name='ShortcutCreated', Type='integer', Value='1', KeyPath='yes')
    node(feature, 'ComponentRef', Id='Shortcuts')
    node(product, 'CustomAction', Id='SetArpInstallLocation', Property='ARPINSTALLLOCATION', Value='[INSTALLDIR]')
    sequence = node(product, 'InstallExecuteSequence')
    node(sequence, 'Custom', Action='SetArpInstallLocation', After='CostFinalize').text = 'NOT Installed'
    script = job / 'installer.wxs'
    ET.ElementTree(root).write(script, encoding='utf-8', xml_declaration=True)
    (SRC / f'{id}.wxs').write_text(ET.tostring(root, encoding='unicode').replace(str(payload), 'payload') + '\n')
    local = b.CACHE / 'local/usr'
    env = os.environ | {'LD_LIBRARY_PATH': str(local / 'lib/x86_64-linux-gnu')}
    b.run([local / 'bin/wixl', '-o', 'setup.msi', script], job, env)
    # wixl 0.103 does not implement the WiX Component/Condition element. Apply
    # its authored condition using msibuild from the same SHA-pinned msitools.
    b.run([local / 'bin/msibuild', 'setup.msi', '-q', (SRC / 'shortcut-condition.sql').read_text()], job, env)
    files = [job / 'setup.msi']
    if not embedded:
        cabinet = job / 'cabinet'
        cabinet.mkdir()
        names = []
        for file in root.iter('{' + ns + '}File'):
            names.append(file.attrib['Id'])
            shutil.copyfile(file.attrib['Source'], cabinet / file.attrib['Id'])
        b.run([local / 'bin/gcab', '-c', '-z', job / 'fixture.cab', *names], cabinet, env)
        files.append(job / 'fixture.cab')
    return files


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prepare', action='store_true')
    args = parser.parse_args()
    b.provision({'inno', 'nsis', 'msi'}, args.prepare)
    EXPECTED.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='caskade-t7-build-') as temp:
        temp = Path(temp)
        for compiler, version in [('inno5', '5.6.1'), ('inno60', '6.0.5')]:
            id = 't7-gog-' + compiler
            job = temp / id
            job.mkdir()
            (job / 'out').mkdir()
            payload = b.payload(job, 'gog')
            shutil.copyfile(SRC / 'installer.iss', job / 'installer.iss')
            b.wine([b.CACHE / compiler / 'ISCC.exe', '/Q', '/DFixtureId=' + id, 'installer.iss'], job)
            emit(id, 'inno', [job / 'out/setup.exe'], payload, 'gog', 'ISCC', version,
                 ['unins000.exe', 'unins000.dat'], ['start-menu-shortcut', 'generic-NOICONS'])
        for compiler, version in [('nsis251', '2.51'), ('nsis3', '3.11')]:
            id = 't7-' + compiler
            job = temp / id
            job.mkdir()
            (job / 'out').mkdir()
            payload = b.payload(job, 'unity')
            shutil.copyfile(SRC / 'installer.nsi', job / 'installer.nsi')
            flags = ['/DFixtureId=' + id] + (['/DNSIS3'] if compiler == 'nsis3' else [])
            b.wine([next((b.CACHE / compiler).glob('*/makensis.exe')), '/V2', *flags, 'installer.nsi'], job)
            emit(id, 'nsis', [job / 'out/setup.exe'], payload, 'unity', 'makensis', version,
                 ['Uninstall.exe'], ['start-menu-shortcut', 'authored-CASKADE_NO_SHORTCUTS'])
        for embedded in [True, False]:
            id = 't7-msi-' + ('embedded' if embedded else 'external')
            job = temp / id
            job.mkdir()
            payload = b.payload(job, 'unity')
            files = msi(job, payload, id, embedded)
            emit(id, 'msi', files, payload, 'unity', 'wixl', '0.103', [],
                 ['embedded-cab' if embedded else 'external-cab', 'start-menu-shortcut', 'authored-CASKADE_NOSHORTCUTS'])
    total = sum(path.stat().st_size for path in BIN.rglob('*') if path.is_file())
    if total > 4_500_000:
        raise RuntimeError(f'T7 namespace exceeds its reserved 4.5 MB budget: {total}')
    print(f'T7 native Windows binaries: {total}/4500000 bytes', flush=True)


if __name__ == '__main__':
    main()
