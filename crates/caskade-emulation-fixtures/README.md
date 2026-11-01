# Generated emulation fixtures (#114)

This unpublished development crate supports detection (E2), ROM scanning (E3)
and launches/tracking (E4). All trees, archives and executables are generated
in temporary directories or Cargo's target directory. Nothing reads a real
Steam/Flatpak/emulator installation. No ROM, BIOS, core or emulator payload is
committed. Commercial game names are filename-format examples only: cartridge,
disc and BIOS files are empty. Playlists and archives have valid container
syntax; they do not contain playable copyrighted data.

`MachineFixture::new(FixtureOs::{Linux,Windows,Macos})` owns a temporary
`machine.toml`, `root/` and `registry/` scenario for
`caskade_discovery::HostEnv::fixture(fixture.path())`. The usual fixture path
mapping applies (`root/drives/c` for Windows). Markers cover PATH, Program
Files/AppData, Scoop/Chocolatey, portable folders, app bundles with original
Info.plist metadata, AppImages in Applications and .local/bin, and
EmuDeck/RetroDECK paths. The AppImages contain genuinely generated gzip
SquashFS metadata behind an identification header; their dummy ELF prefix is
for **static inspection**, not an executable emulator runtime.

Linux includes a file-based RetroArch Flatpak deployment, sandbox-specific
core markers, a runtime and an incomplete deployment for negative tests.
`flatpak_list()` returns a synthetic, fixed TSV recording with the columns
`name, application, version, branch, installation`. Its shape follows the
public `flatpak list --app --columns=...` interface. It was authored from the
format; it is not an export of a user's installation. The shared probe tests
verify the deployment's app ID against that recording.

`RomFixture::thousand()?.tree` owns exactly **1,000 physical files**: 963
expected games, 983 top-level ROM paths, nine claimed playlist/track files,
and eight hidden/excluded files. Twenty CHD-name pairs represent two-disc
sets. The independent expected table assumes every visible extension is
accepted, archives are accepted directly, hidden files are skipped and
`SCANNER_EXCLUSIONS` is applied. M3U outranks its CUEs; CUE and GDI files claim
their track files. ZIP and 7z containers are genuinely generated in Rust and
can be used separately to test archive inspection. `RomTree::generate(path,
count)` also permits bounded larger workloads. `validate()` rejects missing,
additional, duplicated or misclassified files; it never rewrites expectations.

Build the native recorder once:

```sh
cargo build -p caskade-emulation-fixtures --bin fake-emulator --locked
```

Set `CASKADE_TEST_EMULATOR` to Cargo's resulting executable before testing
consumers. `FakeEmulator::new(dir)` uses that path; focused Unix tests without
it get a generated `/bin/sh` fallback. Windows consumers use the native
Cargo-built binary. No test invokes rustc or downloads a builder. The app CI
helper obtains the real Cargo artifact path, including custom target dirs,
and exports it to later steps.

`fake.command(record_prefix)` sets `CASKADE_FAKE_RECORD`. Arguments are
untouched, with empty/quoted/Unicode tokens recorded separately. Files ending
in `.argv`, `.cwd`, `.env` contain NUL-terminated UTF-8 fields; `.pid` is
published last. `Invocation::read(prefix)` reads the record. Environment
recording is allowlisted to `CASKADE_FAKE_MARKER`, `CASKADE_FAKE_PHASE`,
`CASKADE_FAKE_CORE`; the native binary accepts additional explicit keys in
`CASKADE_FAKE_ENV_KEYS`. Arbitrary host environment is never dumped.

Controls (not emulator arguments): `CASKADE_FAKE_SLEEP_MS` (0–30,000),
`CASKADE_FAKE_EXIT_CODE` (0–255), `CASKADE_FAKE_FORK=wait|detach`, and
`CASKADE_FAKE_CHILD_SLEEP_MS`. The child receives the same argv and records to
`<prefix>.child`, with a distinct PID and phase. `wait` models a launcher that
waits; `detach` models an updater/launcher exiting before its game child.
`FakeEmulator::copy_to(path)` copies the recorder without compiling it.
`CASKADE_FAKE_CHILD_PROGRAM` selects that differently named copy for the child,
modelling Flatpak launching `retroarch` for process-name tracking. The shell
fallback runs the copy directly, so its process name is the copy's file name. The override is removed in the child to prevent recursive launching.

Checks, requiring no real emulators or builder downloads:

```sh
cargo xtask emulation check
cargo test -p caskade-emulation-fixtures --locked
python3 scripts/app-ci.py emulation
```

`emulation check` generates and discards all fixtures, validates the expected
partition and enforces 3 MiB per generated file / 16 MiB total. Its JSON
contains aggregate counts only. Tests prove mutation rejection, deterministic
generation, static OS-probe consumption, exact native/script recorder output
and fork lifetimes. The app workflow runs fixture and definition/emulation
tests on Linux and cross-compiles consumers for Windows; native Windows/macOS
checks belong to #22/#71 until the user registers runners.
