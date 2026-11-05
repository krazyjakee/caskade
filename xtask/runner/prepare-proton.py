#!/usr/bin/env python3
"""Copy readonly native tools into a writable job-local XDG tree; never change HOME.

Cache contract: <cache>/native/share/{caskade/umu,umu,Steam/compatibilitytools.d}.
Provision cache on the trusted runner once using pinned UmuBootstrap and umu's
UMU-Proton download. A missing cache fails loudly instead of silently rerunning
a multi-gigabyte first-download during every installer test.

The trees copied are this script's own: whatever is at those paths is replaced,
so a leftover run cannot fail the copy or mix with the cache. Nothing else under
--destination is removed, and links are unlinked, never followed.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import stat
import re

DIRECTORIES = ['caskade/umu', 'umu', 'Steam/compatibilitytools.d']

def writable(path):
    path.chmod(stat.S_IMODE(path.lstat().st_mode) | stat.S_IRWXU)

def clear(share, relative):
    """Remove share/relative. A stale link anywhere on the way is itself removed:
    following it could reach the source cache, and chmod and mkdir both would."""
    path = share
    for part in ('.', *Path(relative).parts):
        path = path / part
        if path.is_symlink() or (path.exists() and not path.is_dir()):
            path.unlink()
        elif path.exists():
            writable(path)
    if path.exists():
        for root, directories, _ in os.walk(path):
            for name in directories:
                if not Path(root, name).is_symlink():
                    writable(Path(root, name))
        shutil.rmtree(path)

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
    # The last component stays unresolved: if it is a stale link, clear() removes
    # the link instead of the tree it points at.
    absolute = Path(os.path.abspath(destination))
    share = absolute.parent.resolve() / absolute.name
    origin = source.resolve()
    if share == origin or share in origin.parents or origin in share.parents:
        raise ValueError(f'destination {share} overlaps the native runner cache {origin}')
    for relative in DIRECTORIES:
        clear(share, relative)
        shutil.copytree(source / relative, share / relative, symlinks=True)
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
