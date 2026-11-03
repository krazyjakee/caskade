# Installer CI on GitHub Actions

The installer's checks are jobs in `.github/workflows/ci.yml`, which runs on
master pushes, pull requests and manual runs, on GitHub-hosted runners. The
repository has no self-hosted runners, and no job asks for one. Pull requests
use the plain `pull_request` event, so a fork's PR gets a read-only token, no
secrets and a fresh hosted machine.

## Jobs and results

| Job | Runs | Runner | Evidence |
| --- | --- | --- | --- |
| `installer` | master push, manual, PR when `scripts/ci-scope.py` selects the installer | `ubuntu-latest`, `windows-latest`, `macos-latest` | fmt, strict Clippy, T1–T6 and T9 fixture pass/fail, doc tests, the CLI-only feature contract, fixture staleness/size checks, xtask tests, JUnit; the native profile on Windows and macOS |
| `installer-linux-native` | master push, manual, PR labelled `run-native` (when the installer is selected) | `ubuntu-latest` | serial Linux native profile: Proton, Debian/Fedora delegates in the runner's Docker, Flatpak, second-filesystem moves |

Both feed `ci result`, the one required check, which passes when every selected
job passed or the PR's scope skipped it. A master push or a manual run selects
the whole suite.

Fuzzing (T8), the T10 release performance gates, the mini scoreboard and the
fixture rebuilds have no workflow: they run locally only, as described under
[Local-only runs](#local-only-runs).

Actions are pinned to full commit SHAs. Main-suite and standalone-CLI reports
are kept apart (`suite-junit.xml` and `junit.xml`). Nextest's `ci` profile does
not stop at the first failure. Native runs are serial; only the external
package/download group retries twice. Local HTTP/mock tests never retry. The
`native` profile requires `CASKADE_TEST_NATIVE=1` and `--run-ignored all`; the
jobs supply both. T10 is excluded from that profile. Rust is the current stable
release with cargo-nextest 0.9.114.

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
  rather than in every PR. Ubuntu's AppArmor blocks unprivileged user namespaces
  by default; the job turns that off for Steam Runtime's bwrap and Flatpak. The second filesystem for the Proton moves is the
  runner's `/mnt` disk, or a tmpfs mounted there.

### Fork boundary

A fork's pull request runs the `installer` job like any other, on hosted runners
with a read-only `GITHUB_TOKEN` and no secrets. `installer-linux-native` runs for
a PR only with the `run-native` label, which only maintainers can add. Only
master runs save the caches every run reads (GitHub scopes caches to the branch
that saved them); a PR's own cache entries stay in its scope. Checkouts never
persist credentials.

## Local validation

```sh
cargo install cargo-nextest --version 0.9.114 --locked
cargo nextest run -p caskade-installer --all-features --locked --profile ci
CASKADE_TEST_NATIVE=1 cargo nextest run -p caskade-installer --all-features \
  --locked --profile native --run-ignored all
cargo xtask fixtures check
actionlint .github/workflows/ci.yml
```

Use the shared heavy lock around Cargo invocations in the developer checkout.
Profile syntax and retry/report behavior follow
[nextest configuration](https://nexte.st/docs/configuration/) and
[test groups](https://nexte.st/docs/configuration/test-groups/).

The explicitly ignored `fetch_resolvers_live` recording test is excluded from the
`native` profile. Run `just test-resolvers-live` manually to refresh public metadata;
the CI suites replay committed resolver responses.

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


## Proton (#50)

[Runner setup](../../../xtask/runner/README.md) defines the cached umu 1.4.4,
Steam Runtime and UMU-Proton layout, kept as one Actions cache entry that the
`installer-linux-native` job warms on a miss and saves from master.
`prepare-proton.py` copies the cache into a writable job-local XDG tree and
records a JSON receipt. Proton runs as the runner's unprivileged user (Steam
Runtime refuses UID 0). Proton JUnit is retained separately from the remaining
native suite.

The #62 Proton moving test needs a second filesystem with 4 GiB free, supplied
through `CASKADE_TEST_MOVE_ROOT`: the runner's separate `/mnt` disk, or an 8 GiB
executable tmpfs mounted at `/mnt/caskade-move` when `/mnt` is on the
workspace's filesystem.

## Local-only runs

These have no workflow; run them on a developer machine (Linux) before changing
the code they cover. Each takes the shared heavy lock for its builds.

- **Fuzzing (T8):** `just fuzz <target> --seconds 600` per target in
  `crates/caskade-installer/fuzz/Cargo.toml` (nightly Rust and cargo-fuzz; see
  the [fuzz README](../fuzz/README.md)). Keep a target's corpus between runs with
  `xtask/runner/fuzz-corpus.py`, which bounds it to 1 MiB/input, 100,000 files
  and 256 MiB/target.
- **Performance (T10):** the release-mode bounds in
  [benches/README.md](../benches/README.md): 20 GiB sparse installer probe under
  16 MiB/200 ms, extraction no slower than 1.5 times pinned official 7zz, 10 GiB
  extraction below 256 MiB peak RSS in an isolated process, and discovery of
  100,000 files below one second, plus the Criterion distributions. The commands
  are in that README; they need 12 GiB free under `/tmp`.
- **Mini scoreboard (T9):** `just corpus` runs the installer CLI's lifecycle over
  the fixture corpus into `target/scoreboard`. Pass `--native-junit` with a
  native-profile JUnit report to include the delegated rows. Foreign host rows
  are plan-only/not-run; pending runner-only fixture families stay distinct.
- **Fixture rebuilds:** `just fixtures <family> --prepare` (see the
  [fixtures README](../tests/fixtures/README.md)), and on a Mac
  `python3 xtask/macos-fixtures.py --artifact <dir>`. Then `just fixtures check`
  and commit the outputs with `build.lock`. The genuine Inno, NSIS, deb,
  AppImage and Burn builds are not byte-for-byte reproducible across hosts, so a
  rebuild changes those binaries even when their sources did not.
