# Discovery CI on Gitea Actions

`.gitea/workflows/discovery.yml` uses repository-scoped self-hosted runners and
the same labels as installer CI. Under the user decision of 27 October 2026,
implementation issues close on Linux build/lint/test evidence plus cross-target
Clippy. Runner registration and Windows/macOS live results are recorded as deferred
verification on #71; a Linux cross compile is not a native OS test result.
No runner registration, server setting or GitHub mirror is changed by this work.

## Jobs and reports

| Job | Trigger | Labels | Checks |
| --- | --- | --- | --- |
| `check` | master push, trusted same-repository PR, manual | `caskade-linux`, `caskade-windows-x64`, `caskade-macos` | discovery-only fmt; all-feature build and strict Clippy; fixture/parser suite; doc tests; baseline and every isolated feature; fixture privacy/hashes/size; Linux cross-target Clippy |
| `live` | same-repository PR carrying `run-native`, or manual dispatch with `live=true` | `caskade-linux-native`, `caskade-windows-x64`, `caskade-macos` | only ignored live invariant tests, serially |

Actions use complete GitHub URLs pinned to commit SHAs. Upload-artifact v3 works
with the Gitea 1.22 artifact transport. Checkout does not retain Git credentials.
The matrix does not stop other OS jobs when one fails. Nextest reports are kept
for 14 days at `target/nextest/discovery-ci/junit.xml` and
`target/nextest/discovery-live/junit.xml`, with distinct artifact names per OS.

The repository profiles work with pinned nextest **0.9.114**. `discovery-ci`
inherits the common default test settings, disables fail-fast, and does not retry
fixture tests. `discovery-live` runs one test at a time. Only explicitly named
`live_network` tests use the separate network group with two retries. Snapshot
acceptance is disabled (`INSTA_UPDATE=no`). Live test authors must avoid printing
user paths, account identifiers or raw launcher records into JUnit output.

The opt-in live job sets `CASKADE_DISCOVERY_LIVE=1` and `CASKADE_TEST_NATIVE=1`.
It warns visibly if the scaffold has no ignored tests yet; that is not evidence
that a live launcher source was scanned. Later source issues supply these tests.
The runner environment determines what live sources are present: a Docker label
sees its job container, while a host label sees its dedicated service account.
Representative live-machine checks require deliberately provisioned launcher data.

## Heavy lane and feature gates

Every build, lint and test action goes through `scripts/discovery-ci.py`.
On Linux it invokes `flock --close /tmp/caskade-heavy.lock` around each Cargo
command; macOS uses Python’s `fcntl.flock` on the same path, since macOS has no
util-linux `flock` executable by default. Both release the lock between feature builds. Windows uses a one-byte `msvcrt` lock
at `%TEMP%\caskade-heavy.lock`, retained until the Cargo child exits. Keep runner
capacity at one and use one worker per shared host; the lock does not coordinate
separate machines or distinct Windows service accounts with different temp dirs.

Native jobs build **all features**, including bundled SQLite. The Linux cross
checks build Windows GNU with all features and Apple Darwin with all declared
features except `sqlite` and `default`. SQLite's bundled C build requires Apple's
SDK, so it is checked by the native macOS job. Source-family features trim the
build rather than selecting the host OS. Every declared feature is also built
alone with defaults disabled, plus a build with no features.

The fixture action is read-only. `cargo run -p xtask --locked -- discovery check`
is mandatory: it checks the committed fixture lock, privacy rules, hashes and
size budget. A missing lock fails the check. Ordinary CI never generates fixture
goldens, downloads builders or refreshes fixture locks.

## Runner provisioning (user action)

Reuse the repository-scoped runner setup in
[the installer CI guide](../../caskade-installer/docs/ci.md), including the
capacity-one rule and repository fork-approval policy. Required labels are:

| Label | Platform | Required tools |
| --- | --- | --- |
| `caskade-linux` | Linux x86_64, fresh Docker jobs | Rust 1.93+, nextest 0.9.114, Python 3.11+, Node 20, Git, util-linux `flock`, C compiler, Windows GNU cross compiler, both cross Rust targets |
| `caskade-linux-native` | Linux x86_64, dedicated jobs | the same tools; explicitly provisioned read-only launcher data for future live tests |
| `caskade-windows-x64` | Windows x64 host | Rust 1.93+ MSVC, nextest 0.9.114, Visual Studio C++ Build Tools/Windows SDK, Python 3.11+ (`python`), PowerShell 7, Node 20, Git |
| `caskade-macos` | macOS host | Rust 1.93+, nextest 0.9.114, Xcode command-line tools/Apple SDK, Python 3.11+ (`python3`), Node 20, Git |

Use the installer's shared `caskade/installer-ci:1.93.0` Linux image, whose recipe
includes `gcc-mingw-w64-x86-64` and the cross targets. The labels can serve both
workflows without changing their meanings. The discovery jobs require neither
privileged containers nor a Docker socket. They do not mount private host paths
or write the installer tool/fuzz caches.

Provision Rust tools on each native host and in the Linux image:

```sh
rustup component add rustfmt clippy
cargo install cargo-nextest --version 0.9.114 --locked
# Linux cross checks only:
rustup target add x86_64-pc-windows-gnu x86_64-apple-darwin
# Debian-family Linux image, for bundled SQLite's Windows cross build:
apt-get install -y gcc-mingw-w64-x86-64
```

Register from a private runner directory using a repository registration token
from **Settings → Actions → Runners** (not the tea/API token):

```sh
./act_runner generate-config > config.yaml
# Set runner.capacity: 1; configure the Linux local image as in the installer guide.
./act_runner --config config.yaml register
# Instance: <your Gitea instance URL>
# Token: supply the repository registration token interactively
# Name: one unique dedicated machine/account name
# Labels, according to this host:
# caskade-linux:docker://caskade/installer-ci:1.93.0
# caskade-linux-native:docker://caskade/installer-ci:1.93.0
# caskade-windows-x64:host
# caskade-macos:host
./act_runner daemon --config config.yaml
```

Keep `.runner` and tokens outside checkouts. A Windows/macOS service account must
see the same Rust, Python, Node and shell installations as its interactive setup.
Run the `discovery` workflow manually on master, inspect all three JUnit artifacts,
then opt into live tests separately after source fixtures and live tests exist.

## Fork boundary

The workflow definition comes from the base branch (`pull_request_target`). Both
PR jobs test repository identity and `fork == false` before PR-head checkout.
A `run-native` label cannot override that check. Manual dispatch is an explicit
maintainer action; ordinary pushes do not trigger the live job. No alternate
fork-defined workflow should be approved to use these dedicated labels. Server
scheduling and its approval history must be verified on Gitea during onboarding;
local YAML checks cannot prove the server's policy.

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
OS, native all-feature Windows/macOS runs, runner registration, and a live fork-PR
scheduling check remain on #71's deferred-native checklist until performed.

References: [nextest configuration](https://nexte.st/docs/configuration/),
[Gitea 1.22 act_runner setup](https://docs.gitea.com/1.22/usage/actions/act-runner/).

## Linux validation, 27 October 2026

Workflow `22ddd66` was validated against published master
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
