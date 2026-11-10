# Skia against vello for caskade-native (2026-11-10)

Issue #128. A Blitz maintainer suggested anyrender's Skia backend for lower
CPU use from painting (DioxusLabs/anyrender#149). BLITZ-0 (#79), BLITZ-3
(#82) and BLITZ-31 (#110) measured only vello. This measures Skia, on the
GPU and on the CPU, against what caskade-native ships: vello_hybrid with
damage-limited repaint on a GPU, and vello_cpu with damage where there's no
usable GPU.

**Recommendation: stay on vello, in both modes.** Skia draws backdrop blur,
but it costs more CPU in every case that decides our designs:

- **The spinner.** On the GPU, Skia repaints the whole window for a 24 px
  spinner, because it has no damage path. That costs 4.2 times
  vello_hybrid's CPU a frame (3.6 ms against 0.85 ms, before the present).
- **The CPU renderer.** For a full frame, Skia's raster renderer is 2.4
  times slower than vello_cpu: 42 ms against 17 ms at 1440×900.
- **Building and shipping.** It adds a C++ library downloaded at build
  time. There's no prebuilt for windows-gnu, and none for MSVC with the
  static CRT the release uses.

## How it was measured

The BLITZ-0 prototype (`apps/native-proto`, the `proto` profile, release
with line tables) read a fresh copy of the real library (8,466 games). It
ran on a private 1440×900 Xvfb (`scripts/private-desktop.sh`), with a
1440×900 window, the bench's 20 s settle and then 30 s measured. These are
the conditions of #110's and #121's tables, and the harness of #82's
(`scripts/frames.py`). CPU is the whole process, all threads.

The machine: Ryzen 9 5900X; RTX 3090 (NVIDIA 610.43, Vulkan); Mesa 25.2.8
(llvmpipe, lavapipe).

Each variant was built from the pinned stack in `vendor/blitz` with the
patch series:

- **vello_hybrid + damage:** the shipped GPU path. Run with
  `ANYRENDER_PRESENT_MODE=immediate` and `ANYRENDER_FRAME_LIMIT_HZ=60`, as
  in #82.
- **vello_cpu + damage:** the shipped no-GPU path (softbuffer).
- **Skia GPU:** `dioxus-native/skia`. On Linux, upstream picks only Skia's
  OpenGL backend, and a private Xvfb's GL is Mesa's llvmpipe, which is
  software. To measure Skia on the real GPU, a scratch build selected Skia's
  existing Vulkan backend (`ANYRENDER_SKIA_BACKEND=vulkan`, about 10 lines)
  and paced it with the same 60 Hz limiter.
- **Skia raster:** `dioxus-native/skia-softbuffer`. Its `ImageRenderer` has
  no region path, so it repaints the whole window. A scratch build added one
  (clip, clear, draw: about 30 lines) to see what damage would give it.

None of this changes the repo: the scratch crates and builds lived outside
it and are deleted.

**The scenarios:**

- **Idle:** nothing animating.
- **Spinner, continuous:** `steps(600)`, which changes every frame.
- **Spinner, `steps(8)`:** the shipped spinner.
- **Ambient:** the theme's stepped backdrop, which repaints the whole window
  about 9 times a second.
- **Scroll:** the library grid scrolled at 3000 px/s.
- **Blur:** `backdrop-filter: blur(8px)` (our token cap) on the top bar,
  sidebar and status bar, added to the prototype's CSS.

**Xvfb's present.** Presenting a Vulkan frame to Xvfb copies the whole
window back to the CPU: about 37 ms a frame for both GPU renderers. That
caps them at about 25 frames a second, and it isn't what a display costs.
The GPU table therefore gives each frame's CPU *before* the present:
process CPU per frame, minus the present's wall time. The vsynced
real-display run is a #120 check (below).

## Results

### On a GPU (RTX 3090 through Vulkan)

CPU a frame before the present, and frames a second:

| Running | vello_hybrid + damage | Skia GPU (Vulkan) |
| --- | ---: | ---: |
| Idle (CPU, % of one core) | 0.30% | 0.33% |
| Spinner, continuous | **0.85 ms** (774 of 775 frames damage-only) | **3.58 ms** (every frame full) |
| … at 60 Hz, before the present | 5.1% of a core | 21.5% of a core |
| Spinner, `steps(8)` (7.7 frames/s) | 1.28 ms: **1.0%** of a core | 4.12 ms: **3.2%** of a core |
| Ambient backdrop (8.8 frames/s, full frames) | 4.82 ms | 4.25 ms |
| Scrolling the library (full frames) | 2.89 ms | 3.27 ms |
| With blur: spinner, continuous | 0.73 ms (no blur drawn) | 3.84 ms |
| With blur: scrolling | 3.04 ms (no blur drawn) | 3.15 ms |
| Startup to first frame (median of warm runs) | 0.26 s | 0.22 s |
| Memory (RSS) | 214–217 MB | 186–215 MB |

The blur's cost on the GPU doesn't show in CPU time. On the GPU it's GPU
work, and Skia's CPU cost a frame barely moves. Scrolling ran at the same
rate in both (about 25 frames a second), held there by Xvfb's present.

### Without a usable GPU

| Running | vello_cpu + damage | Skia raster + damage (scratch) | Skia raster (upstream: full frames) | vello_hybrid on lavapipe | Skia GPU on llvmpipe (GL) |
| --- | ---: | ---: | ---: | ---: | ---: |
| Idle (CPU) | 0.13% | 0.17% | 0.20% | 1.97% | 1.40% |
| Spinner, continuous: CPU a frame | **0.46 ms** | 0.72 ms | 43.8 ms | 47.9 ms | 393 ms |
| … at 60 Hz | 2.8% | 4.3% | (22.7 frames/s at 99.6%) | 287% | (34 frames/s at 1342%) |
| Spinner, `steps(8)` (CPU) | **0.57%** | 0.73% | 33.2% | 39.4% | 328% |
| Ambient backdrop (CPU) | 0.10% (paused: `data-paint="cpu"`) | 40.5% (not paused here) | 38.7% | 473% | 360% |
| Scrolling: frame time, p50 / p95 | **17.2 / 20.0 ms** (57 frames/s) | 41.6 / 47.0 ms (24 frames/s) | 39.9 / 41.9 ms | 38.2 / 43.5 ms | 22.9 / 27.8 ms, at 13.7 cores |
| With blur: spinner, continuous | — | 0.96 ms (+0.24) | — | — | — |
| With blur: scrolling, CPU a frame | — | 43.2 ms (+0.7) | — | — | — |
| Startup to first frame (warm) | 0.08 s | 0.12 s | 0.11 s | 0.27 s | 0.21 s |
| Memory (RSS) | 54–56 MB | 60–61 MB | 60–61 MB | 328–341 MB | 317–331 MB |

**What it shows:**

- **Full frames.** vello_cpu is 2.4 times faster than Skia's raster
  renderer: 17 ms against 40–42 ms for 1440×900.
- **Damage-only frames.** Both are cheap, even with damage added to Skia,
  but vello_cpu is still the cheaper (0.46 ms against 0.72 ms).
- **Skia GL on a software adapter** is the worst configuration measured: a
  continuous spinner kept 13 cores busy. This is the case #110 refuses
  (software adapters go to the CPU renderer), and Skia would need the same
  refusal.
- **Blur on the CPU.** Skia's raster renderer draws a σ 8 px backdrop blur
  over the three chrome surfaces for about 0.25 ms in a spinner frame and
  0.7 ms in a full frame.

## Damage with Skia

- **Raster.** Skia can repaint only damage cheaply: the softbuffer buffer
  keeps the last frame, and Skia clips to the region. The scratch version is
  about 30 lines, and a damage frame took 0.72 ms.
- **GPU.** A Skia GPU surface is a fresh swapchain image each frame, so
  damage needs a persistent offscreen surface, a clip, and a copy to the
  swapchain. That's the shape of our vello_hybrid patch (anyrender 0001 and
  0002), written again against Skia's Ganesh backends: GL, D3D, Metal and,
  if we wanted it, Vulkan.

**What our vello patches would become with Skia:**

- **blitz 0004** (damage tracking and stepped-animation sleeps) is
  renderer-independent and stays as it is.
- **anyrender 0001 and 0002** (vello_hybrid damage) would be rewritten for
  each Skia GPU backend.
- **anyrender 0005** (vello_cpu damage) becomes the 30-line raster region
  path above.
- **blitz 0026 and anyrender 0006** (the no-GPU fallback) are still needed.
  Skia panics when its backend can't start (`Failed to initialize Skia
  backend`), and its GL path happily runs on llvmpipe at the cost shown
  above.
- **anyrender 0007** (a type-erased `PaintScene`) stays: the fallback
  renderer needs it whichever backends it switches between.

## Building and shipping Skia

skia-safe 0.156 gets Skia from `skia-bindings`' build script. It first
tries a prebuilt archive from GitHub (`rust-skia/skia-binaries`, keyed by
target and features). If there's none, it falls back to building Skia from
source: that downloads Skia's sources and syncs its dependencies (about
1 GB), then needs Python 3, gn, Ninja and clang (clang-cl on Windows).

| Target (as we build it) | Where Skia comes from | Result |
| --- | --- | --- |
| Linux x86_64 (release leg, deb and AppImage) | prebuilt `x86_64-unknown-linux-gnu-ganesh-gl-jpegd-jpege-pdf` (6 MB download) | built here: the Skia crates add about 10 s to a cold build |
| Windows MSVC (release leg, `windows-latest`) | a prebuilt exists (`…-msvc-d3d-ganesh-gl-jpegd-jpege-pdf`), but the release builds `caskade-native` with `+crt-static` (#110), and there's no static-CRT prebuilt (the `-static` key returns 404) | a full source build on the runner: clang-cl, Python, Ninja, ~1 GB of sources. Or drop `+crt-static` and ship the VC++ runtime again |
| Windows MSVC without `+crt-static` | prebuilt (9 MB) | downloads (checked here with `cargo check`) |
| Windows GNU (cross from Linux: our VM and local builds) | no prebuilt (404) | the source fallback synced the sources, then failed at `gn gen` for `target_os="win"` with `--target=x86_64-pc-windows-gnu` |
| macOS aarch64 and x86_64 (release legs) | prebuilt `…-apple-darwin-ganesh-jpegd-jpege-metal-pdf` (4–5 MB) | not tried: no Mac here |

**Other build and ship costs:**

- **Offline and CI.** The download happens at build time, from GitHub.
  Offline builds need a mirror (`SKIA_BINARIES_URL=file://…`). If the
  download fails, the build silently turns into the source build above.
- **Linking.** Skia links statically (`libskia.a`, a 23 MB archive) and adds
  `libstdc++.so.6` to the Linux binary. FreeType and fontconfig come from
  the system, as they already do.
- **Size.** Stripped Linux binaries of the prototype: vello_hybrid 44.6 MB,
  vello_cpu 39.8 MB, Skia GPU 43.3 MB, Skia raster 40.9 MB (12.3, 11.0,
  12.2 and 11.4 MB with xz). Shipping Skia for one mode beside vello for the
  other adds about 3–4 MB.
- **Licences.** Skia is BSD-3-Clause, and skia-safe and skia-bindings are
  MIT. The prebuilt also contains libpng, libjpeg-turbo, zlib, expat and
  Wuffs, all permissive. Their notices would join the bundles' third-party
  list.

## What it changes

Nothing ships differently.

- **The stepped spinner** stays at `steps(8)`. With Skia on the GPU it would
  cost 3.2% of a core before the present, over the 2% budget, until Skia had
  a damage path.
- **No backdrop blur** stays. Skia is the only renderer that draws it, and
  only cheaply on the raster path. On the GPU it would come with the
  spinner's cost, and a mix (Skia raster for blur, vello on the GPU) would
  draw glass differently on different machines. The design decision (#85:
  glass at least 80% opaque) didn't depend on the blur's cost.
- **The no-GPU fallback** stays vello_cpu with damage: it beats Skia raster
  on full frames (2.4 times) and on damage frames.
- **#127's rows** don't change. The vello-specific patches (blitz 0004 and
  0026, anyrender 0001, 0002, 0005, 0006 and 0007) stay needed. No Skia
  patch is carried.

## Left for the user (#120)

- A vsynced real-display run on the GPU: vello_hybrid against Skia's
  default (OpenGL) backend on the NVIDIA desktop, the same scenarios, with
  the display's present instead of Xvfb's readback.
- Windows (D3D12) and macOS (Metal), where Skia's other GPU backends run.
