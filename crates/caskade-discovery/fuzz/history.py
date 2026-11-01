#!/usr/bin/env python3
"""Verify elapsed nightly evidence; never infer a green week from configured CI."""
import argparse
from datetime import datetime, timedelta, timezone
import json
import os
from pathlib import Path
import tomllib

HERE = Path(__file__).resolve().parent
MAX_JSON = 1024 * 1024


def summarize(records, targets, required_days=7):
    if not 1 <= required_days <= 365:
        raise ValueError('required days must be in 1..365')
    days = {}
    for record in records:
        if record.get('target') in targets:
            if (record.get('exit_code') != 0 or record.get('requested_seconds', 0) < 600
                    or record.get('executions', 0) < 1):
                continue
            day = datetime.fromisoformat(record['finished']).astimezone(timezone.utc).date()
            state = days.setdefault(day, {'targets': set(), 'performance': False})
            state['targets'].add(record['target'])
        elif record.get('metric') == 'discovery1000':
            profiles = record.get('profiles', [])
            expected = {'Linux', 'Windows', 'Macos'}
            if (set(p.get('profile') for p in profiles) != expected or len(profiles) != 3
                    or not all(p.get('generated_steam_games') == 1000
                               and p.get('generated_steam_libraries') == 10
                               and 0 <= p.get('elapsed_ms', 2000) < 2000
                               for p in profiles)):
                continue
            day = datetime.fromtimestamp(record['finished_unix'], timezone.utc).date()
            days.setdefault(day, {'targets': set(), 'performance': False})['performance'] = True
    complete = sorted(day for day, state in days.items()
                      if state['performance'] and state['targets'] >= set(targets))
    longest = streak = 0
    previous = None
    for day in complete:
        streak = streak + 1 if previous and day == previous + timedelta(days=1) else 1
        longest = max(longest, streak)
        previous = day
    return {'schema_version': 1, 'required_days': required_days,
            'complete_utc_days': [day.isoformat() for day in complete],
            'longest_consecutive_days': longest, 'passed': longest >= required_days}


def read_records(directory):
    records = []
    visited = 0
    for root, directories, names in os.walk(directory, followlinks=False):
        visited += 1
        if visited > 10000:
            raise ValueError('evidence directory count limit exceeded')
        directories[:] = sorted(name for name in directories if not (Path(root) / name).is_symlink())
        for name in sorted(names):
            visited += 1
            if visited > 10000:
                raise ValueError('evidence file count limit exceeded')
            if name not in ('result.json', 'scan-1000.json'):
                continue
            path = Path(root) / name
            if path.is_symlink() or not path.is_file() or path.stat().st_size > MAX_JSON:
                raise ValueError('evidence must be bounded regular JSON files')
            with path.open('rb') as stream:
                data = stream.read(MAX_JSON + 1)
            if len(data) > MAX_JSON:
                raise ValueError('growing evidence file exceeds budget')
            records.append(json.loads(data))
    return records


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--required-days', type=int, default=7)
    args = parser.parse_args()
    if not args.directory.is_dir() or args.directory.is_symlink():
        parser.error('evidence directory must be a real directory')
    targets = sorted(item['name'] for item in tomllib.loads((HERE / 'Cargo.toml').read_text())['bin'])
    result = summarize(read_records(args.directory), targets, args.required_days)
    print(json.dumps(result, indent=2))
    raise SystemExit(0 if result['passed'] else 1)


if __name__ == '__main__':
    main()
