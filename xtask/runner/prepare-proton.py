#!/usr/bin/env python3
"""Copy readonly native tools into a writable job-local XDG tree; never change HOME.

Cache contract: <cache>/native/share/{caskade/umu,umu,Steam/compatibilitytools.d}.
Provision cache on the trusted runner once using pinned UmuBootstrap and umu's
UMU-Proton download. A missing cache fails loudly instead of silently rerunning
a multi-gigabyte first-download during every installer test.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import re

DIRECTORIES = ['caskade/umu', 'umu', 'Steam/compatibilitytools.d']

def prepare(cache, destination):
    source = cache / 'native/share'
    for relative in DIRECTORIES:
        path = source / relative
        if not path.is_dir():
            raise ValueError(f'missing native runner cache {path}; provision per xtask/runner/README.md')
    launcher = source / 'caskade/umu/1.4.4/umu/umu-run'
    if not launcher.is_file():
        raise ValueError('native cache has no pinned umu-launcher 1.4.4')
    builds = sorted((path for path in (source / 'Steam/compatibilitytools.d').glob('UMU-Proton*') if (path / 'proton').is_file()), key=lambda path: [int(part) if part.isdigit() else part.lower() for part in re.split(r'(\d+)', path.name)])
    if not builds:
        raise ValueError('cache has no UMU-Proton build; GE-Proton is not the native acceptance default')
    if not (source / 'umu/steamrt3').is_dir():
        raise ValueError('cache has no warmed umu Steam Runtime (steamrt3)')
    share = destination.resolve()
    share.mkdir(parents=True, exist_ok=True)
    for relative in DIRECTORIES:
        shutil.copytree(source / relative, share / relative, symlinks=True, dirs_exist_ok=True)
    proton = share / 'Steam/compatibilitytools.d' / builds[-1].name
    environment = {'XDG_DATA_HOME':str(share), 'PROTONPATH':str(proton),
                   'XDG_CACHE_HOME':str(share.parent / 'cache'), 'XDG_CONFIG_HOME':str(share.parent / 'config')}
    return {'schema_version':1, 'source_cache':str(source), 'umu_version':'1.4.4',
            'proton':builds[-1].name, 'runtime':'steamrt3', 'environment':environment}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cache', type=Path, default=Path('/caskade-cache'))
    parser.add_argument('--destination', type=Path, required=True)
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    report = prepare(args.cache, args.destination)
    args.evidence.parent.mkdir(parents=True, exist_ok=True)
    args.evidence.write_text(json.dumps(report, indent=2) + '\n')
    if output := os.environ.get('GITHUB_ENV'):
        with open(output, 'a') as stream:
            for key, value in report['environment'].items():
                if '\n' in value or '\r' in value:
                    raise ValueError('runner path contains a newline')
                stream.write(f'{key}={value}\n')
    print(json.dumps(report))
if __name__ == '__main__':
    main()
