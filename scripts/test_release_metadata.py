"""Release gates reject a stale native build or a changed updater trust key."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("release_metadata", Path(__file__).with_name("check-release-metadata.py"))
metadata = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(metadata)


class ReleaseMetadataTests(unittest.TestCase):
    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.root = Path(folder.name)
        self.config = {"version": "1.2.3", "plugins": {"updater": {"pubkey": "release key"}}}
        self.write("apps/desktop/src-tauri/tauri.conf.json", json.dumps(self.config))
        self.write("apps/desktop/package.json", '{"version": "1.2.3"}')
        for path in ("apps/native/Cargo.toml", "apps/desktop/src-tauri/Cargo.toml"):
            self.write(path, '[package]\nversion = "1.2.3"\n')
        self.write("Cargo.lock", '\n'.join(
            f'[[package]]\nname = "{name}"\nversion = "1.2.3"\n'
            for name in ("caskade-native", "caskade-desktop")))
        self.write(metadata.PUBLIC_KEY, "release key\n")

    def write(self, path, text):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def test_matching_versions_and_trust_keys_pass_with_or_without_a_tag(self):
        self.assertEqual(metadata.check(self.root), ("1.2.3", []))
        self.assertEqual(metadata.check(self.root, "1.2.3"), ("1.2.3", []))

    def test_a_tag_for_another_version_is_refused(self):
        _, errors = metadata.check(self.root, "1.2.4")
        self.assertEqual(len(errors), 6)
        self.assertTrue(all("expected 1.2.4" in error for error in errors))

    def test_each_app_version_including_native_and_its_lock_entry_is_checked(self):
        paths = ("apps/native/Cargo.toml", "apps/desktop/src-tauri/Cargo.toml",
                 "apps/desktop/package.json", "apps/desktop/src-tauri/tauri.conf.json")
        for path in paths:
            with self.subTest(path=path):
                original = (self.root / path).read_text()
                self.write(path, original.replace("1.2.3", "1.2.2"))
                _, errors = metadata.check(self.root, "1.2.3")
                self.assertTrue(any(path in error for error in errors), errors)
                self.write(path, original)

    def test_each_locked_executable_version_is_checked_independently(self):
        original = (self.root / "Cargo.lock").read_text()
        for name in ("caskade-native", "caskade-desktop"):
            with self.subTest(name=name):
                self.write("Cargo.lock", original.replace(
                    f'name = "{name}"\nversion = "1.2.3"',
                    f'name = "{name}"\nversion = "1.2.2"'))
                _, errors = metadata.check(self.root)
                self.assertEqual(len(errors), 1)
                self.assertIn(f"Cargo.lock ({name})", errors[0])

    def test_a_different_or_missing_legacy_trust_key_is_refused(self):
        for key in ("another key", None):
            with self.subTest(key=key):
                self.config["plugins"]["updater"] = {} if key is None else {"pubkey": key}
                self.write("apps/desktop/src-tauri/tauri.conf.json", json.dumps(self.config))
                _, errors = metadata.check(self.root)
                self.assertEqual(len(errors), 1)
                self.assertIn(metadata.PUBLIC_KEY, errors[0])

    def test_an_empty_canonical_trust_key_is_refused(self):
        self.write(metadata.PUBLIC_KEY, "\n")
        self.assertTrue(metadata.check(self.root)[1])


if __name__ == "__main__":
    unittest.main()
