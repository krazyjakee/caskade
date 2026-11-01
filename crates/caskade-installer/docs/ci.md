# Installer CI on Gitea 1.22

The workflows target this Gitea repository, not GitHub-hosted runners. They remain
queued until the user registers runners. Issue #22 stays open on **runners
registered (user action)** and retains the consolidated deferred-native checklist.
Under the user decision of 27 October 2026, implementation issues close on Linux
build/test/native-profile evidence plus cross-target Clippy; Windows/macOS-only
checks remain explicitly deferred rather than blocking those closures. Native
evidence must still come from the resulting native runs. No runner registration,
server changes or GitHub mirror has been performed.

## Workflows and results

| Workflow | Trigger | Runner | Evidence |
| --- | --- | --- | --- |
| `installer-pr` | trusted same-repository PR, master push, manual | Linux, Windows x64, macOS | T1–T6 and T9 fixture pass/fail, strict Clippy, fmt, CLI-only feature contract, fixture staleness/size checks, JUnit; Windows/macOS native profile |
| `installer-linux-native` | master push, nightly, manual, same-repository PR with `run-native` | fresh privileged Linux job container | serial native tests, SHA-verified sidecar and private nested Docker for apt delegate tests |
| `installer-nightly` | nightly or manual | fresh Linux job containers | each parser fuzzer for 600 seconds with retained bounded corpora; four release T10 gates including 10-GiB RSS; Criterion; native Proton/delegates and mini scoreboard |
| `installer-cache-refresh` | manual on master only | fresh Linux job container | the only executable tool-cache writer; verified sidecar packages and project-owned fuzz seeds |
| `installer-fixtures` | existing maintainer-only builder workflow | owned by the fixture maintainer | explicit `--prepare` generation and review PR; never invoked by ordinary tests |

The four new workflows pin every action URL to a full commit SHA. Artifact uploads use the v3
transport supported by Gitea 1.22; v4 requires a different artifact backend.
Main-suite and standalone-CLI reports are preserved separately. Nextest's ci
profile does not stop at the first failure. Native runs are serial; only the
external package/download group retries twice. Local HTTP/mock tests never retry.
The `native` profile requires `CASKADE_TEST_NATIVE=1` in the invocation environment
and `--run-ignored all`; workflows supply both. T10 is excluded from that profile
and has its own nightly gate. Proton install/launch/registry/own-uninstaller tests run in this profile;
the workflow does not pretend an absent native test has run.

## Required user provisioning

Use dedicated disposable runner machines/accounts for this repository. Provide:

| Label | Platform and mode | Requirements |
| --- | --- | --- |
| `caskade-linux` | Linux x86_64, Docker | image below, Docker daemon |
| `caskade-linux-native` | Linux x86_64, Docker | same image, privileged job containers for private nested Docker; at least 12 GiB free for T10 plus build space |
| `caskade-windows-x64` | Windows x64, host | Rust 1.93+ MSVC, Visual Studio C++ Build Tools/Windows SDK, Node 20, Python 3, PowerShell 7, Git, nextest 0.9.114 |
| `caskade-macos` | macOS, host | Xcode command-line tools/Apple SDK, Rust 1.93+, Node 20, Python 3, Git, nextest 0.9.114 |

Use an Intel Mac for the committed x86_64 Mach-O execution fixture. An Apple
Silicon runner needs Rosetta and an explicitly verified fixture runtime contract.
Native macOS uses `--all-features`, including fetch; the Linux-to-macOS cross check
that excludes fetch is not the native CI gate.

Windows must have a second writable **volume**, normally D:, for later C:→D:
relocation tests. Create `D:\CaskadeTests` on that volume and reserve it for tests;
creating a second directory on C: does not test cross-volume moves. The runner
account needs normal file/link permissions required by the tests. Enable Windows
Developer Mode or provide the OS's documented symlink privilege for symlink tests.
Do not run a desktop installer test as a personal daily-use account. Desktop UI
clicking is outside these workflows; #35 checks native interactive argv.

### Linux job image

Build this image on the Linux Docker host before registering its labels. The
image is local to that Docker daemon; configure act_runner not to force-pull it.
The repository checkout is a fresh job workspace, not the shared developer tree.

```sh
docker build -t caskade/installer-ci:1.93.0 - <<'DOCKERFILE'
FROM node:20-bookworm-slim AS node
FROM rust:1.93.0-bookworm
COPY --from=node /usr/local/bin/node /usr/local/bin/node
RUN apt-get update && apt-get install -y --no-install-recommends \
    python3 python3-yaml git ca-certificates docker.io time util-linux \
    flatpak dbus libc-bin xvfb xauth libvulkan1 libgl1 libegl1 libgl1-mesa-dri \
    pkg-config libssl-dev clang make passwd gcc-mingw-w64-x86-64 && rm -rf /var/lib/apt/lists/*
RUN useradd --create-home --uid 1000 --groups docker caskade
RUN cargo install cargo-nextest --version 0.9.114 --locked \
 && rustup component add rustfmt clippy \
 && rustup target add x86_64-pc-windows-gnu x86_64-apple-darwin \
 && rustup toolchain install nightly --profile minimal \
 && cargo +nightly install cargo-fuzz --locked
DOCKERFILE
sudo install -d -m 0755 /srv/caskade-cache /srv/caskade-fuzz-corpora
```

Record the built image's immutable ID with the runner's environment receipt.
Rebuild deliberately when changing compiler/fuzz tooling. Do not install project
sources into the image. `dockerd` in the native job uses its private `/tmp` data
root and socket: the host Docker socket is not mounted into jobs. Cached fuzz
seeds are copied into a writable job-local corpus. Every ordinary/native/nightly
tool-cache mount is `:ro`; only the manual refresh workflow writes tools.
Nightly fuzzing has a separate bounded `/srv/caskade-fuzz-corpora` writable
volume containing input bytes only, never executable tools.

### Register act_runner (user action)

1. Download the appropriate act_runner binary from the official release page
   linked in the [Gitea 1.22 runner documentation](https://docs.gitea.com/1.22/usage/actions/act-runner/).
   Verify its published checksum and record `act_runner --version`.
2. In the repository's **Settings → Actions → Runners**, obtain a repository-scoped
   registration token. Do not use the tea/API token or an instance-wide token.
3. In a new private runner directory, generate and edit its configuration:

   ```sh
   ./act_runner generate-config > config.yaml
   ```

   Set `runner.capacity: 1`, a sufficient job timeout (at least 3 hours), and
   `cache.enabled: false`. Linux needs `container.force_pull: false` for the local
   image and `container.valid_volumes: ['/srv/caskade-cache', '/srv/caskade-fuzz-corpora']` (the exact host path,
   not an arbitrary-volume glob) and `container.docker_host: '-'` so act_runner
   does not automatically mount its Docker socket. Permit privileged containers only on
   the dedicated native Linux runner; configure that runner for its native label.
   Never allow arbitrary host paths or mount `/var/run/docker.sock` into jobs.
   Prefer separate Linux machines for the ordinary and privileged native labels.
   If one machine hosts both, register both labels on a single capacity-1 runner
   rather than running competing worker processes. Use this runner only for
   trusted Caskade jobs.
4. Register each machine with its exact labels. Use a fresh repository token for
   each registration, supplied interactively so it is not stored in shell history:

   ```sh
   ./act_runner --config config.yaml register
   # Instance: <your Gitea instance URL>
   # Token: repository registration token from the UI
   # Name: a unique machine name
   # Linux label: caskade-linux:docker://caskade/installer-ci:1.93.0
   # Native Linux label: caskade-linux-native:docker://caskade/installer-ci:1.93.0
   # Windows label: caskade-windows-x64:host
   # macOS label: caskade-macos:host
   ./act_runner daemon --config config.yaml
   ```

   On Windows use `.\act_runner.exe` in PowerShell. Install act_runner as an OS
   service/LaunchDaemon using the upstream instructions after the foreground test.
   Keep `.runner` and its token private and outside checkouts. Labels in the
   generated configuration must match registration; restart after changing them.
5. On each Windows/macOS host, install the same nextest version:

   ```sh
   cargo install cargo-nextest --version 0.9.114 --locked
   rustup component add rustfmt clippy
   ```

   Confirm `rustc -Vv`, `cargo nextest --version`, Node, Python and shell commands
   work under the **service account**, not just an interactive login.
6. Enable the repository workflows if disabled, then run `installer-pr` manually
   on master. Run the native and nightly performance workflows. Review the JUnit
   artifacts and native environment on all three OSes, then check off the deferred
   native items recorded on #22. Linux-evidenced implementation issues are already
   closed under the 27 October decision. Finally open a harmless same-repository PR and
   verify three OS checks are attached to that commit.
7. Verify a fork PR, including one labelled `run-native`, schedules **no native
   runner work** and checks out no fork code. This live guard verification is part
   of onboarding; local YAML tests cannot prove server scheduling behavior.

### Fork boundary

The PR and labelled native workflows use `pull_request_target`: Gitea loads their
workflow definition from the base branch. The job guard requires both the same
repository identity and `fork == false` **before** PR-head checkout. Labels never
override the fork condition. There are no local reusable workflow calls. Nightly
and cache refresh do not have a PR trigger at all. Credentials are not persisted
in checked-out Git configuration.

This secures these checked-in workflows; YAML is not a server-wide fork policy.
Gitea 1.22 can reuse approval for a previously approved fork contributor, so never
approve alternate fork-defined workflows that request these self-hosted labels.
The user must verify the repository's fork-approval policy before enabling these
runners. Review fork code into a same-repository branch when native testing is
needed. [Gitea's implementation](https://github.com/go-gitea/gitea/blob/v1.22.6/services/actions/notifier_helper.go)
shows both base-workflow selection and the approval rules. No server policy has
been changed by this work.

## Local validation

```sh
cargo install cargo-nextest --version 0.9.114 --locked
cargo nextest run -p caskade-installer --all-features --locked --profile ci
CASKADE_TEST_NATIVE=1 cargo nextest run -p caskade-installer --all-features \
  --locked --profile native --run-ignored all
cargo xtask fixtures check
# actionlint is GitHub-specific: ignore only Gitea full URLs/custom labels.
actionlint -ignore 'specifying action "https://github.com/' \
  -ignore 'label .* is unknown' .gitea/workflows/installer-pr.yml \
  .gitea/workflows/installer-linux-native.yml \
  .gitea/workflows/installer-nightly.yml \
  .gitea/workflows/installer-cache-refresh.yml
```

Use the shared heavy lock around Cargo invocations in the developer checkout.
Runners have fresh per-job workspaces and capacity limits instead. Profile syntax
and retry/report behavior follow [nextest configuration](https://nexte.st/docs/configuration/)
and [test groups](https://nexte.st/docs/configuration/test-groups/).

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
Its image needs `flatpak`, `dbus`, `ldd` (from `libc-bin`) and Python, in addition
to the private Docker daemon. Native JUnit and the
[Steam Deck checklist](../tests/fixtures/src/linux-mini/STEAM-DECK.md) are retained
as workflow artifacts. The checklist requires a separate manual Deck run.


## Proton, performance and scoreboard (#50)

[Runner setup](../../../xtask/runner/README.md) defines the cached umu 1.4.4,
Steam Runtime and UMU-Proton directory layout. Warm these under the dedicated
unprivileged runner account, then publish them to the read-only tool cache.
`prepare-proton.py` copies the cache into a writable job-local XDG tree and
records a JSON receipt. Missing cache fails with an actionable setup error.
Proton runs as the image's `caskade` user (Steam Runtime refuses UID 0); a fresh
job workspace and private Cargo/XDG state are writable, HOME is supplied by
`runuser`, and Docker/Flatpak delegates retain the private-daemon root step.
Proton JUnit is retained separately from the remaining native suite.

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

The existing fuzz job discovers every target from the committed fuzz manifest,
runs each for 600 seconds, and atomically merges a bounded corpus into the
separate persistent corpus volume. Limits are 1 MiB/input, 100,000 files and
256 MiB/target. Add the exact corpus host path to `container.valid_volumes`;
ordinary/fork jobs receive no writable tool cache or persistent corpus mount.

The #62 Proton moving test uses a private 8 GiB executable tmpfs mounted at
`/caskade-move` in each native/scoreboard job, supplied through
`CASKADE_TEST_MOVE_ROOT`. This avoids Docker's default 64 MiB `/dev/shm` and
provides a distinct filesystem without exposing a host directory.
