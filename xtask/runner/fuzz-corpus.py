#!/usr/bin/env python3
"""Bounded corpus retention; a dedicated writable corpus volume is not a tool cache."""
import argparse
import hashlib
import os
from pathlib import Path
import re
import tempfile

MAX_INPUT = 1024 * 1024
MAX_TOTAL = 256 * 1024 * 1024
MAX_FILES = 100000
NAME = re.compile(r'(?:seed-)?[0-9a-f]{40,64}\Z')

def merge(source, destination):
    if source.is_symlink() or destination.is_symlink():
        raise ValueError('corpus directories must not be symlinks')
    destination.mkdir(parents=True, exist_ok=True)
    retained = 0
    total = 0
    # Re-hash contents into a single flat namespace, rejecting links/special
    # files and bounding both reads and persistent storage on every run.
    candidates = []
    for directory in (destination, source):
        if not directory.exists():
            continue
        for path in directory.iterdir():
            if path.is_symlink() or not path.is_file() or not NAME.fullmatch(path.name):
                continue
            if path.stat().st_size <= MAX_INPUT:
                candidates.append(path)
            if len(candidates) >= 2 * MAX_FILES:
                break
    seen = set()
    for path in sorted(candidates, key=lambda item: item.name):
        with path.open('rb') as stream:
            data = stream.read(MAX_INPUT + 1)
        if not data or len(data) > MAX_INPUT:
            continue
        digest = hashlib.sha1(data, usedforsecurity=False).hexdigest()
        if digest in seen or retained == MAX_FILES or total + len(data) > MAX_TOTAL:
            continue
        seen.add(digest)
        target = destination / digest
        # Always atomically replace bytes; never trust cached filenames.
        descriptor, temporary = tempfile.mkstemp(prefix='.corpus-', dir=destination)
        try:
            with os.fdopen(descriptor, 'wb') as output:
                output.write(data)
            os.replace(temporary, target)
        finally:
            Path(temporary).unlink(missing_ok=True)
        retained += 1
        total += len(data)
    for path in destination.iterdir():
        if path.is_symlink() or path.is_file() and path.name not in seen:
            path.unlink()
    print(f'{destination}: {retained} regular corpus inputs, {total}/{MAX_TOTAL} bytes')

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('destination', type=Path)
    args = parser.parse_args()
    merge(args.source, args.destination)
if __name__ == '__main__':
    main()
