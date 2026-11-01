#!/usr/bin/env python3
"""T9 real CLI lifecycle scoreboard; simulated host plans are labelled as such."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import platform
import subprocess
import tempfile
import time
import tomllib
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / 'crates/caskade-installer/tests/fixtures'
MINIS = {
    **{name: f'windows/{name}.toml' for name in [
        'gog-inno-unity', 'gog-inno-sliced', 'gog-inno5-legacy', 'legacy-registry-path',
        'nsis-indie', 'msi-plus-cab', 'humble-win-zip', 'unreal-zip', 'steamlike-commonredist']},
    **{name: f'linux-mini/{name}.toml' for name in [
        'gog-linux-mojosetup', 'humble-linux-tar', 'itch-zip-with-manifest',
        'appimage-indie', 'deb-missing-dep', 'rpm-indie', 'flatpak-bundle']},
    'rar-multivolume-portable': 'rar-multivolume-portable.toml',
    'iso-pc-disc': 'disc/iso-pc-disc.toml', 'ps1-cue-bin': 'disc/ps1-cue-bin.toml',
    'mac-pkg': 'macos-pkg/mac-pkg.toml',
    'mac-dmg-sla': 'macos-native/mac-dmg-sla.toml',
    'burn-bundle': 'burn/burn-bundle.toml',
}
NATIVE = {
    'deb-missing-dep': 'deb_missing_dep_falls_back_to_apt_inside_disposable_debian',
    'flatpak-bundle': 'flatpak_bundle_launches_inside_its_sandbox_and_uninstalls_without_user_state',
}

def native_results(path):
    if path is None or not path.is_file():
        return {}
    results = {}
    for case in ET.parse(path).iter('testcase'):
        name = case.attrib.get('name', '')
        results[name] = {'status': 'failed' if case.find('failure') is not None or case.find('error') is not None else 'not_run' if case.find('skipped') is not None else 'passed',
                         'lifecycle_ms': float(case.attrib.get('time', '0')) * 1000}
    return results

def execute(binary, sidecar, args, logs, label):
    command = [str(binary), '--json']
    if sidecar:
        command += ['--seven-zip', str(sidecar)]
    command += [str(arg) for arg in args]
    start = time.perf_counter()
    result = subprocess.run(command, capture_output=True, text=True, timeout=600)
    elapsed = (time.perf_counter() - start) * 1000
    logs.mkdir(parents=True, exist_ok=True)
    (logs / f'{label}.stdout.json').write_text(result.stdout)
    (logs / f'{label}.stderr.log').write_text(result.stderr)
    try:
        value = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(f'{label}: non-JSON CLI result, exit {result.returncode}') from error
    return value, elapsed, result.returncode

def verdict(outcome, expected, code):
    observed = outcome.get('type', 'invalid')
    if observed == 'needs_user':
        return 'failed'
    wanted = {'installed': 'installed', 'handoff': 'hand_off', 'unsupported': 'unsupported'}.get(expected.lower().replace('_', ''))
    if wanted is None:
        return 'failed'
    if observed != wanted:
        return 'failed'
    # Unsupported is an expected result with its own documented CLI exit code.
    return 'passed' if code == (3 if wanted == 'unsupported' else 0) else 'failed'

def cycle(binary, sidecar, item, actual_host, output, native):
    name, expected = item
    rows = []
    with tempfile.TemporaryDirectory(prefix='caskade-scoreboard-') as temporary:
        storage = Path(temporary) / 'root'
        storage.mkdir()
        source = FIXTURES / expected['input']
        before = {part: hashlib.sha256((FIXTURES / part).read_bytes()).hexdigest() for part in expected['parts']}
        for host in ['linux', 'windows', 'macos']:
            row = {'installer': name, 'host': host, 'stage': 'plan', 'status': 'not_run',
                   'expected': expected.get('host', {}).get(host, {}).get('outcome', 'Unsupported'),
                   'install_ms': None, 'needs_user': False}
            if expected['platform'] == 'macos' and host != 'macos':
                row['fixture_static_outcome'] = row['expected']
                row['expected'] = 'Unsupported'
                row['policy_note'] = 'T4 can extract macOS bytes across hosts; T9 native platform policy cannot install/launch a macOS app on this host.'
            logs = output / 'logs' / name / host
            planned, elapsed, code = execute(binary, sidecar, ['plan', source, '--host', host, '--root', storage], logs, 'plan')
            plans = planned.get('plans', {})
            row['plan_ms'] = elapsed
            row['plan_strategy'] = next((candidate['strategy'] for index, candidate in enumerate(plans.get('candidates', [])) if index == plans.get('selected')), None)
            row['plan_needs_user'] = bool(plans.get('question'))
            row['needs_user'] = row['plan_needs_user']
            if code != 0:
                row['status'] = 'failed'
            if host != actual_host:
                # Host profiles test pure planning; never claim a foreign host install.
                rows.append(row)
                continue
            if name in NATIVE and actual_host == 'linux':
                suffix = NATIVE[name]
                result = next((value for key, value in native.items() if key.endswith(suffix)), None)
                row['stage'] = 'native_test'
                row['status'] = result['status'] if result else 'not_run'
                row['native_lifecycle_ms'] = result['lifecycle_ms'] if result else None
                row['install_time_note'] = 'Native JUnit duration includes isolated setup, install, launch and uninstall; not an isolated install timer.'
                rows.append(row)
                continue
            outcome, elapsed, code = execute(binary, sidecar, ['install', source, '--extract', '--root', storage], logs, 'install')
            row.update(stage='install', install_ms=elapsed, observed=outcome.get('type'), needs_user=outcome.get('type') == 'needs_user')
            row['status'] = verdict(outcome, row['expected'], code)
            if outcome.get('type') == 'installed' and code == 0:
                game = storage / outcome['value']['dir']
                # The CLI persists a manifest; check payload hashes independently.
                document = json.loads((game / '.caskade/install.json').read_text())
                record = document.get('record', document)
                for entry in record['files']:
                    path = game / entry['path']
                    if not path.is_file() or path.stat().st_size != entry['size']:
                        raise RuntimeError(f'{name}: missing/changed installed file {entry["path"]}')
                    if entry.get('sha256') and hashlib.sha256(path.read_bytes()).hexdigest() != entry['sha256']:
                        raise RuntimeError(f'{name}: hash differs for {entry["path"]}')
                removed, _, remove_code = execute(binary, sidecar, ['uninstall', game], logs, 'uninstall')
                if remove_code or removed.get('type') != 'uninstalled' or game.exists():
                    row['status'] = 'failed'
                # Root identity intentionally survives an install/uninstall cycle.
                remaining = sorted(str(path.relative_to(storage)) for path in storage.rglob('*'))
                if remaining != ['.caskade', '.caskade/root.json']:
                    raise RuntimeError(f'{name}: uninstall left unexpected files: {remaining}')
            rows.append(row)
        after = {part: hashlib.sha256((FIXTURES / part).read_bytes()).hexdigest() for part in expected['parts']}
        if before != after:
            raise RuntimeError(f'{name}: local source changed')
    return rows

def render(report):
    lines = ['# Mini-installer scoreboard', '',
             f'Generated {report["generated_at"]}; actual host: **{report["actual_host"]}**.',
             'Foreign host rows are pure plan observations, not native install passes.',
             '', '| Installer | Host | Stage | Result | Install ms | NeedsUser |',
             '| --- | --- | --- | --- | ---: | --- |']
    for row in report['rows']:
        duration = '—' if row.get('install_ms') is None else f'{row["install_ms"]:.2f}'
        lines.append(f'| {row["installer"]} | {row["host"]} | {row["stage"]} | {row["status"]} | {duration} | {row.get("needs_user", False)} |')
    lines += ['', f'Failures: {report["failed"]}; NeedsUser: {report["needs_user"]}; pending fixture families: {len(report["pending_fixtures"])}; native delegates not run: {report["native_not_run"]}.',
              'Native delegated lifecycle timings (including setup/launch/uninstall) are separate JSON fields; unavailable install timings remain null.']
    return '\n'.join(lines) + '\n'

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--seven-zip', type=Path)
    parser.add_argument('--output', type=Path, default=ROOT / 'target/scoreboard')
    parser.add_argument('--native-junit', type=Path)
    parser.add_argument('--only', help='comma-separated ids for a focused local run')
    args = parser.parse_args()
    actual = {'Linux': 'linux', 'Windows': 'windows', 'Darwin': 'macos'}[platform.system()]
    binary = args.binary.resolve(strict=True)
    args.output.mkdir(parents=True, exist_ok=True)
    native = native_results(args.native_junit)
    rows, pending = [], []
    for name, relative in sorted(MINIS.items()):
        if args.only and name not in args.only.split(','):
            continue
        path = FIXTURES / 'expected' / relative
        if not path.is_file():
            pending.append(name)
            continue
        expected = tomllib.loads(path.read_text())
        try:
            rows.extend(cycle(binary, args.seven_zip, (name, expected), actual, args.output, native))
        except (RuntimeError, OSError, subprocess.TimeoutExpired) as error:
            rows.append({'installer':name,'host':actual,'stage':'error','status':'failed','install_ms':None,'error':str(error),'needs_user':False})
    report = {'schema_version':1,'generated_at':datetime.now(timezone.utc).isoformat(),'actual_host':actual,
              'rows':rows,'pending_fixtures':pending,'failed':sum(row['status']=='failed' for row in rows),
              'needs_user':sum(row.get('needs_user',False) for row in rows),
              'native_not_run':sum(row['stage']=='native_test' and row['status']=='not_run' for row in rows),
              'native_junit':str(args.native_junit) if args.native_junit else None,
              'revision':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
              'fixture_source_sha256':tomllib.loads((FIXTURES/'build.lock').read_text())['source_sha256']}
    (args.output / 'scoreboard.json').write_text(json.dumps(report, indent=2)+'\n')
    (args.output / 'scoreboard.md').write_text(render(report))
    print(f'{len(rows)} rows, {report["failed"]} failures, {report["needs_user"]} NeedsUser, pending={pending}, native_not_run={report["native_not_run"]}; {args.output}')
    raise SystemExit(bool(report['failed'] or report['needs_user'] or report['native_not_run']))

if __name__ == '__main__':
    main()
