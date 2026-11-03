# Installer archive fixtures

All inputs are committed. Loading fixtures and running checks uses no network,
archive executables, or builder tools. Rust dependencies are resolved by the
workspace lockfile in the usual way; the fixture loader never downloads data.

```sh
cargo xtask fixtures check        # source/version hash, output integrity, sizes
cargo xtask fixtures check-stale  # source/version hash and output integrity
cargo xtask fixtures check-size   # each binary <= 3,000,000; entire dir <= 80,000,000
cargo test -p xtask --locked      # loader, checks, deliberate failures, ZIP/tar goldens
```

`cargo xtask fixtures --check` is an alias for `check`. `--root <workspace>`
allows CI and the rejection tests to point checks at a synthetic input tree.
Missing fixture files or golden trees are hard errors. Neither checker rebuilds
fixtures or probes installed builder versions.

## Rebuild

On Linux, with the pinned versions in `src/builders.toml`, the GNU Windows Rust
target and MinGW linker installed:

```sh
cargo xtask fixtures
```

The builder compiles the standalone fixture game for Linux x86_64 and Windows
x86_64 GNU, builds ZIP/tar/gzip in Rust, and uses 7-Zip for 7z (including split
volumes), bzip2 and xz, plus the zstd CLI for tar.zst. `CASKADE_7Z` can point to
`7zz` instead of `7z`. Other hosts use the committed Linux/Windows game binaries
when rebuilding archive containers. A changed game should be rebuilt on Linux.
The Rust dependency versions are pinned in `xtask/Cargo.toml` and the root lock.
Maintainers on this shared machine wrap Cargo in
`flock /tmp/caskade-heavy.lock cargo ...`, including fixture rebuilds.

`src/corpus.toml` declares layouts, archives and immutable handmade fixture ids.
`src/layouts/` holds readable engine metadata; binaries, execute bits, empty
directories and symlinks are declared in the corpus. All framework links point
inside the `.app`. The macOS archive fixture uses a real POSIX shell entrypoint
to exercise copying, extraction and discovery, with synthetic framework data.
It does not claim to contain a Mach-O or universal Rust executable. Building the
Rust game as a macOS universal binary is a separate native-runner step (see
`../fixture-game/README.md`); it is not required for archive checks.

`handmade/` contains genuine RAR5 Unity, Unreal and multi-volume GOG archives.
Its README records provenance and creation commands. The generator never
rewrites those archives or their expectations. RAR4 is not included; the Windows namespace adds a genuine handmade RAR SFX. Their absence does not cause a silent skip for a declared fixture.

## Consumer helper

Integration tests can include the helper directly, without depending on xtask:

```rust,ignore
#[path = "fixtures/support/mod.rs"]
mod fixtures;

let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
let fixture = fixtures::Fixture::load(&root, "unity-zip")?;
let input = fixture.input();
let all_volumes = fixture.parts();
// Extract using the installer under test, then:
fixture.assert_tree(&extracted)?;
```

The helper needs serde with derive, serde_json, sha2 0.10 and toml 0.8 as test
dependencies. It performs no extraction itself. `load_all` enumerates the complete I0 archive corpus; `load_windows` enumerates
the separate I1 corpus under `expected/windows/`. `assert_tree` compares exact paths, kinds, file sizes,
SHA-256, Unix execute bits and symlink targets, without following symlinks.
Windows ignores Unix execute bits because its filesystem has no such mode.

`expected/*.toml` schema 1 records title, layout, archive format, payload platform,
input, the full ordered volume set (including the first), launch target, tree,
and per-host extraction/runtime expectations. Those runtime names describe how
to run a supported payload after extraction; foreign Linux/macOS binaries are
not made executable on another OS by extracting them. All golden paths are
relative and use `/`. Golden trees contain explicit parent directories.

`build.lock` records a length-delimited SHA-256 of fixture sources, builder
declarations, fixture-game manifest/lock/sources/tests, generator manifest/
sources/tests and the loader. It also records every output's size and SHA-256
under bin/, handmade/ and expected/. Source hashing normalizes CRLF to LF.
Checks compare declarations to the lock rather than consulting the local Rust,
7-Zip, MinGW or zstd installations, so they work on all CI hosts without builders.

The xtask tests prove nonzero command exits for changed layout, game, generator
and builder versions; missing/changed fixture and golden files; a sparse binary
of 3,000,001 bytes; and a sparse source file exceeding the 80 MB total limit.
The check-only test runs with an empty PATH.


## Windows installers (#34)

`bin/windows/<id>/` and `expected/windows/` contain 33 genuine fixtures: six
feature cases for each ISCC 5.6.1, 6.0.5 and 6.4.3; official NSIS 2.51 ANSI
solid/non-solid and 3.11 ANSI/Unicode solid/non-solid; wixl 0.103 embedded cab,
external cab and external cab with a real type-51 custom action; 7z and ZIP SFX;
Humble/Unreal/Steam-style portable packages; plus `handmade/rar-sfx.exe`.
NSIS 2.51's official distribution is ANSI-only; Unicode is covered by NSIS 3.11.
The named GOG/legacy minis are members of the Inno feature matrix. Sliced
installers contain compiler-generated 2 MB volumes and deterministic padding.
Each **file**, including each volume, remains below 3 MB.

```sh
# Explicit opt-in downloads, checked against src/windows/builders.json:
cargo xtask fixtures windows --prepare
# Cached builders only, with no implicit download:
cargo xtask fixtures windows
# A subset, e.g. on a Windows runner:
cargo xtask fixtures windows --prepare --families inno,nsis
# Finalize integrity metadata after direct checked-in builder invocations:
cargo xtask fixtures record-build
# Independent maintainer extraction/native checks (Linux):
python3 xtask/windows-verify.py
python3 xtask/windows-verify.py --wine-inno
```

Inno and NSIS run natively on Windows or in a private Wine prefix on Linux.
The Linux MSI builder downloads SHA-pinned Ubuntu packages into target/ and
extracts them with dpkg-deb; no sudo or system install is used. External cabinets
are made with gcab using the MSI File-table identifiers. `msi-plus-cab` contains
a type-51 property-setting custom action, verifiable with msiinfo. SFX compression
uses the existing SHA-verified official 7zz 26.05 (`scripts/7zz-download.py`) plus
the official 26.01 Windows 7z.sfx. The ZIP SFX is a real MinGW-built launcher
(`src/windows/zip-sfx.c`) with an appended ZIP and valid absolute directory
offsets; native execution uses Windows PowerShell/.NET ZIP extraction and does
not execute the payload. Its native smoke test is pending #22.

`windows` metadata records exact builder versions, feature flags, the public
test password (`caskade-fixture`), and native-generated file assertions. Golden
trees describe the static installed payload. Absolute `{app}` values produced
by [INI]/[Code] are checked semantically by windows-verify.py, including actual
fixture-game `--self-check` for the GOG/legacy registry+INI cases. They cannot
have one byte-identical hash across install locations. Runtime-generated
registry values and Start-menu effects belong to native integration tests.

For moving-test acceptance (#62), only `gog-inno-unity` and
`legacy-registry-path` enable Inno's genuine own uninstaller. Their generated
`unins000.exe`/`.dat` are native effects, not static extracted payload files.
Only `msi-plus-cab` is x64/per-machine and records `[INSTALLDIR]` in both
`config.ini` and HKLM64 `Software\CaskadeFixture\FixtureGame` `InstallPath`.
Its fixed ProductCode is `{E8AEE435-3746-51E3-B090-78112E5A78A7}`; an authored
64-bit RegistrySearch remembers its install directory for uninstall after a
move/repair. Other MSI and Inno fixtures retain their original authoring.
Rebuild these three without rebuilding the payload or other installers:

```sh
python3 xtask/windows-fixtures.py --families inno,msi \
  --only gog-inno-unity,legacy-registry-path,msi-plus-cab
```

Fixture rebuilds run locally only; no workflow rebuilds them. Inno/NSIS build on
Windows (`just fixtures windows --prepare --families inno,nsis`), MSI/SFX/archives,
Burn and the Linux minis on Linux, and the native macOS fixtures on a Mac
(`python3 xtask/macos-fixtures.py --artifact <dir>`); then `just fixtures check`
and commit the outputs with `build.lock` (see
[CI setup](../../docs/ci.md#local-only-runs)). The genuine builds are not
byte-for-byte reproducible across hosts. Builder and independent extraction
evidence do not substitute for the native Windows/macOS pipeline tests in CI's
`installer` job.


## Linux shell fixtures for #37

`cargo xtask fixtures linux-shell --prepare` builds the genuine makeself 2.5.0
`bin/linux-shell/makeself-generic.sh` and a GOG-style structural fixture
`gog-linux-mojosetup.sh`. The latter has a genuine makeself shell header labelled
GOG.com/MojoSetup, a small explicitly synthetic bootstrap marker, and an appended
real ZIP containing `data/noarch/{start.sh,FixtureGame.x86_64,gameinfo}`. It does
not claim to ship the proprietary GOG MojoSetup engine. The golden is the game
payload after stripping `data/noarch/`. Header offsets and archive bytes are
created by makeself/zipfile, not fabricated signatures.

`fixtures::load_linux_shell` reads the separate `expected/linux-shell` corpus.
The xtask test statically extracts the actual generated containers, compares
complete hashes/modes to goldens, and on Linux runs the extracted `start.sh
--token <path>` to prove both payloads start. No installer shell is executed.
The builder is an SHA-pinned Ubuntu package unpacked under target/, without sudo.

`cargo xtask fixtures appimage --prepare` builds `bin/appimage/Indie_Game-x86_64.AppImage`
(`appimage-indie`) with the SHA-pinned official appimagetool 1.9.1 and type2
runtime 20251108 from `src/appimage/builder.json`. appimagetool is unpacked with
its own `--appimage-extract`, so no FUSE or sudo is needed. The AppDir holds the
fixture game as `usr/bin/indie-game`, an `AppRun` link, a desktop entry, AppStream
metadata and a generated 16×16 PNG icon linked from `.DirIcon`. The squashfs is
appimagetool's default zstd, and `SOURCE_DATE_EPOCH` makes rebuilds byte-identical.
The builder proves the payload starts through the image's own runtime
(`APPIMAGE_EXTRACT_AND_RUN=1 … --token`). The golden is the installed tree: the
image itself, executable. `fixtures::load_appimage` reads `expected/appimage`.

The primary raised the total committed corpus budget to **80 MB** on 2026-10-26
for the native installer and Burn acceptance families. The **3 MB per binary**
limit remains unchanged. If the 80 MB total is outgrown, the plan's long-term
path is moving `bin/` to Git LFS; the read-only checks continue to enforce the
active limits in `xtask::checks`.

The seven Linux I2 minis have a consolidated `load_linux_mini` loader and
`expected/linux-mini` catalogue; see `src/linux-mini/README.md` for the real
launch/delegate tests and the pending Steam Deck checklist.
