"""Regression checks for the PR gate, including real Git merge-base diffs."""
import importlib.util
import contextlib
import io
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("ci_scope", Path(__file__).with_name("ci-scope.py"))
ci = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ci)


class ScopeTests(unittest.TestCase):
    def test_cli_emits_boolean_outputs_used_by_workflow_conditions(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "output"
            event = Path(temporary) / "event.json"
            event.write_text("{}")
            environment = {"GITHUB_OUTPUT": str(output), "GITHUB_EVENT_PATH": str(event),
                           "GITHUB_EVENT_NAME": "push"}
            with patch.dict(os.environ, environment), patch.object(sys, "argv", ["ci-scope.py"]), \
                    contextlib.redirect_stdout(io.StringIO()):
                ci.main()
            values = dict(line.split("=", 1) for line in output.read_text().splitlines())
            self.assertEqual(set(values), set(ci.FIELDS))
            self.assertEqual(values["bundle"], "false")
            self.assertTrue(all(value in ("true", "false") for value in values.values()))

    def test_prose_skips_heavy_jobs(self):
        self.assertFalse(any(ci.scope(["README.md", "docs/TECH-PLAN.md"]).values()))

    def test_frontend_change_does_not_select_native_rust_suites(self):
        scope = ci.scope(["apps/desktop/src/views/LibraryView.tsx"])
        self.assertTrue(scope["app"] and scope["frontend"] and scope["catalogs"])
        self.assertFalse(scope["rust"] or scope["installer"] or scope["discovery"] or scope["e2e"])

    def test_installer_change_reaches_discovery_transitively(self):
        scope = ci.scope(["crates/caskade-installer/src/lib.rs"])
        self.assertTrue(scope["installer"] and scope["discovery"] and scope["rust"])

    def test_shared_jobs_change_reaches_both_libraries(self):
        scope = ci.scope(["crates/caskade-jobs/src/lib.rs"])
        self.assertTrue(scope["installer"] and scope["discovery"])

    def test_core_does_not_rebuild_standalone_libraries(self):
        scope = ci.scope(["crates/caskade-core/src/lib.rs"])
        self.assertTrue(scope["rust"] and scope["cross"])
        self.assertFalse(scope["installer"] or scope["discovery"])

    def test_embedded_contracts_and_catalogs_need_rust_checks(self):
        for path in ["packages/ui/anatomy.json", "packages/tokens/tokens/sets/base.json",
                     "locales/fr/core.po", "docs/legal/privacy.md", "wit/caskade-plugin.wit",
                     "docs/errors.md"]:
            with self.subTest(path=path):
                self.assertTrue(ci.scope([path])["rust"])

    def test_generated_documentation_still_runs_its_freshness_checks(self):
        for path in ["docs/theming/anatomy.md", "docs/theming/tokens.md"]:
            with self.subTest(path=path):
                self.assertTrue(ci.scope([path])["frontend"])

    def test_unknown_build_inputs_and_new_crates_select_every_suite(self):
        for path in ["Cargo.lock", "Cargo.toml", "vendor/inno-0.7.1/src/lib.rs",
                     "scripts/ci-scope.py", ".github/workflows/app.yml",
                     "crates/new-crate/src/lib.rs", "docs/plugins/plugin.schema.json"]:
            with self.subTest(path=path):
                self.assertTrue(all(ci.scope([path]).values()))

    def test_master_keeps_full_checks_without_packaging(self):
        scope = ci.select("push", {})
        self.assertTrue(all(value for key, value in scope.items() if key != "bundle"))
        self.assertFalse(scope["bundle"])

    def test_nightly_and_explicit_manual_bundle(self):
        self.assertTrue(ci.select("schedule", {})["bundle"])
        for value in [True, "true"]:
            self.assertTrue(ci.select("workflow_dispatch", {"inputs": {"bundle": value}})["bundle"])
        self.assertFalse(ci.select("workflow_dispatch", {"inputs": {"bundle": "false"}})["bundle"])

    def test_missing_pr_history_runs_full_checks_instead_of_skipping(self):
        for event_name in ci.PR_EVENTS:
            with self.subTest(event_name=event_name):
                scope = ci.select(event_name, {})
                self.assertTrue(scope["rust"] and scope["installer"] and scope["discovery"])
                self.assertFalse(scope["bundle"])

    def test_pull_requests_are_scoped_by_their_diff(self):
        for event_name in ci.PR_EVENTS:
            with self.subTest(event_name=event_name), \
                    patch.object(ci, "changed_paths", return_value=["crates/caskade-discovery/src/lib.rs"]):
                scope = ci.select(event_name, {"pull_request": {"labels": []}})
                self.assertTrue(scope["discovery"] and scope["rust"] and scope["app"])
                self.assertFalse(scope["installer"] or scope["e2e"] or scope["bundle"])
            with self.subTest(event_name=event_name, paths="prose"), \
                    patch.object(ci, "changed_paths", return_value=["docs/README.md"]):
                self.assertFalse(any(ci.select(event_name, {"pull_request": {}}).values()))

    def test_workflow_changes_select_every_suite(self):
        for path in [".github/workflows/discovery.yml", ".github/workflows/i18n.yml",
                     ".config/nextest.toml", "scripts/discovery-ci.py"]:
            with self.subTest(path=path):
                self.assertTrue(all(ci.scope([path]).values()))

    def test_labels_allow_e2e_or_bundle_for_an_otherwise_unaffected_pr(self):
        for label in ["run-e2e", "run-bundle"]:
            with self.subTest(label=label), patch.object(ci, "changed_paths", return_value=["docs/README.md"]):
                scope = ci.select("pull_request", {"pull_request": {"labels": [{"name": label}]}})
                self.assertTrue(scope["app"] and scope["rust"] and scope["e2e"])
                self.assertEqual(scope["bundle"], label == "run-bundle")

    def test_dependency_graph_includes_target_build_dev_and_workspace_dependencies(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "Cargo.toml").write_text('[workspace]\nmembers=["crates/*"]\n'
                                             '[workspace.dependencies]\nshared={path="crates/shared"}\n')
            for name in ["shared", "build", "dev", "platform", "consumer"]:
                directory = root / "crates" / name
                directory.mkdir(parents=True)
                (directory / "Cargo.toml").write_text(f'[package]\nname="{name}"\nversion="0.1.0"\n')
            with (root / "crates/consumer/Cargo.toml").open("a") as stream:
                stream.write('[dependencies]\nshared={workspace=true}\n'
                             '[build-dependencies]\nbuild={path="../build"}\n'
                             '[dev-dependencies]\ndev={path="../dev"}\n'
                             '[target.\'cfg(windows)\'.dependencies]\nplatform={path="../platform"}\n')
            _, graph = ci.package_graph(root)
            self.assertEqual(graph["consumer"], {"shared", "build", "dev", "platform"})

    def test_real_diff_preserves_deleted_and_renamed_inputs(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)

            def git(*args):
                return subprocess.check_output(["git", *args], cwd=root, text=True).strip()

            git("init", "-q")
            git("config", "user.email", "test@example.invalid")
            git("config", "user.name", "CI scope test")
            (root / "old.rs").write_text("source")
            git("add", "old.rs")
            git("commit", "-qm", "initial")
            base = git("rev-parse", "HEAD")
            git("mv", "old.rs", "renamed.rs")
            git("commit", "-qm", "rename")
            head = git("rev-parse", "HEAD")
            event = {"pull_request": {"base": {"sha": base}, "head": {"sha": head}}}
            self.assertEqual(set(ci.changed_paths(root, event)), {"old.rs", "renamed.rs"})
            # The base advanced to the same tree independently. A direct
            # base/head diff would be empty and incorrectly skip the PR.
            advanced = git("commit-tree", head + "^{tree}", "-p", base, "-m", "base advanced")
            event["pull_request"]["base"]["sha"] = advanced
            self.assertEqual(set(ci.changed_paths(root, event)), {"old.rs", "renamed.rs"})


if __name__ == "__main__":
    unittest.main()
