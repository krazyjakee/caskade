# Spike: faster Rust builds (2026-11-08)

Question: would a different code generator, linker or compiler mode make Caskade's
builds faster?

**Answer: no, not for the edit loop.** After an edit, the build is dominated by the
compiler's front end in `caskade-core` (138k lines in one crate) and
`caskade-desktop`, then the desktop binaries, one after another. Cranelift and mold
change nothing. The parallel front end cuts cold builds by a fifth to a quarter.

Setup: Rust nightly 1.101 (2026-10-19), 24 cores, the shared target directory. Each
variant used its own Cargo profile (`bench-*`), so it started cold and didn't touch
anyone's `debug/` artifacts. Every step ran under the heavy lock. The edit adds a
private function to `crates/caskade-core/src/sessions/sqlite.rs`. Script and raw
results are in [`build-speed/`](build-speed/).

| Variant | Cold build (core + desktop) | Cold `test --no-run -p caskade-core` | Edit, then build | Edit, then test build |
| --- | ---: | ---: | ---: | ---: |
| LLVM (baseline; rust-lld is the default linker) | 136 s | 106 s | 20–27 s | 13 s |
| Cranelift (build scripts and proc-macros on LLVM) | 134 s | 101 s | 22–27 s | 14 s |
| LLVM + mold 3.0 | 145 s | 109 s | 24–29 s | 17 s |
| LLVM + `-Zthreads=8` | **108 s** | **76 s** | 22 s | 16 s |

## Where an edit's 20–25 s go

Critical path from `--timings`, one step after another:

1. `caskade-core`: 9.8 s (front end 8.2 s, code generation 1.6 s).
2. `caskade-desktop`: 6.4 s (front end 5.4 s). Cargo rebuilds it after any change to
   core.
3. Binaries `caskade`, `export-bindings` and `caskade-catalogue`: 3–3.5 s each, in
   parallel.

`-Ztime-passes` for the core rebuild (10.1 s): writing metadata for dependents
2.3 s, proc-macro expansion 1.8 s (about 1,500 serde and specta derives, 219
`async_trait`), saving incremental state 1.4 s, monomorphization 0.7 s, coherence 0.5 s,
name resolution 0.9 s, type-check 0.6 s, borrow-check 0.3 s, linking 0.4 s. Nearly
all of this scales with the size of the crate, not the size of the edit.

## Findings

- **Cranelift:** code generation is only 15% of an edit rebuild. As-is it also
  breaks the build: `tauri-plugin-fs`'s build script aborts on an unsupported SSE2
  intrinsic (`llvm.x86.sse2.pmulhu.w`), so build scripts need LLVM. Runtime SIMD
  code could fail the same way.
- **mold:** linking takes about 0.4 s and rust-lld is already the default, so there's
  nothing left to win.
- **Parallel front end** (`-Zthreads=8`, nightly): −21% on cold builds and −28% on cold
  test builds, no change to the edit loop. Worth it for CI, fresh checkouts and
  agents in fresh worktrees, if nightly is acceptable.
- **Cold builds:** `aws-lc-sys`'s build script (rustls' default crypto provider,
  compiled from C) takes 36 s. Using `ring` where possible would cut that; not
  checked against the iroh pins.

## What would speed up the edit loop

1. **Split `caskade-core`** into crates along its modules. The fixed per-crate costs
   above (metadata, expansion, incremental state) shrink with the crate an edit
   lands in. `caskade-desktop` (about 6 s) and the binaries (about 3.5 s) still rebuild
   after any core change, so expect roughly 20–25 s down to 12–15 s for edits in a
   leaf crate. Not measured.
2. **Make `caskade-desktop` cheaper to rebuild:** its 5.4 s front end runs even
   when nothing it uses changed. Moving the command modules into their own crates
   would shrink it.
3. **The heavy lock:** with several agents, waiting for the shared lane often costs
   more than compiling.

## Outcome (2026-11-09)

Done on this branch:

- **Six feature crates over `caskade-core`:** `caskade-streaming` (with `elevation`),
  `caskade-achievements` (with the notification feed), `caskade-backup`,
  `caskade-addons`, `caskade-share` and `caskade-system-stats`. Core went from 138k
  to 106k lines. Nothing in core depends on them; `caskade-desktop` does.
- **rustls on ring only:** two vendored one-line patches (`hudsucker`,
  `librqbit-sha1-wrapper`) keep aws-lc out of every build.

Re-measured the same way as above (nightly 1.101, LLVM, a fresh profile;
[`build-speed/results-after.txt`](build-speed/results-after.txt)):

| | Before | After |
| --- | ---: | ---: |
| Cold build (core + desktop) | 136 s | 133 s |
| Edit in core, then build core + desktop | 20–27 s | 20–22 s |
| Edit in core, then `test --no-run -p caskade-core` | 13.4 s | 12.6–13.5 s |
| Edit in a moved crate (`caskade-streaming`), then build core + desktop | 20–27 s (it was in core) | **11.4–11.8 s** |
| Edit in a moved crate, then build its tests | 13.4 s (core's) | **2.9 s** |

- Edits in the moved features no longer rebuild core, so they take about half the
  time, and their tests rebuild in seconds. Edits in core cost the same as before.
- aws-lc's build script (37 CPU-s) was never on the cold build's critical path. It
  ran beside `tokio → h2 → hyper → axum → librqbit → caskade-installer →
  caskade-core → caskade-desktop`, so cold builds only gained about 3 s of wall
  time (more on machines with fewer cores, and Windows cross-builds no longer
  compile AWS-LC).
- Next, if wanted: take the torrent client (`librqbit`, about 30 s of the cold
  critical path) off core's dependency path, and move more of core's
  lightly-depended-on features out the same way (the module graph is in the
  analysis above).
