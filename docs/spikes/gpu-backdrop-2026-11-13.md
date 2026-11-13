# GPU backdrop before and after, 13 Nov 2026

What #149's GPU backdrop costs against the CSS backdrop it replaces, on the
same build: `CASKADE_BACKDROP=css` draws the theme's animated CSS backdrop
as before #149, and the default draws `backdrop.json` on the GPU. Method,
hardware and caveats are those of the [RTX 3090 timings](native-gpu-2026-11-13.md).

## Setup

- Release build with `bench,smoke,dioxus-native/log-times` (SHA-256 in the
  [results](native-render-2026-11-13/backdrop-measurements.json)), RTX 3090
  over Vulkan (`WGPU_ADAPTER_NAME`), a 1440×1000 window on a private Xvfb
  and D-Bus with networking unshared.
- A read-only copy of the real 8,466-game library and its thumbnails, as in
  the earlier runs; one anonymised account; Nexus.
- [`measure.py`](native-render-2026-11-13/measure.py) samples the main
  process once a second and reads the frame counts the app writes
  (`CASKADE_NATIVE_SMOKE_FRAMES`). Shown figures are seconds 25–45; with
  `--hide-after 45` the window then hides to the tray
  (`CASKADE_NATIVE_SMOKE_HIDE`) and seconds 50–70 are the hidden figures.
- `backdrop-only` pauses Home's hero loops with custom CSS so the backdrop
  is all that moves; plain Home measured the same here.

## Results

| Home, Nexus | Before (CSS) | After (GPU) |
| --- | ---: | ---: |
| Main process CPU, shown | 6.2% of a core | **1.4%** |
| Document frames (resolve, layout, paint) | 1.2 a second | **0** |
| Backdrop frames of their own | – | 0.2 a second (Xvfb; see below) |
| CPU a frame, before presenting | ~3.5 ms (resolve 0.5, scene 2.6, encode 0.7) | **0.1 ms** (acquire 0.04, encode 0.05) |
| Presenting a frame (Xvfb copies it) | 46 ms | 50 ms |
| GPU a frame (timestamps) | 1.07 ms p50, 1.59 p95 (scene) | **0.12 ms p50, 0.15 p95** (backdrop and compose) |
| GPU memory, shown | 145 MiB | 145 MiB |
| Reduced motion | – | 0.18% CPU, no frames |
| Hidden in the tray: CPU | 0.19% | 0.19% |
| Hidden: frames | 0 | 0 |
| Hidden: GPU memory | 20 MiB | 20 MiB |
| Hidden: anonymous memory | 107 MiB (112 shown) | 104 MiB (109 shown) |

Hiding releases the renderer (its surface, kept frame and atlases) and
trims malloc in both columns, since they are the same build: that is part
of #149 too. Master's hidden window wasn't measured here.

**Presentation dominates on Xvfb.** A backdrop frame does a twentieth of a
millisecond of the app's own work, and the GPU a tenth, but Xvfb's present
copies the frame on the CPU for about 45 ms. At Nexus's 6 frames a second
(`ANYRENDER_BACKDROP_CPU_SHARE=1`) that is 27% of a core, all of it in
present. So backdrop frames are kept within 1% of a core, presenting
included: here they come about every five seconds, and the motion steps
accordingly. On a display that presents a frame cheaply the backdrop runs
at its own rate; that measurement is the physical-display check in #120.

**What moved.** Before, each step of the backdrop's CSS animation resolved
the document, rebuilt and encoded its scene and presented it. After, the
document is resolved and painted only when it changes (zero times in the
idle window), and a backdrop frame updates a few dozen bytes of offsets and
records one GPU pass.

## Not measured here

- A physical display's presentation and frame pacing: #120.
- macOS, which draws the CSS backdrop still for now (no intermediate
  texture in its renderer path), and Windows.
- Theme shaders' own cost, which depends on the shader: the GPU-time watch
  is tested with a zero budget (`tests/backdrop.rs`).
