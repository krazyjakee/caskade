# RTX 3090 frame timings, 13 Nov 2026

The app is using the hardware GPU. Its scene command buffer is fast; most
elapsed frame time in these private-window runs is outside GPU scene
execution. This follows the [initial rendering investigation](native-render-2026-11-13.md).

## Hardware and measurement

- AMD Ryzen 9 5900X, NVIDIA GeForce RTX 3090, proprietary driver 610.43.02.
- The renderer logged `NVIDIA GeForce RTX 3090 (DiscreteGpu, Vulkan)`.
  `nvidia-smi` independently identified each instrumented process as a
  compute/graphics client on that device.
- Release build with `bench,smoke,dioxus-native/log-times`, actual app window
  1440×1000. `WGPU_ADAPTER_NAME` explicitly selected the NVIDIA device.
- Xvfb and D-Bus were private, with external networking disabled. These
  windows used the RTX 3090; Xvfb does not imply software GPU rendering.
  **Their presentation cost is not a physical-monitor latency measurement.**
- The original app process had exited before this investigation. The
  instrumented binary included concurrent native/streaming edits from
  other sessions. Its checksum and source-file fingerprints are recorded;
  comparisons of timestamp capture on/off used the same copied binary.

The real-library runs used a read-only SQLite backup of 8,466 games and
12,490 copied 96/320 px thumbnails (about 817 MiB). Discovery, launchers,
online metadata and hosting were disabled in the copy. For Home, copied
accounts were reduced to one anonymised, unlocked benchmark account so
normal startup would reach Home. Original accounts, settings, database and
artwork were not modified. Home with actual covers was visually verified
on the private display; that personal-library screenshot is not published.

## Results

CPU-side stages below are wall-clock durations from the existing frame log.
GPU commands use device timestamps, converted with the queue's timestamp
period. The GPU interval covers uploads and rendering recorded in the scene
command buffer. It excludes CPU scene construction, later surface blitting,
presentation and physical display scanout. GPU duration overlaps the CPU-side
stages: these columns are not additive.

| Real library, release | Resolve p50 | Scene commands p50 | Encode/submit p50 | GPU commands p50 / p95 | Present p50 | Completion wait p50 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Grid scrolling | 9.4 ms | 2.7 ms | 0.34 ms | **0.051 / 0.053 ms** | 44 ms | 0.047 ms |
| List scrolling | 8.0 ms | 4.5 ms | 0.32 ms | **0.035 / 0.037 ms** | 45 ms | 0.083 ms |
| Home, normal motion | 0.49 ms | 2.8 ms | 0.69 ms | **0.278 / 3.409 ms** | 44 ms | 0.138 ms |

Grid/list stage summaries cover the final 100 logged renderer frames of a
240-step traversal. Their redraw-interval p95 was **65.56 ms grid** and
**64.03 ms list**. Home covers seconds 25–45: 176 GPU samples, about 8.8
presented frames/second. The table uses that entire settled window, rather
than an arbitrary tail of the animation cycle.

Home used **43.99% of one CPU core** with motion on. Reduced motion brought
it to **0.19%**, with **zero frames during the settled 20-second interval**.
The reduced-motion run's nine startup GPU samples are excluded from idle
GPU timing. These are main-process CPU figures in this private presentation
environment, not the hour-long all-process acceptance budget.

The art-free fixture grid also used the hardware GPU: its last 100 device
timestamps were 0.052 ms median / 0.122 ms p95. Actual cover artwork did not
make GPU scene execution the limiting stage in the measured real-library
grid, although CPU layout, data loading and image decoding still matter.

## Controls and limitations

- **Timestamp overhead:** same instrumented binary, real grid, capture on
  versus off: redraw p95 **65.558 versus 65.562 ms**, total scroll **14.29
  versus 14.39 seconds**. No material overhead was visible at this granularity
  in this short pair; this is not a general claim of zero probe overhead.
- **Vsync:** before adding timestamps, the explicitly selected hardware
  GPU fixture grid measured 75.47 ms p95 with default presentation and
  61.14 ms with `ANYRENDER_PRESENT_MODE=immediate`. Median presentation
  still took about 45 ms in immediate mode. Vsync alone does not explain
  this private-window presentation cost. These baseline runs used the
  earlier binary, so they are not an instrumentation-overhead comparison.
- **Failed external probe:** the installed Mesa overlay layer returned
  all-zero GPU timings. Those values were discarded, not interpreted as
  zero GPU work. Direct wgpu timestamp queries returned valid nonzero data.
- **Home validity:** an initial copied-profile run showed the account
  chooser, and its reduced-motion smoke override was rejected because the
  copied settings already had a revision. Those Home results were discarded.
  The final copy is anonymised/unlocked and appearance controls are applied
  directly to the disposable database before startup. Original settings
  remain untouched.
- The machine was not otherwise idle, and the samples are short. They do
  not establish input-to-photon latency, physical-display frame pacing,
  or behaviour on every view. No synthetic input or measurement window
  reached the login desktop.

## What this changes

GPU scene execution is not the principal cost in these runs. The earlier
debug scene-command cost still warrants [#147](https://github.com/krazyjakee/caskade/issues/147).
The real-library motion control also strengthens
[#148](https://github.com/krazyjakee/caskade/issues/148): an idle view should
avoid repeatedly rebuilding/presenting unchanged content.

The large private-window presentation cost needs a separate physical-display
comparison before choosing a renderer or claiming the same stall occurs on
the user's monitor. Retain GPU rendering as the default; these measurements
do not justify changing it to CPU rendering. Acceptance remains in
[#111](https://github.com/krazyjakee/caskade/issues/111).

## Reproduce

The opt-in anyrender patch requests timestamp-query features only in a
native `log_frame_times` build when `ANYRENDER_GPU_TIMINGS=1`. It logs the
adapter and `gpu_frame: …ms` values. It adds two timestamp queries and
readback buffers per measured frame, uses the renderer's existing completion
poll, and never starts a sampler thread. Ordinary builds do not contain
this instrumentation; frame-timing builds leave queries disabled unless asked.

```sh
rtk proxy flock /tmp/caskade-heavy.lock cargo build --release -p caskade-native --features bench,smoke,dioxus-native/log-times
binary=/path/to/release/caskade-native
rtk proxy unshare -rn scripts/private-desktop.sh --size 1440x1000 -- python3 docs/spikes/native-render-2026-11-13/measure.py "$binary" grid /tmp/caskade-gpu-grid --width 1440 --height 1000 --renderer auto --adapter 'NVIDIA GeForce RTX 3090' --present-mode immediate --gpu-timestamps
```

For real artwork, add `--library /path/to/library.sqlite --artwork
/path/to/cache/artwork`; both sources are copied, with the isolation changes
above. `default` and `reduced` measure Home. Omitting `--gpu-timestamps`
provides the overhead control. The probe rejects missing/zero hardware
timestamps rather than reporting a successful GPU measurement.

[Raw valid samples, adapter evidence and stage summaries](native-render-2026-11-13/gpu-measurements.json)
are archived without the copied database, artwork, account names or personal
screenshots. The initial account-chooser controls are omitted.
