# BLITZ-0: the library on Dioxus Native, measured (2026-11-10)

Gate for the Native UI milestone (#113), issue #79. The PR #78 spike
([`blitz.md`](blitz.md)) measured static snapshots; this measures a working
Dioxus Native prototype of the library, reading a copy of a real 8,456-game
library straight from `caskade-core`, against today's webview build of the
same commit on the same library.

**Decision: go, with conditions** ([below](#decision-and-conditions)).
Idle, the native UI holds about half the memory of today's app and its CPU
is near zero, on Linux and Windows; it starts about three times faster. But
Blitz repaints the whole window for every animation frame, about 9 ms of CPU
on a fast desktop, so a running spinner would cost about half a core at
60 Hz against a budget of 2%. That has to be fixed in Blitz (BLITZ-3, #82)
or designed out before any view ships. The real-display criteria (spinner
cost and scroll pacing at the refresh rate) couldn't be run: this machine's
monitors were off (DPMS) all session.

## What was built

[`apps/native-proto`](../../apps/native-proto/README.md), a throwaway binary
with its own Cargo workspace and lockfile:

- Blitz `9f9c8e3` (0.3.0-beta.4, the spike's commit), anyrender `812644a`,
  Dioxus 0.8.0-alpha.3; Blitz's default renderer, `vello_hybrid`, unless noted.
- Sidebar, top bar with one text field (it filters through `list_games`),
  status bar with the spinner, and the library grid virtualised as the
  webview's (`VirtualLibrary.tsx`: 312 px rows, 176 px minimum card width,
  one row of overscan, rows placed with `transform`), with 100-game pages
  fetched around what's on screen and 320 px cover thumbnails.
- The app's own styles: `apps/desktop/src/styles/index.css` (tokens, base,
  Tailwind 4 utilities) and the default theme, compiled against the
  prototype's classes (29 KB of CSS).
- Arrow keys and Page Up/Down move a focus ring between cards and keep it on
  screen; pads (`caskade-input`) drive the same moves.
- Built-in benchmarks (`CASKADE_PROTO_BENCH`) that drive the grid themselves,
  so they can run on a real display without synthetic input.

## Method

- **Library:** a copy of the user's library (8,466 games, 8,456 shown, 5,815
  with covers, 7.2 GB of artwork), with the sync device key removed and, so
  that the webview opens without the account picker, one admin account
  without a PIN. Each run starts from a fresh copy.
- **Webview:** a release build of master `89bfad24` (`tauri/custom-protocol`),
  opened on Library → All Games by two clicks on the private display.
- **Linux sandbox** (`scripts/run.sh`): user and network namespace (no
  network), private Xvfb at 1440×900, private D-Bus and throwaway keyring.
  3 minutes to settle (2 for the CPU renderer), then 5 minutes (3) of samples
  every 10 s of every app process (`sample.py`): anonymous memory plus swap,
  PSS, threads and CPU time. Xvfb has no GPU: WebKit paints with llvmpipe,
  while wgpu still renders on the NVIDIA card but copies every frame back to
  present it (about 45 ms). Memory compares fairly; CPU while animating doesn't.
- **Windows:** Windows Server 2022 in the KVM lab, which has no GPU (Microsoft
  Basic Display Adapter, so D3D12 runs on WARP, the software rasteriser), at
  1024×768 and 125%. Private bytes, private working set and CPU of every app
  process (`scripts/windows/sample.ps1`), 1 minute to settle and 3 of samples.
  Windows trims idle working sets in this 6 GB guest, so private bytes
  (commit) is the stable figure.
- **Frames:** Blitz's `log-frame-times` and `log-phase-times`, timestamped by
  `scripts/frames.py`; startup by grabbing the display from process start
  (`sample.py --startup`); latency by `scripts/latency.sh`.

## Results

### Linux, X11, RTX 3090 (driver 610.43), Ryzen 9 5900X; Xvfb 1440×900

Idle on the library. Anonymous memory + swap, MB.

| App | Processes | Main / UI | Web processes | **Main + web** | PSS | CPU-s/min |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Webview `89bfad24`, All Games | 3 (+ Sunshine) | 114.2 | 223.0 + 7.6 | **344.8** | 836.6 ‡ | 5.05 * |
| Webview `89bfad24`, Recently Played | 3 (+ Sunshine) | 120.0 | 216.7 + 7.6 | 344.3 | 764.7 ‡ | 14.3 * |
| Native, vello_hybrid, ambient paused | 1 | **67.0** | — | **67.0** | 124.5 | **0.12** |
| Native, vello_hybrid, ambient running | 1 | 67.7 | — | 67.7 | 222.5 | 60.2 † |
| Native, vello_hybrid, spinner running | 1 | 67.5 | — | 67.5 | 226.3 | 60.2 † |
| Native, vello_cpu, ambient paused | 1 | **18.5** | — | **18.5** | 43.5 | **0.02** |
| Native, vello_cpu, spinner running | 1 | 18.9 | — | 18.9 | 43.8 | 59.6 § |
| Native, no window (`--core-only`) | 1 | 5.2 | — | 5.2 | 12.2 | 0 |

\* Main + web processes; WebKit paints the ambient background on the CPU (llvmpipe).
† Every frame is copied back from the GPU (about 45 ms), so animations run
at about 20 frames a second on one busy core.
§ No vsync on Xvfb: about 75 frames a second, one core.
‡ PSS includes Sunshine (the streaming host the copied settings enable, about
60 MB anon), which isn't part of either UI.

- **The UI's own cost:** the GPU build holds 61.8 MB more than its core
  alone (67.0 − 5.2); the CPU build 13.3 MB. So Blitz, Stylo, the DOM, fonts
  and decoded covers take about 13 MB, and the NVIDIA driver's Vulkan
  allocations about 49 MB. The webview's web and network processes hold
  230.6 MB.
- **Projected native app:** today's main process plus the prototype's UI
  share is about 114 + 62 = **176 MB, against 345 MB today**: about half. That
  is an upper bound: the native main process also sheds GTK and WebKit's
  UI-process half, and the prototype ran no background services (sync,
  discovery, stores), which the main process carries either way.
- **Threads:** 41 (Tokio, Stylo's style pool, wgpu) against 134 across the
  webview's processes.
- **Startup**, process start to visually complete, 3 runs each: the native
  prototype shows the library grid in **0.67–0.70 s** (first frame at
  0.23–0.29 s on the real display); the webview shows Home in **2.05–2.23 s**,
  including its restart onto WebKit's GPU renderer on NVIDIA.
- **Input latency**, pointer event to the end of the frame that answers it
  (hover, 40 events): **20.6 ms** median (p95 23.8) with the CPU renderer;
  46.4 ms with vello_hybrid, almost all of it Xvfb's 45 ms copy-back.
- **Scrolling with the CPU renderer** (softbuffer presents from the CPU, so
  Xvfb is representative): 75–76 frames a second at 1,000 and 3,000 px/s,
  frame interval p95 14.5–16.5 ms, 12.2–12.5 ms of raster a frame. It would
  hold 60 Hz, with one core busy.

### Linux, X11, RTX 3090: the real display

**Not measured.** The monitors were in DPMS off for the whole session. With
no vblank, X presents every vsynced app (`vkcube` and `glxgears` as well)
about once a second, so frame pacing couldn't be observed. The 1 Hz runs
still measured per-frame work on the real GPU:

| Frame | Style + layout | Scene building | GPU submission | Total | CPU per frame |
| --- | ---: | ---: | ---: | ---: | ---: |
| Spinner running | 0.64 ms | 6.5 ms | 1.5 ms | 8.7 ms | about 10 ms ¶ |
| Ambient background running | 0.44 ms | 6.1 ms | 1.6 ms | 8.1 ms | about 10 ms ¶ |
| Scrolling, 3,000 px a frame | 0.89 ms | 2.8 ms | 1.3 ms | 5.0 ms | — |

¶ 1.25% of a core at one frame a second, less 0.25% idle.

- A frame fits a 16.7 ms refresh with room to spare while scrolling, so the
  grid can probably hold 60 Hz on this machine; whether presents keep pace
  needs a display that's on.
- An animation frame costs about 9–10 ms of CPU however small the animated
  element: Blitz rebuilds the scene for the whole window. At 60 Hz that is
  **about 52–60% of one core** for the 14 px spinner. The budget (2% of a
  core) allows 0.33 ms a frame.
- With the screen off, the spinner costs 0.68 CPU-s/min: presents block
  rather than spin.

### Windows Server 2022 VM (no GPU), 1000×700

| App | Processes | Private bytes | Private working set | CPU-s/min idle | CPU-s/min, spinner |
| --- | ---: | ---: | ---: | ---: | ---: |
| Webview `89bfad24`, All Games (maximised) | 1 + 6 WebView2 | 188.9 + 121.0 = **309.9** | 103.8 + 37.4 | 1.75 + 0.11 | — |
| Native, vello_hybrid on WARP | 1 | **194.2** | 3.8–110 (trimmed) | 1.2 | **108.5** |
| Native, vello_cpu | 1 | **18.1** | 12.2 | 1.08 | **60.8** |

- The GPU build's extra 176 MB over the CPU build is WARP, the software
  D3D12 device; on real hardware it is the GPU driver's share.
- Without a GPU, any animation costs one to two cores: each frame is
  rasterised in full on the CPU.

### Not run (user gates)

| Platform | Why | What to run |
| --- | --- | --- |
| Linux X11 on this machine, real display | monitors off all session | `display-suite.sh` (below), monitors on, the machine left alone for ~6 minutes |
| Linux Wayland, NVIDIA | no Wayland session here | `display-suite.sh` in a Wayland session with `DISPLAY` unset |
| Linux, AMD or Intel GPU (X11 and Wayland) | no such GPU here | `display-suite.sh` |
| Windows 11 on hardware | the lab VMs have no GPU | `scripts/windows/` (README) with `proto-hybrid.exe`, and `CASKADE_PROTO_BENCH=scroll` with the `log-frame-times` build |
| macOS | no Mac | build there, then `display-suite.sh` |
| Steam Deck in gamescope | no Deck | `gamescope -W 1280 -H 800 -r 60 -- apps/native-proto/scripts/display-suite.sh …` |

```sh
# a library copy as scripts/run.sh makes one; the prototype built into bin/
apps/native-proto/scripts/display-suite.sh --bin bin --home ~/blitz0-home \
  --out blitz0-$(hostname)-$(date +%F) --refresh <the display's Hz>
```

## Acceptance criteria

| Criterion | Status |
| --- | --- |
| A results table per platform under `docs/spikes/` | Linux X11 (Xvfb, and per-frame work on the real GPU) and the Windows VM; the rest are listed above as not run |
| Idle memory of the prototype (one process) below today's main + web on the same library | **Met.** Linux: 67 MB (GPU) or 18.5 MB (CPU) against 345 MB. Windows VM: 194 MB (WARP) or 18 MB (CPU) against 310 MB |
| Idle CPU near zero when nothing animates; a running spinner under 2% of one core on a vsynced display | Idle: **met** (0.12 CPU-s/min on Linux, 1.1–1.2 on Windows). Spinner: **not met**: about 9 ms a frame on the real GPU, so about 50% of a core at 60 Hz; not run on a vsynced display |
| Scrolling holds the display's refresh rate on the reference machines | **Not measured** (no display on). Frame work while scrolling is 5 ms on the RTX 3090; the CPU renderer reaches 75 frames a second |
| A go, go-with-conditions or stop decision in the tracking issue's Decisions | Done: #113 |

## The spike's open questions

- **Do Vello's GPU renderers animate?** Yes. The spike's frozen spinner was
  an artifact of its setup: here vello_hybrid animates the spinner and the
  ambient background on Xvfb and on the real GPU.
- **What does an animation cost per frame?** A whole frame: 8–9 ms on the
  RTX 3090 and Ryzen 9 5900X (most of it scene building on the CPU), 12.5 ms
  of raster with vello_cpu, one to two cores on WARP. Blitz has no damage
  tracking: a 14 px spinner repaints 1440×900.
- **Does the flex sizing bug (BLITZ-2) bite in RSX layouts?** Yes. The
  webview's grid layout (`flex min-h-full flex-col` > `flex min-h-96 grow
  basis-0` > the scroller) grew the scroller to its 440,000 px of content, so
  the outer view scrolled instead and the virtual list drew nothing past the
  first screen. The spike's workaround (`height: 0` on the growing row) fixes
  it; `CASKADE_PROTO_FLEX_BUG=1` shows the bug.

## Other findings

- **No ellipsis** on truncated titles and **no input placeholders**
  (BLITZ-1, #80): titles are cut mid-letter.
- **Clicking a focusable element doesn't focus it.** A click on the grid
  (`tabindex="0"`) left the focus in the search field, so the arrow keys never
  reached the grid; Tab does move the focus there, and the arrows then work
  (BLITZ-13, #92). Pads were wired but not exercised: no pad here.
- **Programmatic scrolls send no `scroll` event** (a TODO in Blitz's
  `scroll_to`): a virtual list scrolled from code must track its own offset.
- **Fonts:** fontique registers bundled fonts under their own names ("Inter",
  not Fontsource's "Inter Variable"), so the `--cs-font-*` tokens need to name
  those.
- **Binary size**, stripped: 41 MB on Linux (36 MB with only the CPU
  renderer); 94 MB for Windows with the GNU toolchain, mostly code.
- **Windows build:** wgpu-hal 30's D3D12 backend needs `gpu-allocator` built
  against `windows` 0.62; a lockfile that already holds 0.61 picks that one
  and fails to compile.
- **Dioxus 0.8 alpha** worked without patches: signals, `use_future`,
  `onmounted`, `onscroll` and element `scroll()`.
- **On NVIDIA**, the GPU driver's allocations (about 49 MB anon) are most of
  the native UI's memory; the L8 budget should count them.

## Decision and conditions

**Go, with conditions.** The memory case holds by a wide margin on real data
(about 345 → 176 MB on Linux, one process instead of three or seven), idle
CPU is near zero, it starts about three times faster, and the app's own
styles render as the spike's side-by-side comparison found.
The failures are in Blitz's paint pipeline, which the milestone already has
an issue for, not in the approach. Work goes on as follows:

1. **BLITZ-3 (#82) is a hard gate before any view (#96 onwards).** An
   animation must cost only what it changes: on the reference desktop a
   running spinner must stay under 2% of one core at 60 Hz (0.33 ms a frame),
   through damage tracking and scene reuse upstream or a pinned patch. If
   that isn't reachable, the native UI ships with no continuous animation (a
   static busy indicator, the ambient background off), or the migration stops
   before the views.
2. **Real-display runs before views too:** `display-suite.sh` on this machine
   with the monitors on, then Wayland, AMD/Intel, Windows 11, macOS and the
   Steam Deck in gamescope. The grid must hold the refresh rate on each; a
   Deck that can't is a stop.
3. **Without a GPU, animations stay off** (BLITZ-31, #110): software
   rendering (WARP, llvmpipe, vello_cpu) costs one to two cores for any
   animation, so it must force reduced motion. Memory in that mode is the
   lowest of all (18 MB).
4. **BLITZ-2's workaround becomes the pattern** until Taffy is fixed
   (#81): a growing flex child that contains a scroller gets `height: 0`.
5. **Foundations proceed now:** they don't depend on paint cost and help
   either way: `caskade-app` (#86), the data layer (#89), theming (#90),
   translations (#91), input and focus (#92, including click-to-focus) and
   the test strategy (#95).
6. **Pins:** Blitz `9f9c8e3`, anyrender `812644a`, Dioxus 0.8.0-alpha.3 are
   the measured baseline; move them only together, and rerun these
   measurements (the scripts in `apps/native-proto`) on every bump.
