# Installer CI on GitHub Actions

Every installer workflow runs on GitHub-hosted runners. The repository has no
self-hosted runners, and no workflow asks for one. Pull requests use the plain
`pull_request` event, so a fork's PR gets a read-only token, no secrets and a
fresh hosted machine.

## Workflows and results

| Workflow | Trigger | Runner | Evidence |
| --- | --- | --- | --- |
| `installer-pr` | PR (scoped by `scripts/ci-scope.py`), master push, manual | `ubuntu-latest`, `windows-latest`, `macos-latest` | fmt, strict Clippy, T1–T6 and T9 fixture pass/fail, doc tests, the CLI-only feature contract, fixture staleness/size checks, xtask tests, JUnit; the native profile on Windows and macOS |
| `installer-linux-native` | master push, nightly, manual, PR labelled `run-native` | `ubuntu-latest` | serial Linux native profile: Proton, Debian/Fedora delegates in the runner's Docker, Flatpak, second-filesystem moves |
| `installer-nightly` | nightly or manual, master only | `ubuntu-latest` | each parser fuzzer for 600 s with retained bounded corpora; the T10 release gates and Criterion; native Proton/delegates and the mini scoreboard |
| `installer-cache-refresh` | manual, master only | `ubuntu-latest` | replaces the Proton cache entry (`xtask/runner/README.md`) |
| `installer-fixtures` | master push touching a fixture builder, manual | all three | genuine fixture rebuilds, then a review PR from `automation/installer-fixtures`; never invoked by ordinary tests |

`installer-pr` ends in an `installer result` job that passes when the selected
checks passed, or when the PR's scope skipped them. A master push or a manual
run always selects the whole suite.

Actions are pinned to full commit SHAs. Main-suite and standalone-CLI reports
are kept apart (`suite-junit.xml` and `junit.xml`). Nextest's `ci` profile does
not stop at the first failure. Native runs are serial; only the external
package/download group retries twice. Local HTTP/mock tests never retry. The
`native` profile requires `CASKADE_TEST_NATIVE=1` and `--run-ignored all`;
workflows supply both. T10 is excluded from that profile and has its own nightly
gate. Rust is the current stable release with cargo-nextest 0.9.114.

### Hosts

- **Windows** (`windows-latest`): the runner account is an administrator, so the
  HKLM, Uninstall-registration and msiexec tests run as written. Hosted Windows
  runners have a second volume, D:, while TEMP stays on C:; the workflow checks
  both and points `CASKADE_TEST_MOVE_ROOT` at `D:\CaskadeTests` for the C:→D:
  relocation tests. Checkout turns off line-ending conversion, because fixtures
  and snapshots are compared byte for byte.
- **macOS** (`macos-latest`, Apple Silicon): `xtask/macos-fixtures.py` builds the
  genuine app, DMG and pkg fixtures for the runner's architecture into
  `target/macos-fixture-artifact` (`CASKADE_MACOS_FIXTURES`) before the native
  profile. Current hdiutil no longer embeds SLA resources (`udifrez`), so the
  builder writes them into the image's UDIF XML itself. Native macOS uses
  `--all-features`, including fetch.
- **Linux** (`ubuntu-latest`): the native profile needs Docker, Flatpak, Xvfb,
  unprivileged user namespaces and Proton, so it runs in `installer-linux-native`
  rather than in every PR. The second filesystem for the Proton moves is the
  runner's `/mnt` disk, or a tmpfs mounted there.

### Fork boundary

A fork's pull request runs `installer-pr` like any other, on hosted runners with
a read-only `GITHUB_TOKEN` and no secrets. `installer-linux-native` runs for a PR
only with the `run-native` label, which only maintainers can add. Nightly, cache
refresh and fixtures have no PR trigger, and their jobs check for master. Only
master runs save the caches that every run reads (GitHub scopes caches to the
branch that saved them); a PR's own cache entries stay in its scope. Checkouts
never persist credentials, except the fixtures `propose` job, which pushes its
review branch.

The fixtures PR needs a token that may open PRs: the repository setting
"Allow GitHub Actions to create and approve pull requests", or a
`FIXTURE_PR_TOKEN` secret with contents and pull-requests write access. Without
either, `xtask/fixture-pr.py` pushes the branch and links the comparison in the
job summary. A run started by hand with `propose` off only rebuilds and checks.

## Local validation

```sh
cargo install cargo-nextest --version 0.9.114 --locked
cargo nextest run -p caskade-installer --all-features --locked --profile ci
CASKADE_TEST_NATIVE=1 cargo nextest run -p caskade-installer --all-features \
  --locked --profile native --run-ignored all
cargo xtask fixtures check
actionlint .github/workflows/installer-*.yml
```

Use the shared heavy lock around Cargo invocations in the developer checkout.
Profile syntax and retry/report behavior follow
[nextest configuration](https://nexte.st/docs/configuration/) and
[test groups](https://nexte.st/docs/configuration/test-groups/).

The explicitly ignored `fetch_resolvers_live` recording test is excluded from the
`native` profile. Run `just test-resolvers-live` manually to refresh public metadata;
ordinary PR, native and nightly suites replay committed resolver responses.

## Native fixture evidence

The Windows native profile runs `tests/native_windows.rs`: silent Inno 5.6.1 and
6.0.5, NSIS 2.51 and 3.11, and embedded/external-cab MSI install into paths with
spaces and non-ASCII characters. Only native execution candidates are retained;
a failed installer cannot pass through an extraction fallback. The tests compare
the same payload goldens as Linux/macOS extraction, require owned Uninstall
registrations, check shortcut opt-outs, and invoke the native uninstaller before
manifest cleanup. Only explicitly listed generated uninstaller files are excluded
from the payload golden.

The interactive test reads each real process command line through `sysinfo`,
compares the OS arguments to the family serialization contract, cancels the job,
and checks fixture-owned registry entries, shortcuts and the root are clean. MSI
may finish before cancellation; the report records that race. The Windows job
uploads `target/native-windows/interactive-argv.json` alongside native JUnit.
This checks argument delivery; manually checking wizard behavior and recording
`checked_versions` in `data/silent-switches.toml` remains a Windows-runner task.

Every PR's ordinary suite also runs the T9 test in `native_windows`: all six
committed installers are extracted through the built CLI, verified against the
same golden, uninstalled, and retried with a malformed input that must fail
without leaving an installation. It does not execute Windows installers.

For local Wine pre-evidence only (Wine, wine32, Xvfb and Python 3.11+ required):

```sh
xvfb-run -a python3 \
  crates/caskade-installer/tests/native_windows_support/wine.py
```

Each Wine command takes the shared heavy lock separately. The script uses a
fresh private prefix and isolated XDG directories, compares all six
payloads, checks shortcut/registry behavior, and runs each own uninstaller.
Generated NSIS uninstaller leftovers are removed only after payload removal is
verified, matching manifest cleanup. The JSON report is written to
`target/native-windows/wine-pre-evidence.json`. Successful scratch directories
are removed; failures retain the printed directory and logs. Wine pre-evidence
does not substitute for the native Windows runner or OS command-line readback.

The Linux native profile includes isolated Debian apt and Fedora DNF delegate
install/launch/uninstall tests and the Flatpak sandbox launch/uninstall test.
The job installs `flatpak` and `dbus` and uses the runner's Docker daemon.
Native JUnit and the
[Steam Deck checklist](../tests/fixtures/src/linux-mini/STEAM-DECK.md) are retained
as workflow artifacts. The checklist requires a separate manual Deck run.


## Proton, performance and scoreboard (#50)

[Runner setup](../../../xtask/runner/README.md) defines the cached umu 1.4.4,
Steam Runtime and UMU-Proton layout, kept as one Actions cache entry that
`installer-cache-refresh` replaces. `prepare-proton.py` copies the cache into a
writable job-local XDG tree and records a JSON receipt. Proton runs as the
runner's unprivileged user (Steam Runtime refuses UID 0). Proton JUnit is
retained separately from the remaining native suite.

`installer-linux-native` runs on master pushes and nightly. `installer-nightly`
also runs the native Proton suite and produces a mini-installer Markdown/JSON
scoreboard. Foreign host rows are plan-only/not-run; actual per-mini CLI
lifecycle times, native delegated lifecycle times and pending runner-only
fixture families remain distinguishable.

The nightly performance job enforces the release-mode bounds in
[benches/README.md](../benches/README.md): 20 GiB sparse installer probe under
16 MiB/200 ms, extraction no slower than 1.5 times pinned official 7zz, 10 GiB
extraction below 256 MiB peak RSS in an isolated process, and discovery of
100,000 files below one second. JSON and Criterion reports are retained.
The native nextest profile excludes these nightly-only heavy performance gates.

The fuzz job discovers every target from the committed fuzz manifest, runs each
for 600 seconds, and keeps a bounded corpus per target in the Actions cache for
the next night. Limits are 1 MiB/input, 100,000 files and 256 MiB/target. Pull
requests never run it.

The #62 Proton moving test needs a second filesystem with 4 GiB free, supplied
through `CASKADE_TEST_MOVE_ROOT`: the runner's separate `/mnt` disk, or an 8 GiB
executable tmpfs mounted at `/mnt/caskade-move` when `/mnt` is on the
workspace's filesystem.
