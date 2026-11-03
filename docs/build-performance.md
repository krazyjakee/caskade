# Build performance and CI scope

The desktop adapter builds one Rust library (`rlib`), consumed by the app,
bindings exporter, catalogue tool and integration tests. It does not produce the
mobile template's unused C static and shared libraries. Debug builds retain
line numbers, with no dependency debug info, and integration tests share one
binary per crate. Local Cargo incremental compilation stays enabled.

## Pull requests and master

`.github/workflows/ci.yml` runs every check on GitHub-hosted runners: the app's
frontend, Rust, Windows cross-Clippy and console e2e jobs, the installer and
discovery crates on Linux, Windows and macOS, and the translation catalogs. Its
`changes` job runs `scripts/ci-scope.py` in a full-history checkout of the pull
request. Pull requests run on the `pull_request` event with a read-only token and
no secrets, so pull requests from forks get the same checks. The selector uses a
merge-base diff with renames expanded into deleted/added paths, and derives transitive local dependencies from Cargo
manifests, including target, build and dev dependencies.

| Change | PR checks |
| --- | --- |
| Ordinary Markdown documentation | Scope/result checks only |
| Desktop UI or JS packages | Typecheck, Biome, Vitest, JS advisories and translation checks |
| Rust crate | App Rust/UI checks and Windows cross-Clippy; installer/discovery suites when their dependency graph is affected |
| Embedded theme contracts, plugin examples, WIT, core catalogs or legal text | App Rust/UI checks |
| Generated theming tables or Markdown included by Rust | Their frontend/Rust contract checks |
| Cargo manifests at the workspace root, locks, vendored code, CI/tooling, unknown paths | Full relevant suites |

A changed crate's README and doctests still select its Rust checks. Unknown or
unavailable diff information selects all checks; it never silently skips them.
The `ci result` job fails if a selected job failed or did not run, and succeeds
explicitly for an unaffected PR. Make it the required check.

Every master push and manual run runs the full native, cross-target and console
e2e checks, and the installer's Linux native profile. PRs can opt into console
e2e with the `run-e2e` label, and into the installer's native profile and
discovery's live-machine checks with `run-native`. CI builds no deb/AppImage
packages: `release.yml` builds and signs every platform's bundles from a tag, and
`just build-app` builds a debug bundle locally.

The app's Rust suites run in one Cargo invocation to unify dependency features
once. The emulation fixture step prepares the helper and tests definitions and
fixtures; the combined suite runs the core's emulation tests once, along with
all other core tests.

The workflow's `concurrency` block cancels a pull request's superseded run when
a new commit arrives; master runs are never cancelled.

Run the selector's regression tests with:

```sh
python3 -m unittest discover -s scripts -p test_ci_scope.py
```

## Measure the remaining work

`just build-timings` builds the desktop binaries under the heavy lock with
`--timings`. Cargo prints the report's actual path, honoring both project and
runner target-directory configuration. The app workflow also records its
combined Rust test build and uploads the HTML report with fixture results.

Separate queue wait, compilation, linking, test execution and packaging. Compare
cold builds, warm no-op builds and ordinary function edits independently. Do
not run `cargo clean` on the shared target to get a cold measurement; use a
separate `CARGO_TARGET_DIR` instead, and compare equivalent cache states.

GitHub already caches compiled dependencies and pnpm's store. Local/persistent
targets keep incremental artifacts. Shared sccache storage requires a separate
service/configuration and would need a CI-only incremental policy; it is not
enabled by this change. Compiler or linker replacements should be benchmarked
against the remaining critical path, as described in the
[Blaze/build-performance investigation](research/rust-build-performance.md).
