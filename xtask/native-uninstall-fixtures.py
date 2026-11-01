#!/usr/bin/env python3
"""Real native-uninstall mini installers; shares the SHA-pinned Windows builders."""
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
b = importlib.util.module_from_spec(spec); spec.loader.exec_module(b)
SRC = b.FIXTURES / 'src/native-uninstall'
BIN = b.FIXTURES / 'bin/native-uninstall'
EXPECTED = b.FIXTURES / 'expected/native-uninstall'
PRODUCT_CODE = '{F78B4949-77EC-441F-B1E9-9673F41E1A18}'

def emit(id, family, source, payload, builder, version, generated):
    out = BIN / id / source.name; out.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, out)
    if out.stat().st_size > 3_000_000: raise RuntimeError('native fixture exceeds 3 MB')
    q = json.dumps
    lines = ['schema_version = 1', f'id = {q(id)}', f'title = {q("Caskade Native " + family.upper() + " Fixture")}',
        'layout = "unity"', f'format = {q(family)}', 'platform = "windows"',
        f'input = {q(out.relative_to(b.FIXTURES).as_posix())}',
        f'parts = {q([out.relative_to(b.FIXTURES).as_posix()])}',
        f'tree = "native-uninstall/{id}.tree.json"', 'launch_target = "FixtureGame.exe"',
        '', '[windows]', f'builder = {q(builder)}', f'version = {q(version)}',
        'features = ["native-uninstall", "registry", "ini"]', f'native_generated = {q(generated)}']
    for host, runtime in [('windows', 'Native'), ('linux', 'ProtonPrefix'), ('macos', 'WinePrefix')]:
        lines += ['', f'[host.{host}]', 'outcome = "Installed"', f'strategy = {q("RunSilent" if host == "windows" else "Extract")}', f'runtime = {q(runtime)}']
    (EXPECTED / f'{id}.toml').write_text('\n'.join(lines) + '\n')
    (EXPECTED / f'{id}.tree.json').write_text(json.dumps(b.tree(payload), indent=2) + '\n')
    print(id, out.stat().st_size, flush=True)

def msi(job, payload):
    ns = 'http://schemas.microsoft.com/wix/2006/wi'; ET.register_namespace('', ns)
    def node(parent, tag, **attrs): return ET.SubElement(parent, '{' + ns + '}' + tag, attrs)
    root = ET.Element('{' + ns + '}Wix')
    product = node(root, 'Product', Id=PRODUCT_CODE, Name='Caskade Native MSI Fixture', Language='1033', Version='1.0.0', Manufacturer='Caskade', UpgradeCode='B81C3344-A3B1-4F76-9EE5-C0EC2F81A306')
    node(product, 'Package', InstallerVersion='200', Compressed='yes', InstallScope='perUser')
    node(product, 'Media', Id='1', Cabinet='fixture.cab', EmbedCab='yes')
    # Persist a caller-selected INSTALLDIR for uninstall costing, including Wine.
    prop = node(product, 'Property', Id='INSTALLDIR')
    node(prop, 'RegistrySearch', Id='RememberInstallDir', Root='HKCU', Key='Software\\CaskadeNativeFixture\\msi', Name='InstallPath', Type='raw')
    target = node(product, 'Directory', Id='TARGETDIR', Name='SourceDir')
    install = node(node(target, 'Directory', Id='LocalAppDataFolder'), 'Directory', Id='INSTALLDIR', Name='CaskadeNativeMSIFixture')
    dirs = {'.': install}; feature = node(product, 'Feature', Id='Main', Level='1')
    for i, path in enumerate(sorted(payload.rglob('*'))):
        relative = path.relative_to(payload); parent = dirs[relative.parent.as_posix()]
        if path.is_dir(): dirs[relative.as_posix()] = node(parent, 'Directory', Id=f'D{i}', Name=path.name)
        else:
            component = node(parent, 'Component', Id=f'C{i}', Guid=str(uuid.uuid5(uuid.UUID('32b52ed7-2536-4897-8f40-cb662b805d70'), relative.as_posix())).upper())
            node(component, 'File', Id=f'F{i}', Name=path.name, Source=str(path), KeyPath='yes')
            node(feature, 'ComponentRef', Id=f'C{i}')
    component = node(install, 'Component', Id='Registry', Guid='D83C58B2-DFFB-423B-BEC7-3C635F8971B7')
    node(component, 'RegistryValue', Root='HKCU', Key='Software\\CaskadeNativeFixture\\msi', Name='InstallPath', Type='string', Value='[INSTALLDIR]', KeyPath='yes')
    node(feature, 'ComponentRef', Id='Registry')
    node(product, 'CustomAction', Id='SetArpInstallLocation', Property='ARPINSTALLLOCATION', Value='[INSTALLDIR]')
    sequence = node(product, 'InstallExecuteSequence')
    node(sequence, 'Custom', Action='SetArpInstallLocation', After='CostFinalize').text = 'NOT Installed'
    script = job / 'installer.wxs'; ET.ElementTree(root).write(script, encoding='utf-8', xml_declaration=True)
    # Preserve authored source after generation for review; it has portable payload paths.
    review = ET.tostring(root, encoding='unicode').replace(str(payload), 'payload')
    (SRC / 'msi.wxs').write_text(review + '\n')
    env = os.environ | {'LD_LIBRARY_PATH': str(b.CACHE / 'local/usr/lib/x86_64-linux-gnu')}
    b.run([b.CACHE / 'local/usr/bin/wixl', '-o', 'setup.msi', script], job, env)

def main():
    parser = argparse.ArgumentParser(description=__doc__); parser.add_argument('--prepare', action='store_true'); parser.add_argument('--family', choices=['inno', 'nsis', 'msi']); args = parser.parse_args()
    b.provision({'inno', 'nsis', 'msi'}, args.prepare)
    BIN.mkdir(parents=True, exist_ok=True); EXPECTED.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='caskade-native-fixtures-') as temp:
        for family, id, script, builder, version in [
            ('inno', 'native-inno-shortcuts', 'inno.iss', 'ISCC', '6.0.5'),
            ('nsis', 'native-nsis-uninstall', 'nsis.nsi', 'makensis', '3.11'),
            ('msi', 'native-msi-uninstall', 'msi.wxs', 'wixl', '0.103')]:
            if args.family and family != args.family: continue
            job = Path(temp) / family; job.mkdir(); (job / 'out').mkdir()
            payload = b.payload(job, 'unity')
            if family == 'inno':
                shutil.copyfile(SRC / script, job / script)
                b.wine([b.CACHE / 'inno60/ISCC.exe', '/Q', script], job)
                source = job / 'out/setup.exe'
                generated = ['config.ini', 'unins000.exe', 'unins000.dat']
            elif family == 'nsis':
                shutil.copyfile(SRC / script, job / script)
                b.wine([next((b.CACHE / 'nsis3').glob('*/makensis.exe')), '/V2', script], job)
                source = job / 'out/setup.exe'; generated = ['config.ini', 'Uninstall.exe']
            else:
                msi(job, payload); source = job / 'setup.msi'; generated = []
            emit(id, family, source, payload, builder, version, generated)

if __name__ == '__main__': main()
