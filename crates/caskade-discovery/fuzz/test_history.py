"""Unit fixtures are fabricated validator inputs, never nightly evidence."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('discovery_history', Path(__file__).with_name('history.py'))
history = importlib.util.module_from_spec(spec)
spec.loader.exec_module(history)


def records(days):
    result = []
    for day in range(1, days + 1):
        for target in ['vdf', 'epic']:
            result.append({'target': target, 'exit_code': 0, 'requested_seconds': 600,
                           'executions': 100, 'finished': f'2026-01-{day:02d}T03:00:00+00:00'})
        result.append({'metric': 'discovery1000', 'finished_unix': 1767225600 + (day - 1) * 86400,
                       'profiles': [{'profile': profile, 'generated_steam_games': 1000,
                                     'generated_steam_libraries': 10, 'elapsed_ms': 1500}
                                    for profile in ['Linux', 'Windows', 'Macos']]})
    return result


class HistoryTests(unittest.TestCase):
    def test_one_day_is_never_a_green_week(self):
        self.assertFalse(history.summarize(records(1), ['vdf', 'epic'])['passed'])

    def test_seven_complete_days_pass_but_missing_target_or_failed_budget_does_not(self):
        self.assertTrue(history.summarize(records(7), ['vdf', 'epic'])['passed'])
        missing = records(7)
        del missing[4]
        self.assertFalse(history.summarize(missing, ['vdf', 'epic'])['passed'])
        slow = records(7)
        slow[-1]['profiles'][0]['elapsed_ms'] = 2000
        self.assertFalse(history.summarize(slow, ['vdf', 'epic'])['passed'])

    def test_short_smoke_runs_and_failed_fuzz_do_not_count(self):
        for field, value in [('requested_seconds', 60), ('executions', 0), ('exit_code', 1)]:
            data = records(7)
            data[0][field] = value
            self.assertFalse(history.summarize(data, ['vdf', 'epic'])['passed'])

    def test_symlink_evidence_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'real').write_text('{}')
            (root / 'result.json').symlink_to(root / 'real')
            with self.assertRaises(ValueError):
                history.read_records(root)


if __name__ == '__main__':
    unittest.main()
