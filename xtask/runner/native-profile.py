#!/usr/bin/env python3
"""Run native suites with Proton unprivileged inside the fresh CI job container."""
import argparse
import os
from pathlib import Path
import pwd
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--proton', action='store_true')
    args = parser.parse_args()
    environment = os.environ.copy()
    environment['CASKADE_TEST_NATIVE'] = '1'
    command = ['flock', '/tmp/caskade-heavy.lock', 'cargo', 'nextest', 'run', '-p', 'caskade-installer',
               '--all-features', '--locked', '--profile', 'native', '--run-ignored', 'all', '-E',
               'binary(/^(proton|perf_proton|moving_proton)$/)' if args.proton else 'not (binary(/^(proton|perf_proton|moving_proton)$/))']
    if args.proton and environment.get('GITHUB_ACTIONS') == 'true':
        command = ['xvfb-run', '-a', *command]
    if args.proton and os.geteuid() == 0:
        # Only change ownership inside a fresh Actions job container. A local
        # root invocation must never recursively chown a developer checkout.
        if environment.get('GITHUB_ACTIONS') != 'true' or not environment.get('GITHUB_WORKSPACE'):
            parser.error('root Proton execution requires a fresh Actions job container; run locally as an ordinary user')
        user = pwd.getpwnam('caskade')
        if ROOT != Path(environment['GITHUB_WORKSPACE']).resolve():
            parser.error('native runner workspace differs from the checked-out repository')
        share = Path(environment['XDG_DATA_HOME'])
        # Cargo/Rust from the documented image remain immutable. Registry state
        # and package downloads use this private writable job directory.
        cargo = ROOT / 'target/native-cargo'
        cargo.mkdir(parents=True, exist_ok=True)
        environment['CARGO_HOME'] = str(cargo)
        subprocess.run(['chown', '-R', f'{user.pw_uid}:{user.pw_gid}', str(ROOT), str(share.parent)], check=True)
        # runuser resets HOME from passwd; no HOME variable is assigned by us.
        command = ['runuser', '-u', 'caskade', '--', 'env',
                   f'CARGO_HOME={cargo}', f'PATH={environment["PATH"]}', *command]
    # nextest's default store is workspace target/nextest, independently of
    # Cargo's compilation target directory. Keep workflow receipts beside it.
    target = ROOT / 'target'
    source = target / 'nextest/native/junit.xml'
    receipt = target / ('proton-junit.xml' if args.proton else 'native-junit.xml')
    source.unlink(missing_ok=True)
    receipt.unlink(missing_ok=True)
    result = subprocess.run(command, cwd=ROOT, env=environment)
    if source.is_file():
        shutil.copyfile(source, receipt)
    result.check_returncode()

if __name__ == '__main__':
    main()
