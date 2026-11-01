# Fixture game

This standalone Cargo workspace has no dependencies. Its release profile uses
`opt-level = "z"`, LTO, one codegen unit, `panic = "abort"` and stripping without
changing the installer's build profile.

```sh
cargo build --manifest-path crates/caskade-installer/tests/fixture-game/Cargo.toml \
  --release --locked --target x86_64-unknown-linux-gnu --target x86_64-pc-windows-gnu
```

`--token <path>` writes the absolute executable path plus a newline, even when
the working directory differs. `--self-check` additionally reads `config.ini`
beside the executable and requires exactly one `InstallPath=<absolute path>`.
The path must resolve to the executable directory, accepting stable aliases.
On Windows/Wine it also validates `InstallPath` (REG_SZ or REG_EXPAND_SZ) under
`HKLM\Software\CaskadeFixture\FixtureGame` in the 64-bit registry view.
`--name <name>` selects a different final registry key component. The program
never writes to the registry. Success writes `ok\n`; a mismatch writes its
reason into the token and returns a nonzero status.

`--check-z-drive` can be combined with either mode on Windows/Wine. It fails
if `Z:\etc\hostname` is readable, for the future Proton isolation tests.

On a native macOS runner, install both Apple Rust targets and build the same
source with these settings, then combine it with the system lipo tool:

```sh
cargo build --manifest-path crates/caskade-installer/tests/fixture-game/Cargo.toml \
  --release --locked --target x86_64-apple-darwin --target aarch64-apple-darwin
lipo -create \
  crates/caskade-installer/tests/fixture-game/target/x86_64-apple-darwin/release/caskade-fixture-game \
  crates/caskade-installer/tests/fixture-game/target/aarch64-apple-darwin/release/caskade-fixture-game \
  -output FixtureGame
```

The initial `.app` archive fixture has a documented shell entrypoint for archive
tests; the native Rust universal build belongs to the macOS fixture workflow.
