# Blaze and Rust build performance

Investigated 2026-11-01 against Caskade `dcdf2c3` and Blaze main
`9656181fd5c507729dd514edad0572d8a85d6089`. This is an investigation,
not a change to the build configuration.

The follow-up [build and CI changes](../build-performance.md) implement the
desktop artifact reduction, PR scoping, test batching, scheduled packaging and
timing reports described below. The remaining alternatives are experiments.

## Decision

Keep Cargo/rustc for development, CI and releases. Blaze's newest published
canary can run a small Caskade crate's tests, but full desktop compatibility
and an edit/rebuild speedup have not been established. First reduce unnecessary
artifacts and CI work, improve cache reuse, and measure the remaining critical
path. Those changes can benefit the existing compiler on every supported OS.

## What was actually tested

The host was Linux x86-64 with stable Rust 1.98.0. Release archives were
downloaded directly from GitHub and checked against their published SHA-256
files. Binaries, toolchain, Cargo outputs and Blaze caches were put under
`/tmp/caskade-blaze-investigation`; neither release was enabled globally.
Builds used `/tmp/caskade-heavy.lock`. Both daemons were stopped afterwards.

| Command | Tool | Result |
| --- | --- | --- |
| `test -p caskade-i18n --lib --locked` | Cargo 1.98 toolchain | All 30 tests passed |
| Same arguments | Blaze stable `0.20261027.0` | Exit 101: four reachable functions could not compile |
| Same arguments | Blaze canary `0.20261030.1` | All 30 tests passed |
| `build -p caskade-desktop --locked` | Blaze stable | Exit 101: refuses the desktop library's `staticlib` output |

Stable's diagnostics included writable standard-library statics
`__RUST_STD_INTERNAL_VAL` and `MIN` without a rustc symbol, reached through
thread-local initialization and thread spawning. Canary got past those failures.
Its test build included the i18n build script, bundled translation catalogs and
`tracing-attributes` proc macro; this is more evidence than a hello-world build.

Canary took about 35.5 seconds for its first test invocation, including downloading
its library and compiling build scripts and standard-library code. Its ready
worker reported about 2.27 GB held. The Cargo invocation used existing dependency
artifacts and finished compilation in about 1 second. **These are different cache
states, not a speed comparison.** No source-edit benchmark was performed. Canary
also reported that its disk store was disabled because the newly created cache
directory was writable by other users; the directory permissions were corrected
after that run. All 30 tests did execute.

The attempted canary desktop build did not obtain the machine's heavy lock within
120 seconds; it never reached the compiler. This is neither a compilation failure
nor evidence that the desktop app works. Lock waits must be measured separately
from compiler time when investigating this shared machine.

| Downloaded archive | SHA-256 |
| --- | --- |
| `blaze-v0.20261027.0-x86_64-unknown-linux-gnu.tar.gz` | `df61ce6d8216087a40e986d4f84cf2a230638bde014e38519e08b50282e90e5d` |
| `blaze-v0.20261030.1-x86_64-unknown-linux-gnu.tar.gz` | `4121feea06809eb5d4d6f4fb5bfe42fdd49281ed73adcfdfaa3a5d6c661c6ce4` |

For a repeat experiment, use the binaries from those exact releases side by side
with their helper programs, and give each release separate `BLAZE_HOME`,
`BLAZE_CACHE` and `CARGO_TARGET_DIR` directories. Make the cache private (mode
0700), cap v1 workers with `BLAZE_MEMORY_MAX=6G`, and set `BLAZE_TESTS=all`.
Run the arguments above under the heavy lock. A separate output directory avoids
replacing the normal Cargo artifacts; `blaze stop` with the same `BLAZE_HOME`
stops that experiment's daemon.

## Why the upstream version matters

- [Stable](https://github.com/indexable-inc/blaze/releases/tag/v0.20261027.0)
  and the latest [canary](https://github.com/indexable-inc/blaze/releases/tag/v0.20261030.1)
  still ship v1. Canary documents static archive support that stable lacks.
- Main had replaced v1 with v2. Its
  [support matrix](https://github.com/indexable-inc/blaze/blob/9656181fd5c507729dd514edad0572d8a85d6089/docs/support.md)
  and [benchmarks](https://github.com/indexable-inc/blaze/blob/9656181fd5c507729dd514edad0572d8a85d6089/docs/benchmarks.md)
  explicitly describe historical v1 measurements, not v2 results. Headline
  speedups cannot be applied to Caskade or assumed for the replacement compiler.
- Main's [link target implementation](https://github.com/indexable-inc/blaze/blob/9656181fd5c507729dd514edad0572d8a85d6089/crates/blaze-link/src/target.rs)
  explicitly rejects `wasm32-wasip2`: it writes core modules, not components.
  Our plugin host's integration tests build that target using `env!("CARGO")`
  (`crates/caskade-plugin-host/tests/main.rs` and `tests/installer.rs`). It also
  lacks our Windows GNU cross-check and Intel macOS release targets.
- V2's [command documentation](https://github.com/indexable-inc/blaze/blob/9656181fd5c507729dd514edad0572d8a85d6089/docs/cargo.md)
  says `clippy` is unsupported and doctests are not implemented. These commands
  remain necessary in our CI. Blaze uses its own pinned nightly, so a pass does
  not establish compatibility with our stable Rust toolchain or minimum version.

These main-branch findings were checked in source; v2 was not built locally.
They do not describe an observed failure of the tested canary binary.

## Alternatives, in priority order

### 1. Stop producing desktop library formats we do not use

`apps/desktop/src-tauri/Cargo.toml` requests `staticlib`, `cdylib` and `rlib`.
The existing debug output contains a **1,334 MiB static archive** and a
**105 MiB shared library**, alongside the Rust library and executable. These
sizes are an artifact inventory, not measured time savings.

For this desktop application, evaluate keeping only `rlib`. Validate the app,
bindings exporter, catalogue tool, integration tests, and native bundles on all
release targets before landing it. A mobile or external C ABI consumer would
need the other formats; none was identified in this repository's desktop flow.

### 2. Reduce the CI work per change

At the time, the app workflow (on Gitea; GitHub Actions since #35) ran
workspace Clippy, native tests, console e2e, Windows cross-Clippy and debug
AppImage/deb packaging on every master push and trusted PR update. App,
installer and discovery workflows had no path filters and all set
`cancel-in-progress: false`.

Use an affected-area gate that understands shared crates, lockfiles, manifests,
generated inputs and build scripts. Preserve a full master/scheduled gate. Keep
native packaging as a release or explicitly requested smoke check instead of a
default for every unrelated edit. Cancel superseded PR checks, while retaining
master and release runs. Make any required aggregate check succeed explicitly
when an area legitimately has no work.

`scripts/app-ci.py emulation` already runs the core's `emulation::` unit tests;
the later unfiltered core test step includes them again. Review that duplication.
Discovery also builds each feature in isolation: preserve that contract gate,
but avoid rerunning it for unrelated UI/documentation changes.

Parallel jobs help wall time when they have separate capacity. On one machine
with a shared heavy lane they can merely increase queueing; measure queue wait
and memory pressure before adding more concurrent Cargo jobs.

### 3. Use caches suited to each environment

GitHub release and i18n workflows already gained `Swatinem/rust-cache` and pnpm
caching in `dcdf2c3`; adding the same action again is not a new improvement.
The release cache's own comment correctly promises tag reruns: GitHub cannot
restore a cache from an unrelated previous tag. A compatible cache seeded on
master, or a compiler cache shared across jobs, is needed for dependable reuse
between different releases. Match toolchain, target, profile and flags.

For disposable CI machines, benchmark **sccache with a shared storage backend**
and `CARGO_INCREMENTAL=0`. It can reuse eligible library compilation across
clean checkouts. It cannot cache Rust linker invocations (executables, shared
libraries and proc macros), requires incremental compilation to be disabled,
and is not a replacement for caching build-script outputs. Keep Cargo's
incremental mode for local edits and persistent local targets until measured
evidence favors changing it. See [sccache's Rust limitations](https://github.com/mozilla/sccache/blob/main/docs/Rust.md),
[rust-cache's behavior](https://github.com/Swatinem/rust-cache#cache-details)
and [GitHub cache scope](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#restrictions-for-accessing-a-cache).

### 4. Benchmark mold against the linker we already use

A tiny `rustc --print link-args` probe confirmed that this host's Rust 1.98
already passes `-fuse-ld=lld`. Enabling lld therefore offers no gain here.
[mold](https://github.com/rui314/mold) is the next Linux candidate if final linking
dominates. Benchmark an actual Caskade app/test relink before choosing it;
changing linker flags invalidates Cargo artifacts and causes a one-time rebuild.
Keep configuration target-specific so it does not affect Windows or WASI guests.
[Wild](https://github.com/wild-linker/wild) is another candidate, but its promised
incremental linking is not implemented yet.

### 5. Reduce recompilation through crate boundaries and feature consistency

`caskade-core/src` contains roughly 94,000 Rust source lines. After collecting
timings, move cohesive services with frequent independent edits behind smaller
crates, keeping shared domain types below them to avoid dependency cycles.
This can reduce the work invalidated by an edit. Audit heavy dependencies and
their features, particularly Wasmtime and the installer/torrent graph; features
that implement required behavior cannot simply be removed.

Batch compatible package test invocations to reduce dependency feature churn.
Keep explicit feature-isolation checks separate: broad feature unification can
hide missing feature declarations. Installer and discovery already use nextest;
extending it to the app may help test execution, but does not remove compilation
or linking. See [Cargo's timing and crate-structure guidance](https://doc.rust-lang.org/cargo/reference/timings.html).

### 6. Try nightly compiler options as separate experiments

[rustc's Cranelift backend](https://github.com/rust-lang/rustc_codegen_cranelift)
keeps the normal Rust frontend while changing code generation. It is a more
conservative compiler experiment than replacing the frontend with Blaze, but
still requires nightly and has SIMD and unwinding limitations. Guest Wasm builds
need a supported backend, and slower generated code can make tests take longer.

The nightly parallel frontend (`-Zthreads=4` or `8`) is another experiment for a
large crate's type-check/codegen tail. Cap Cargo parallelism and monitor memory
on this shared host. Neither experiment should replace stable release validation.
See [Cargo's current build-performance guide](https://doc.rust-lang.org/cargo/guide/build-performance.html).

## Measurements needed before claiming a speedup

Record cold builds, warm no-op builds, one-function edits in core and adapter,
test compilation, test execution, final linking, and CI queue time separately.
Use `cargo build --timings` and `cargo test --no-run --timings` under the heavy
lock, with explicit package/target/feature selections. Keep baseline artifacts
instead of running `cargo clean` on the shared target. Repeat candidate settings
in separate target directories with comparable cache states.

The existing dev profile already uses line tables only and disables dependency
debug info. Integration tests are already consolidated into one binary per
crate. These optimizations should not be presented as new proposals.

## Follow-up implementation validation (2026-11-01)

With the desktop library reduced to `rlib`, all three desktop binaries built,
native strict Clippy and Windows GNU cross-Clippy passed, and Debian and
AppImage packages were produced. The host had the AppIndicator runtime but not
its development metadata: bundling succeeded using the official development
packages extracted under `/tmp` and a temporary `PKG_CONFIG_PATH`, without a
system installation. The packages retained their AppIndicator dependency.

All 23 desktop integration tests passed, as did the input, jobs, plugin host,
plugin SDK, component cache, theme and emulation fixture/source suites. The
broader test command stopped at an existing desktop unit-test failure:
`permissions::tests::every_registered_command_has_a_permission_group` reports
that `video_play` has no classification in `permissions::access`. No command or
permission code was changed for the build work; that failure remains separate.

The selector's 15 regression tests, workflow YAML/actionlint validation and
108 aggregate-result status combinations passed locally. Remote CI execution
and native Windows/macOS bundling were not performed here. No comparable
before/after edit benchmark was taken, so these checks establish compatibility
and reduced work, not a claimed speedup factor.
