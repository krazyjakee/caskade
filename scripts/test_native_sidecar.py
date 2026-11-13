import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("native_sidecar", Path(__file__).with_name("native-sidecar.py"))
sidecar = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sidecar)
extractor_spec = importlib.util.spec_from_file_location("tauri_sidecar", Path(__file__).with_name("tauri-sidecar.py"))
extractor = importlib.util.module_from_spec(extractor_spec)
extractor_spec.loader.exec_module(extractor)


class NativeSidecarTests(unittest.TestCase):
    def test_native_dev_stages_the_extractor_and_windows_companion(self):
        for target, suffix in [("x86_64-unknown-linux-gnu", ""), ("x86_64-pc-windows-msvc", ".exe")]:
            with self.subTest(target=target), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                source = root / f"7zz-{target}{suffix}"
                source.write_bytes(b"verified extractor")
                source.chmod(0o755)
                source.with_name("7z.dll").write_bytes(b"companion")
                extractor.stage_dev({"program": str(source), "target": target}, root / "custom-target")
                staged = root / "custom-target/debug" / f"7zz{suffix}"
                self.assertEqual(staged.read_bytes(), source.read_bytes())
                self.assertEqual(staged.stat().st_mode, source.stat().st_mode)
                self.assertEqual(staged.with_name("7z.dll").exists(), bool(suffix))

    def test_host_and_cross_builds_use_the_right_cargo_directory_and_bundle_name(self):
        for target, host, profile, suffix in [
            (None, "x86_64-unknown-linux-gnu", "debug", ""),
            ("x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu", "release", ".exe"),
            ("aarch64-apple-darwin", "x86_64-apple-darwin", "release", ""),
        ]:
            with self.subTest(target=target), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                build = root / "custom-target"
                source = (build / target if target else build) / profile / f"caskade-native{suffix}"
                source.parent.mkdir(parents=True)
                source.write_bytes(b"native app")
                source.chmod(0o755)
                destination = sidecar.stage(build, root / "binaries", profile, target, host)
                self.assertEqual(destination.name, f"caskade-native-{target or host}{suffix}")
                self.assertEqual(destination.read_bytes(), source.read_bytes())
                self.assertEqual(destination.stat().st_mode, source.stat().st_mode)

    def test_missing_build_does_not_reuse_a_stale_sidecar(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            stale = root / "binaries/caskade-native-x86_64-unknown-linux-gnu"
            stale.parent.mkdir()
            stale.write_bytes(b"old app")
            with self.assertRaises(FileNotFoundError):
                sidecar.stage(root / "target", stale.parent, "debug", host="x86_64-unknown-linux-gnu")
            self.assertEqual(stale.read_bytes(), b"old app")


if __name__ == "__main__":
    unittest.main()
