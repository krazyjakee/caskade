# Spike: Blitz instead of the webview (2026-11-08)

Question: would rendering Caskade's UI with [Blitz](https://github.com/DioxusLabs/blitz)
(DioxusLabs' HTML/CSS engine: Stylo + Taffy + Parley + Vello, no browser) make
Caskade lighter or faster than WebKitGTK/WebView2/WKWebView?

**Answer: Blitz's engine is lighter (about 60 MB anonymous memory in one process,
against 100–175 MB across three WebKit processes), but Caskade can't use it without
rewriting its entire 66k-line React UI in Rust (Dioxus Native) and dropping
plugin UI halves. Blitz is in beta, and it misses features Caskade's UI uses. Most of
the memory win at idle can be had far more cheaply by dropping the webview while
Caskade sits in the tray.** Recommendation: don't adopt; revisit when Blitz
reaches 1.0. See "Options" below.

**Update: [Blitsen](#blitsen-the-current-react-ui-on-blitz) (Blitz plus QuickJS) runs the
current React UI unchanged, with no rewrite.** It took two workarounds and still has
one crash, and it isn't lighter than WebKit: about 234 MB anon against WebKit's 178 MB
on Home. While any CSS animation runs, it keeps one core busy on Xvfb.

Blitz tested at commit `9f9c8e3` (2026-11-09, `0.3.0-beta.4`); WebKitGTK 2.52.6;
Linux, X11, RTX 3090 (driver 610). Sources, scripts and the three local Blitz
patches are in [`blitz/`](blitz/).

## What Blitz is today

- **No JavaScript in the product.** `blitz` renders HTML/CSS; `dioxus-native`
  renders a Dioxus (Rust) virtual DOM. A Boa-based JS runtime,
  `blitz-vibey-script`, exists in the repo but is unpublished (`publish = false`).
- **Beta.** Blitz is at `0.3.0-beta.4`, and the git build of Dioxus Native depends
  on Dioxus `0.8.0-alpha.3` (the stable line is 0.7.10). Dioxus 0.8.0-alpha.2 fixed "a leak where removed nodes were
  never freed". The roadmap puts IME, a richer accessibility tree, `position:
  sticky` and smarter paint invalidation at 1.0, with no date.
- **CSS gaps that hit Caskade** (from blitz.is/status/css, counted in our
  sources): `text-overflow: ellipsis` in progress (36 files use `truncate`);
  `backdrop-filter` only on the Skia backend (58 files use `backdrop-blur`);
  container queries unsupported (5 files); `position: sticky` in progress
  (3 files); no 3D transforms, no `<select>` widget, no input placeholder text,
  no IME composition (search fields for CJK input).

## Method

1. Fixture build of the real UI (`VITE_CASKADE_FIXTURES=1`,
   `VITE_CASKADE_FIXTURE_COUNT=8400`, `NODE_ENV=development vite build`, with
   onboarding patched to "completed" in the build output only).
2. `snapshot.cjs` (headless Chromium) renders /library and /settings, then
   saves each as one script-free HTML file with all CSS inlined: 301 and 602
   elements, 155 KB of CSS (Tailwind 4 output).
3. The same snapshots are rendered by WebKitGTK (MiniBrowser) and by Blitz
   (`rdme` windowed, plus a headless probe). The live React UI also runs in
   MiniBrowser. All runs use a private Xvfb with an isolated profile.
   `measure.py` sums memory over the process tree after 20 s, then idle CPU
   over the next 30 s.
4. `caskade_probe script` runs the real React bundle in Blitz's Boa runtime.

Caveat: on Xvfb, WebKit paints with llvmpipe (on the CPU), while Blitz's wgpu
still found NVIDIA's Vulkan driver. Idle CPU therefore isn't comparable;
memory is.

## Results

### Memory, windowed, /library (anonymous MB, all processes)

| Engine | Content | Anon | Processes | Idle CPU s/min |
| --- | --- | ---: | ---: | ---: |
| WebKitGTK | blank page | 76 | 3 | 0.0 |
| WebKitGTK | static snapshot | 102 | 3 | 16.7 * |
| WebKitGTK | **live React UI** | **154** (settings 174) | 3 | 15.8 * |
| Blitz, vello_hybrid (default) | static snapshot | **60** (settings 63) | 1 | 0.1 † |
| Blitz, vello (GPU) | static snapshot | 69 | 1 | 0.1 † |
| Blitz, vello_cpu | static snapshot | 145 | 1 | **107** |
| Blitz headless (no window) | static snapshot | 12 (36 RSS) | 1 | — |
| Blitz + Boa | **live React bundle** | **186** before first render | 1 | — |

\* WebKit is repainting the ambient background animations on the CPU.
† Nothing on screen changed between two screenshots taken 1.3 s apart, so these
windows weren't animating: the spinner was frozen. A real desktop needs to confirm
whether this is an Xvfb artifact.

- In WebKit, the React app itself costs about 52–72 MB of anon memory over the
  same DOM rendered without JS (WebKitWebProcess: 50 → 101–122 MB).
- Blitz's own share is about 52 MB (heap plus anon). Its PSS adds about 90 MB of GPU
  driver mappings (libnvidia-gpucomp, nvidiactl, libLLVM), which are shared in
  practice. The stripped binary is 33 MB, versus a dynamically linked system
  WebKit.
- Blitz's CPU renderer (the fallback where there's no compute-capable GPU)
  spent 107 CPU-s/min repainting a 12-pixel spinner: it redraws the whole frame.

### Speed (headless probe, release build)

| Step | /library | /settings |
| --- | ---: | ---: |
| Parse HTML + 155 KB CSS | 156 ms (cold) | 82 ms |
| Fonts + style + layout | 363 ms (font fetch) | 55 ms |
| Resolve again, nothing changed | 6 ms | 12 ms |
| Resolve after a `<html>` attribute flip | 7 ms | 10 ms |
| CPU raster 1400×900 (vello_cpu) | 36–60 ms | 46–160 ms |

### Fidelity

With animations off, Blitz lays out both pages almost exactly as Chromium does: the
sidebar, cards, gradients, masks, SVG icons, toggles and segmented controls all match
([library](blitz/library-blitz.png) vs
[Chromium](blitz/library-chromium.png),
[settings](blitz/settings-blitz.png) vs
[Chromium](blitz/settings-chromium.png)). Where it differs:

- No ellipsis on truncated text, and no input placeholders.
- Offsets of a few pixels on the toolbar.
- Fonts and SVG `<img>`s sometimes arrive after the first layout, so they can be missing from it (flash of unstyled content; on Blitz's roadmap).
- The ambient `background-position` keyframes (grid pan, nebula) don't run.

### Running the existing React bundle in Blitz (Boa)

This route doesn't work, and won't be competitive even once it does:

- Boa is an interpreter. **Evaluating the 2.5 MB bundle takes 8.6–9 s and 186
  MB of anon memory**, more than WebKit's whole web process with the UI drawn.
- To get that far I had to enable Boa's `Intl`, set `import.meta.url` in Blitz's module
  loader, and stub `ResizeObserver`, `IntersectionObserver`, `MutationObserver`,
  `matchMedia`, `screen`, `visualViewport` and `localStorage`. `HTMLLinkElement` is
  missing too.
- After four rounds of shims, `main.tsx`'s startup `await` chain still
  never settles, and React never mounts.

## Options

1. **Dioxus Native rewrite.** This would save about 90–110 MB of anon memory and two
   processes while the window is open; startup would avoid JS parsing.
   - **Scope:** port 357 files and 62.7k lines of `apps/desktop/src`, plus
     `packages/ui`, from React to Rust RSX.
   - **Libraries to replace:** react-aria (focus, keyboard handling,
     accessibility), TanStack Query, Router and Virtual, Lingui, and the spatial
     focus engine.
   - **Plugins:** UI halves (React ES modules behind an import map) would have no
     host, so third-party plugin UIs would break.
   - **WebKit stays anyway:** wry is still needed for web games and the video
     window.
   - **Accounting:** the main process would grow by about 60 MB, which counts
     against the L8 budget. The webview processes it removes are not counted today.
   - **Gaps:** blocked on the CSS, IME and accessibility gaps above until Blitz 1.0.
2. **Blitz + Boa running the current bundle.** Not viable (see above).
3. **Drop the webview while Caskade is hidden in the tray.** Today
   `window.hide()` keeps WebKit alive (`tray.rs`). Destroying the main webview
   on hide and recreating it on show would free the whole WebKit cost (about 150–175
   MB here) for the hours Caskade spends idle in the tray. The cost is a slower
   reopen (JS parse and first render; not measured here). It needs no rewrite and keeps plugins working. **Recommended as
   the next experiment if idle memory matters.**
4. **Revisit Blitz at 1.0.** Re-run this spike (the scripts here repeat it) once ellipsis, sticky positioning, IME, the accessibility tree and paint
   invalidation land, and once Dioxus Native 0.8 is stable.

## Blitsen: the current React UI on Blitz

[Blitsen](https://blitsen.dev) 0.2.6 (npm) packages built web output into one
executable: Blitz for rendering, QuickJS-ng for JS, and `blitsen/*` native modules.
I exported the same fixture build with `blitsen build --include 'assets/**'`
(66 MB). `blitsen doctor` found 0 errors and 66 warnings: ignored paint effects
such as `backdrop-filter`, Blitz bug 689 (transition values), bug 690 (fixed
positioning), no `IntersectionObserver` or `MutationObserver`, no modal dialogs,
and partial `Intl`. Timings come from `flow.mjs` (`blitsen/test`, headless, CPU
raster) against headless Chromium; memory comes from `measure.py` on Xvfb, as above.

**It works.** React, TanStack Router and Query, react-aria and Lingui all run,
and Home and Library render close to Chromium. Problems found:

| Problem | Effect | Cause | Workaround |
| --- | --- | --- | --- |
| Virtualisation mounts every card | All Games (8,429): **218 s and 1.1 GB anon**, versus 99 ms in Chromium | Taffy sizes the auto-height column `flex min-h-full flex-col` from the content height of its `grow basis-0 min-h-96` row (55,224 px), not from its flex basis. The scroller grows to its full content, so TanStack Virtual sees a 55k-px viewport. | `height: 0` on the growing row: geometry then matches Chromium (558/628 px) and 9 cards mount |
| `localeCompare` is 121× slower than V8 | 931 ms to sort 8,429 strings (`Intl.Collator`: 10×) | probably builds a collator on every call | use one cached `Intl.Collator` (fixtures call `localeCompare` per comparison) |
| Settings crashes | "This view couldn't be shown: NodeFilter is not defined" | react-aria's focus scope needs `TreeWalker`/`NodeFilter`; `doctor` doesn't report it | none tried |
| An error on load | `screen.orientation` is undefined (react-aria's viewport code) | no `screen` | none tried |
| The export drops lazy chunks | dynamic `import()` targets counted as unreachable | asset scan | `--include 'assets/**'` |
| Wheel scrolling | 10 wheel events moved the grid 600 px (Chromium: 6,000) | unclear: driver or engine | — |

With both workarounds:

| | Blitsen | Chromium |
| --- | ---: | ---: |
| Launch to first content (500 fixtures) | 600 ms | 280 ms |
| Launch to first content (8,400 fixtures, generated in JS) | 4.5–5.9 s | 340 ms |
| Open Library / All Games (8,429) / Settings | 300 / 618 / 240 ms | 224–244 / 90–99 / 200–230 ms |
| Peak anon during the flow (8,400 fixtures) | 172 MB | — |
| Plain JS (map, filter, sort, JSON) | 2–5× slower than V8 | — |

Windowed on Xvfb (wgpu used the NVIDIA card; WebKit painted on the CPU):

| | Anon | PSS | Processes | Idle CPU s/min |
| --- | ---: | ---: | ---: | ---: |
| WebKitGTK, React UI, Home | 178 | 339 | 3 | 14.5 |
| Blitsen, React UI, Home | **234** | 347 | 1 | **60** |
| Blitsen, static snapshot | 104 | 227 | 1 | 60 |
| Blitsen, static snapshot, animations off | 90 | 211 | 1 | **0.06** |

- QuickJS holds about 130 MB more than the static page (234 − 104). WebKit's
  JavaScriptCore costs 52–76 MB more for the same UI. Both heaps include the
  8,400 fixture records, which the real app keeps in Rust.
- Any running CSS animation (the status-bar spinner is enough) keeps the main
  thread at 100%, whether or not JS is running. It drops to nothing once nothing
  animates. Xvfb has no vsync, so frames may be uncapped there; check on a real
  display.
- **Fitting it into Caskade:** Blitsen has no Tauri. The webview's commands and
  events would need a new transport to `caskade-core`. Custom Rust reaches the
  page through Node-API addons, which need the larger Bun-hosted export, not the
  QuickJS one. `blitsen/window` can't create windows, so the game, video and
  web-player windows would stay on tao/wry. Blitsen is pre-alpha.

**Verdict:** this route is far more realistic than a Dioxus rewrite, and Blitsen
should fix the issues above anyway (all but the core transport are general
web-compatibility bugs). It doesn't yet save memory or CPU against WebKitGTK,
though. Re-measure once the flex sizing, `NodeFilter`, `localeCompare` and
animation-frame issues are fixed, and on a real display.

## Not measured

- A real desktop with a real GPU (rendering is software on Xvfb). Check on the
  user's display: whether Blitz's GPU renderer animates the spinner, and its
  idle CPU.
- Windows (WebView2) and macOS (WKWebView).
- Interactions: scrolling 8,400 virtualised cards and per-frame cost while
  animating. Blitz's no-op resolve costs 6–12 ms, so a real Dioxus prototype
  would need frame profiling.
