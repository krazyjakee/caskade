# Idle RAM and CPU benchmark

Measures what Caskade costs while it sits idle with a large library, counting
**every** process the app runs: the main process and, for the webview UI, its
WebKit web, network and GPU processes. It exists to compare the webview UI
with the native UI (milestone "Native UI (Blitz + Dioxus)"); results are kept
in the milestone's benchmark issue.

`docs/manual-checks.md` L8 measures one installed build on your own library.
This benchmark is the repeatable one: the same synthetic library, sandbox and
procedure for every build.

## What it does

1. Seeds a synthetic library with `crates/caskade-core/examples/seed_library.rs`:
   8,400 title-only manual games, each with its own generated cover
   (300×450), discovery, launchers and streaming hosting off, onboarding
   done. The app opens on its default view, Home.
2. Runs the app in a sandbox of its own: a user and network namespace (no
   network, so nothing external changes between runs), Xvfb (software GL),
   and a private D-Bus with a throwaway keyring. Nothing outside `--out` is
   read or written, and only processes in the sandbox are stopped.
3. Waits a warm-up (5 minutes), then samples every app process every 10 s for
   30 minutes: anonymous memory plus swap, PSS, threads and CPU time.
4. Writes `samples.csv`, `summary.json` and `summary.md` (the table to paste
   into the results issue), with `meta.json` describing the machine and the
   binary, and `screen.png`, the sandbox's screen as measuring starts (needs
   ImageMagick's `import`), to show the library was what it measured.

## Run it

Build the app under test and the seeder **from the same commit** (the
database schema must match), in release:

```sh
P=(--profile bench-release --config 'profile.bench-release.inherits="release"')
T=$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')
# the webview app: build the UI first, then the binary that embeds it
(cd apps/desktop && pnpm vite build)
flock /tmp/caskade-heavy.lock cargo build "${P[@]}" -p caskade-desktop --bin caskade --features tauri/custom-protocol
flock /tmp/caskade-heavy.lock cargo build "${P[@]}" -p caskade-core --example seed_library
mkdir -p ~/idle-bench/app && cp "$T/bench-release/caskade" ~/idle-bench/app/
cp apps/desktop/src-tauri/binaries/7zz-x86_64-unknown-linux-gnu ~/idle-bench/app/7zz

scripts/idle-bench/run.sh --app ~/idle-bench/app/caskade \
  --seed "$T/bench-release/examples/seed_library" \
  --out ~/idle-bench/webview-$(git rev-parse --short HEAD) --label "webview UI, $(git rev-parse --short HEAD)"
```

For the native UI, pass its binary as `--app` and its process names as
`--exe` (a regex matched against each executable's name; the default,
`caskade|WebKit\w*Process`, covers the webview app).

A run takes about 40 minutes (seeding, 5 minutes of warm-up, 30 of
measuring). `--warmup`, `--duration`, `--interval`, `--games` and `--size`
(the Xvfb screen, default 1600x900) change the procedure; keep them the same
across builds you compare, and say so if you don't.

## Reading the results

- **Anonymous + swap** is what the processes hold that isn't a file: the
  closest measure of what the app costs. PSS shares mapped files (libraries,
  GPU drivers) fairly between processes; it adds driver and library code the
  machine shares with other programs.
- **Idle CPU** is CPU-seconds per minute across all app processes: 60 is one
  core busy all the time.
- Xvfb has no GPU: WebKit paints with llvmpipe (on the CPU), while a wgpu
  renderer may still find the GPU's Vulkan driver. CPU figures from Xvfb
  favour GPU renderers; repeat the comparison on a real display before
  drawing conclusions about CPU.
- The window stays open on Home, the default start view. The library grid
  and idle in the tray aren't measured yet: "Open to" and "Start minimized"
  aren't wired up, and deep links only install, so neither can be reached
  without clicking. Add those scenarios once they can.
