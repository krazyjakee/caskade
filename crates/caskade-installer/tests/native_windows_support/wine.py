"""Wine pre-evidence for T7; this does not replace the native Windows tests.

Run under Xvfb; each Wine command takes the machine heavy lock separately.
Uses a fresh Wine prefix and isolated
XDG paths, never the caller's prefix. Successful scratch directories are removed;
failures retain logs in the printed PRIVATE_WORK directory.
"""
import os, subprocess, tempfile, json, pathlib, hashlib, shutil, tomllib, xml.etree.ElementTree as ET, time
repo = pathlib.Path(__file__).resolve().parents[4]
fixtures = repo / 'crates/caskade-installer/tests/fixtures'
work = pathlib.Path(tempfile.mkdtemp(prefix='caskade-t7-wine-golden-'))
env = os.environ | {'WINEPREFIX': str(work / 'prefix'), 'WINEDEBUG': '-all', 'WINEDLLOVERRIDES': 'winemenubuilder.exe=d', 'XDG_DATA_HOME': str(work / 'xdg-data'), 'XDG_CONFIG_HOME': str(work / 'xdg-config')}
sequence = 0
reports = []
succeeded = False
report_path = repo / 'target/native-windows/wine-pre-evidence.json'

def win(path):
    return 'Z:' + str(path).replace('/', '\\')

def run(args):
    global sequence
    sequence += 1
    log = work / f'command-{sequence}.log'
    with log.open('wb') as output:
        # A persistent wineserver must not inherit the lock file descriptor.
        # flock keeps its parent lease until this command exits, then releases it.
        r = subprocess.run(['flock', '--close', '/tmp/caskade-heavy.lock', *args], env=env, stdout=output, stderr=subprocess.STDOUT, timeout=120)
    return (r.returncode, log.read_bytes().decode('cp437', errors='replace'))

def batch(text):
    file = work / f'command-{sequence + 1}.cmd'
    file.write_bytes(('@echo off\r\n' + text + '\r\n').encode('cp437'))
    return run(['wine', 'cmd', '/c', win(file)])

def tree(root, omit):
    entries = []
    for path in sorted(root.rglob('*')):
        rel = path.relative_to(root).as_posix()
        if rel in omit:
            continue
        entry = {'path': rel, 'kind': 'directory' if path.is_dir() else 'file', 'size': 0, 'executable': False}
        if path.is_file():
            data = path.read_bytes()
            entry['size'] = len(data)
            entry['sha256'] = hashlib.sha256(data).hexdigest()
        entries.append(entry)
    return sorted(entries, key=lambda e: e['path'])
print('PRIVATE_WORK', work, flush=True)
try:
    assert run(['wineboot', '-u'])[0] == 0
    run(['wineserver', '-p'])
    for path in sorted((fixtures / 'expected/native-windows').glob('*.toml')):
        fixture = tomllib.loads(path.read_text())
        ident = fixture['id']
        family = fixture['format']
        dest = work / ('Café Space ' + ident)
        dest.mkdir()
        directory = win(dest)
        source = win(fixtures / fixture['input'])
        if family == 'inno':
            command = f'"{source}" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /NOICONS /LANG=english /DIR="{directory}"'
        elif family == 'nsis':
            command = f'"{source}" /S /CASKADE_NO_SHORTCUTS=1 /D={directory}'
        else:
            command = f'msiexec /i "{source}" /qn /norestart "INSTALLDIR={directory}" "TARGETDIR={directory}" CASKADE_NOSHORTCUTS=1'
        code, out = batch(command)
        until = time.monotonic() + 20
        while not (dest / 'FixtureGame.exe').exists() and time.monotonic() < until:
            time.sleep(0.05)
        for generated in fixture['windows']['native_generated']:
            assert (dest / generated).is_file(), (ident, 'missing generated', generated)
        actual = tree(dest, fixture['windows']['native_generated'])
        expected = json.loads((fixtures / 'expected' / fixture['tree']).read_text())
        assert code == 0, (ident, code, out)
        assert actual == expected, (ident, 'golden mismatch', actual, expected, out)
        custom = 'HKCU\\Software\\CaskadeT7\\' + ident
        assert run(['wine', 'reg', 'query', custom, '/v', 'InstallPath'])[0] == 0
        code_guid = None
        if family == 'msi':
            source_xml = fixtures / 'src/native-windows' / f'{ident}.wxs'
            code_guid = ET.parse(source_xml).getroot().find('{http://schemas.microsoft.com/wix/2006/wi}Product').attrib['Id']
        uninstall_id = code_guid if family == 'msi' else ident + ('_is1' if family == 'inno' else '')
        uninstall_suffix = '\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\' + uninstall_id
        registrations = [(hive + uninstall_suffix, view) for hive in ['HKCU', 'HKLM'] for view in ['/reg:32', '/reg:64']
                         if run(['wine', 'reg', 'query', hive + uninstall_suffix, view])[0] == 0]
        assert registrations, (ident, 'missing Uninstall entry')
        links = [str(p) for p in (work / 'prefix').rglob('Caskade T7 ' + ident + '.lnk')]
        assert not links, (ident, 'shortcut created', links)
        if family == 'inno':
            uninstaller = win(dest / 'unins000.exe')
            uninstall = f'"{uninstaller}" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART'
        elif family == 'nsis':
            uninstaller = win(dest / 'Uninstall.exe')
            uninstall = f'"{uninstaller}" /S _?={directory}'
        else:
            uninstall = f'msiexec /x {code_guid} /qn /norestart'
        removed, log = batch(uninstall)
        assert removed == 0, (ident, removed, log)
        until = time.monotonic() + 10
        while dest.exists() and any(dest.iterdir()) and (time.monotonic() < until):
            time.sleep(0.05)
        remaining = [p for p in dest.rglob('*') if p.is_file()] if dest.exists() else []
        assert all(p.relative_to(dest).as_posix() in fixture['windows']['native_generated'] for p in remaining), (ident, 'payload left over', remaining)
        for artifact in remaining: artifact.unlink()
        if dest.exists(): shutil.rmtree(dest)
        assert run(['wine', 'reg', 'query', custom, '/v', 'InstallPath'])[0] != 0, (ident, 'registry left over')
        assert all(run(['wine', 'reg', 'query', key, view])[0] != 0 for key, view in registrations), (ident, 'Uninstall registration left over')
        report = {'fixture': ident, 'version': fixture['windows']['version'], 'runner': 'Wine (not native Windows)', 'golden': fixture['tree'], 'golden_entries': len(expected), 'no_shortcuts': True, 'registry_removed': True, 'files_removed': True, 'install_exit': code, 'uninstall_exit': removed}
        reports.append(report)
        print(json.dumps(report), flush=True)
    assert len(reports) == 6
    succeeded = True
finally:
    run(['wineserver', '-k'])
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps({'runner': 'Wine (not native Windows)', 'results': reports}, indent=2) + '\n')
    if succeeded:
        shutil.rmtree(work)
print('COMPLETE', len(reports), 'REPORT', report_path, flush=True)
