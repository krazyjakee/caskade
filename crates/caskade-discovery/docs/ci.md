# Discovery CI on GitHub Actions

`.github/workflows/discovery.yml` runs on GitHub-hosted runners (`ubuntu-24.04`,
`windows-latest`, `macos-latest`); the repository is public, so they cost
nothing and no self-hosted runner is involved. Pull requests run it when their
changes reach `caskade-discovery` (`scripts/ci-scope.py`,
[build performance](../../../docs/build-performance.md)); master pushes and
manual runs always do. `.github/workflows/discovery-nightly.yml` runs the fuzz
targets and the performance budget (see [the fuzz README](../fuzz/README.md)).

## Jobs and reports

| Job | Trigger | Runners | Checks |
| --- | --- | --- | --- |
| `check` | master push, pull request the scope selects, manual | Linux, Windows, macOS | discovery-only fmt; all-feature build and strict Clippy; fixture/parser suite; doc tests; baseline and every isolated feature; fixture privacy/hashes/size; Linux cross-target Clippy |
| `result` | every run | Linux | fails if a selected `check` leg failed or did not run; succeeds for an unaffected pull request |
| `live` | pull request carrying `run-native`, or manual run with `live` | Linux, Windows, macOS | only ignored live invariant tests, serially |

Actions are pinned to commit SHAs. Checkout does not retain Git credentials.
The matrix does not stop other OS jobs when one fails. Nextest reports are kept
for 14 days at `target/nextest/discovery-ci/junit.xml` and
`target/nextest/discovery-live/junit.xml`, with distinct artifact names per OS.
Each OS keeps a Rust cache saved from master only.

The repository profiles work with pinned nextest **0.9.114**. `discovery-ci`
inherits the common default test settings, disables fail-fast, and does not retry
fixture tests. `discovery-live` runs one test at a time. Only explicitly named
`live_network` tests use the separate network group with two retries. Snapshot
acceptance is disabled (`INSTA_UPDATE=no`). Live test authors must avoid printing
user paths, account identifiers or raw launcher records into JUnit output.

The opt-in live job sets `CASKADE_DISCOVERY_LIVE=1` and `CASKADE_TEST_NATIVE=1`.
It warns visibly if the scaffold has no ignored tests yet; that is not evidence
that a live launcher source was scanned. Later source issues supply these tests.
A hosted runner is a fresh image with no launchers or stores installed, so the
live job proves the invariants hold on a bare machine; representative
live-machine checks still need deliberately provisioned launcher data (#71).

## Heavy lane and feature gates

Every build, lint and test action goes through `scripts/discovery-ci.py`.
On Linux it invokes `flock --close /tmp/caskade-heavy.lock` around each Cargo
command; macOS uses Python’s `fcntl.flock` on the same path, since macOS has no
util-linux `flock` executable by default. Both release the lock between feature builds. Windows uses a one-byte `msvcrt` lock
at `%TEMP%\caskade-heavy.lock`, retained until the Cargo child exits. On a hosted
runner the lock is uncontended; it keeps local runs off a busy machine's heavy lane.

Native jobs build **all features**, including bundled SQLite. The Linux cross
checks build Windows GNU with all features (bundled SQLite with
`gcc-mingw-w64-x86-64`) and Apple Darwin with all declared features except
`sqlite` and `default`. SQLite's bundled C build requires Apple's SDK, so it is
checked by the native macOS job. Source-family features trim the build rather
than selecting the host OS. Every declared feature is also built alone with
defaults disabled, plus a build with no features.

The fixture action is read-only. `cargo run -p xtask --locked -- discovery check`
is mandatory: it checks the committed fixture lock, privacy rules, hashes and
size budget. A missing lock fails the check. Ordinary CI never generates fixture
goldens, downloads builders or refreshes fixture locks.

## Fork boundary

Pull requests run on the `pull_request` event: the jobs get a read-only token
and no secrets, on fresh hosted runners, so pull requests from forks run the
same checks. The `run-native` label only adds the live job; only maintainers
can label. No job runs on a self-hosted runner.

## Local checks

These helpers already acquire the appropriate heavy lock:

```sh
python3 scripts/discovery-ci.py fmt
python3 scripts/discovery-ci.py build
python3 scripts/discovery-ci.py clippy
python3 scripts/discovery-ci.py test
python3 scripts/discovery-ci.py doc
python3 scripts/discovery-ci.py features
python3 scripts/discovery-ci.py fixtures
python3 scripts/discovery-ci.py cross
CASKADE_DISCOVERY_LIVE=1 python3 scripts/discovery-ci.py live
# Review exact commands without building:
python3 scripts/discovery-ci.py cross --dry-run
```

The fixture suite uses the recorded machine trees on Linux for all OS profiles.
Live Windows registry parity, representative installed-launcher invariants on each
OS remain on #71's deferred-native checklist until performed. Native all-feature
Windows and macOS runs happen on every `check` run since the move to GitHub (#35).

Reference: [nextest configuration](https://nexte.st/docs/configuration/).

## Linux validation, 27 October 2026

This predates the move to GitHub Actions (#35); CI then ran on self-hosted
Gitea runners. Workflow `22ddd66` was validated against published master
`7c8b1e2f3a1a08ef1ff71ce6d2a9becb870011a7` (scaffold, core types,
HostEnv, shared parsers and orchestrator). Every local helper action above passed:
format check; all-feature Linux build and all-target strict Clippy;
**49 nextest tests passed, zero skipped**; doc tests; eight isolated builds
(no features plus each of seven declared features); mandatory fixture checks;
and Windows GNU / approved Apple cross-Clippy. The explicit live action reported
**zero live tests and 49 skipped**, with a visible warning, because live-source
tests have not been supplied yet.

Actionlint passed with exceptions only for Gitea's full GitHub action URLs and
repository-specific runner labels. YAML/nextest parsing and checks for pinned
SHAs, read-only permissions, fork guards, runner labels and opt-in live triggers
passed. Calling the live helper without `CASKADE_DISCOVERY_LIVE=1` was rejected.
These checks establish local workflow/configuration behavior; runner execution,
server-side fork scheduling and native OS results remain deferred on #71.

## macOS discovery profile and real-machine checks

The ordinary macOS matrix runs the same `macos_apps` and `macos_launchers`
fixture binaries that are proven on Linux, including SQLite-backed Galaxy/itch
records. The opt-in live matrix automatically picks up the ignored
`macos_native::real_mac_steam_epic_and_app_store_game_are_discovered` test on
macOS. It requires a deliberately provisioned Mac with native Steam, Epic and
Mac App Store games, and reports only aggregate counts. Setup, exact commands,
receipt limitations and the deferred real-Mac checklist are in
[macos-verification.md](macos-verification.md). No account provisioning or native
result is implied by a successful Linux fixture run.
