"""Release gates reject a stale native build, a changed bundle identity or updater trust key."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("release_metadata", Path(__file__).with_name("check-release-metadata.py"))
metadata = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(metadata)

CONFIG = """\
productName = "Caskade"
identifier = "dev.caskade.app"
mainBinaryName = "caskade"

[plugins.deep-link.desktop]
schemes = ["caskade"]

[plugins.updater]
pubkey = "../../crates/caskade-app/updater.pub"
"""


class ReleaseMetadataTests(unittest.TestCase):
    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.root = Path(folder.name)
        self.write(metadata.MANIFEST, '[package]\nname = "caskade-native"\nversion = "1.2.3"\n')
        self.write("Cargo.lock", '[[package]]\nname = "caskade-native"\nversion = "1.2.3"\n\n'
                   '[[package]]\nname = "caskade-app"\nversion = "0.1.0"\n')
        self.write(metadata.BUNDLE_CONFIG, CONFIG)
        self.write(metadata.PUBLIC_KEY, "release key\n")

    def write(self, path, text):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def errors(self, version=None):
        return metadata.check(self.root, version)[1]

    def test_matching_versions_pass_with_or_without_a_tag(self):
        self.assertEqual(metadata.check(self.root), ("1.2.3", []))
        self.assertEqual(metadata.check(self.root, "1.2.3"), ("1.2.3", []))

    def test_a_tag_for_another_version_is_refused(self):
        errors = self.errors("1.2.4")
        self.assertEqual(len(errors), 2)
        self.assertTrue(all("expected 1.2.4" in error for error in errors))

    def test_the_lockfile_entry_is_checked(self):
        self.write("Cargo.lock", '[[package]]\nname = "caskade-native"\nversion = "1.2.2"\n')
        errors = self.errors()
        self.assertEqual(len(errors), 1)
        self.assertIn("Cargo.lock (caskade-native)", errors[0])

    def test_a_version_in_the_bundle_config_must_match(self):
        self.write(metadata.BUNDLE_CONFIG, 'version = "1.2.2"\n' + CONFIG)
        errors = self.errors()
        self.assertEqual(len(errors), 1)
        self.assertIn(metadata.BUNDLE_CONFIG, errors[0])
        self.write(metadata.BUNDLE_CONFIG, 'version = "1.2.3"\n' + CONFIG)
        self.assertEqual(self.errors(), [])

    def test_the_bundle_identity_is_kept(self):
        for line in ('productName = "Caskade"', 'identifier = "dev.caskade.app"', 'mainBinaryName = "caskade"',
                     'schemes = ["caskade"]'):
            with self.subTest(line=line):
                self.write(metadata.BUNDLE_CONFIG, CONFIG.replace(line, line.replace("aska", "asca")))
                errors = self.errors()
                self.assertEqual(len(errors), 1, errors)
                self.assertIn(metadata.BUNDLE_CONFIG, errors[0])

    def test_the_bundle_config_names_the_one_trust_key(self):
        for pubkey in ("another key", "../../crates/caskade-app/other.pub", None):
            with self.subTest(pubkey=pubkey):
                old = 'pubkey = "../../crates/caskade-app/updater.pub"'
                self.write(metadata.BUNDLE_CONFIG, CONFIG.replace(old, "" if pubkey is None else f'pubkey = "{pubkey}"'))
                errors = self.errors()
                self.assertEqual(len(errors), 1)
                self.assertIn(metadata.PUBLIC_KEY, errors[0])

    def test_an_empty_trust_key_is_refused(self):
        self.write(metadata.PUBLIC_KEY, "\n")
        self.assertTrue(self.errors())


if __name__ == "__main__":
    unittest.main()
