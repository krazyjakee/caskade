# macOS memory investigation, 15 November 2026 (#156)

The reported 510 MB reading has no build, screen, elapsed time or task state.
The reporter later associated it with first-run setup and reported memory
increasing while navigating throughout the app. It must not be treated as a
measured leak or a steady-state baseline.

## Read-only observation of the installed app

Measured the user's already-running process; no app was started, no input
was sent, and no profile or permissions were changed. The user continued
using the app. These are observations, not a controlled before/after test.

- Caskade 0.2.0, installed macOS x86-64 bundle; exact binary commit unknown.
- macOS 13.7.8 (22H730), MacBookPro14,3, Intel i7-7820HQ, 16 GiB RAM.
- Process launched at 09:28:50 UTC. GPU resources were present in the
  profile; the selected renderer/adapter and each active screen were not
  recorded. A later read-only SQL count found one library game; task completion at each
  sample was not independently established. The screenshot shows Home with no recently played sessions.
- Metric: `vmmap -summary <pid>` physical footprint, including compressed
  memory. RSS, virtual size and Activity Monitor's Memory column are not
  interchangeable with this metric. `footprint -p <pid>` corroborated the
  first two readings.

| UTC | Time since launch | Physical footprint | Peak |
| --- | --- | --- | --- |
| 09:30:12 | 82 s | 72.2 MiB | 76.2 MiB |
| 09:32:59 | 249 s | 168.2 MiB | 174.2 MiB |
| 09:33:42 | 292 s | 178.1 MiB | 179.8 MiB |
| 09:37:44 | 534 s | 219.2 MiB | 221.8 MiB |
| 09:52:40 | 1430 s | 295.9 MiB | 297.4 MiB |

The allocator's live bytes grew from 57.3 to 196.7 MiB, and then 269.9 MiB, with about 10.5 MiB of dirty/swapped fragmentation
in the final sample. `heap -sortBySize`
reported about 195 MB in non-Objective-C allocations and 8.5 MB in Metal
resource lists. `leaks` at 09:33 found 540 KiB unreachable, far less than the
observed growth. A later `leaks --noContent --groupByType --referenceTree`
graph attributes about 209 MiB to descendants of the AMD Metal device
(`BronzeMtlDevice`). Raw non-Objective-C malloc blocks include driver-owned
allocations: they are not proof of retained Rust page objects. The graph is
a strong lead for driver/resource retention, not an allocation-stack diagnosis.
A two-second stack sample found the main thread idle and background workers
mostly waiting. Adding allocator trimming alone would not explain this result.

## Changes and regression checks

Steam discovery previously read its entire `appinfo.vdf` (up to 256 MiB)
into a vector before selectively parsing installed apps. It now seeks over
unwanted records and limits the compacted selected input, including v29's
key table, to 16 MiB. Decoder budgets and span checks still apply; failures
fall back to the tool list. This removes a first-run allocation candidate;
it is not evidence that Steam caused the reported 510 MB or navigation growth.

`parsers::streamed_appinfo_does_not_read_unwanted_large_records` creates a
sparse 192 MiB record and asserts exactly 20 bytes read. Both format versions
are compared with the existing decoder, including truncation and budget tests.
The ignored `parsers::appinfo_memory_probe` compares the old buffered path
with the new reader in separate processes under `/usr/bin/time -l`. On this
Mac, the same sparse 192 MiB fixture measured 195.0 MiB maximum RSS for the
buffered path and 3.3 MiB for the streamed reader (development test binary).
These are parser-process measurements, not whole-app before/after figures.
The headless frame's repeated-navigation test checks the retained DOM slab
after successive tours, including Home, Library, Collections and Settings.
It passed on this Mac with exactly 653 live nodes after each of four tours.
This bounds detached page retention in that fixture; it does not exercise
the GPU renderer.

The ignored `apps/native/tests/metal_memory.rs` test renders 512 offscreen
frames with Vello on the AMD Radeon Pro 560, using Metal with no window.
Physical footprint was 6.1 MiB at frames 128 and 512. An experimental pool
drained after each frame also measured 6.1 MiB, so that workaround was not
adopted. This small workload does not reproduce the app's growth and does
not rule out more complex renderer resources or driver caches.

## Layered Metal reproduction and adapter mitigation

A richer offscreen fixture paints a Blitz/Dioxus document through the app's
Vello Hybrid scene painter: text in the bundled Inter font, linear gradients,
rounded clipped cards, shadows and opacity layers. It reproduces the growth
without navigation or a window. A simpler scene of 32 translucent rectangles
also reproduces it; plain solid rectangles did not. The document is reused,
scene storage is reset each frame, and every submission is followed by a
blocking device poll.

| Workload / adapter | Frame 32 | Frame 128 | Frame 512 | Frame 2048 |
| --- | --- | --- | --- | --- |
| Translucent rectangles / AMD Radeon Pro 560 | 6.9 MiB | 7.7 MiB | 11.5 MiB | 26.9 MiB |
| Same / Intel HD Graphics 630 | 38.1 MiB | 38.4 MiB | 38.5 MiB | 38.6 MiB |
| Rich document / AMD, pool drained per frame | 10.0 MiB | 11.4 MiB | 17.2 MiB | 40.4 MiB |
| Rich document / Intel | 51.6 MiB | 51.8 MiB | 50.3 MiB | 47.6 MiB |

All figures are `vmmap` physical footprint from development test processes on
the hardware above, without stack logging. The richer fixture had the same
AMD footprint at frame 512 with and without a per-frame autorelease pool.
Disabling wgpu debug/validation flags did not stop the translucent-rectangle
growth (19.3 MiB from frames 128 to 2048).

A separate logged rich-document run captured `malloc_history -allBySize`.
The growing live allocation groups include 736 allocations each in
`amdMtlBronzeResourceMgrAlloc` / `amdMtlBronzeAllocThreadGroupLengthMgr`,
called from `BronzeMtlCmdBuffer renderCommandEncoderWithDescriptor:` through
wgpu-hal's Metal render-pass creation. This independently reproduces driver
resource retention consistent with the earlier app graph; it does not prove
that every allocation in the installed app has that cause.

The macOS GPU-backed renderer defaults to the low-power adapter, retaining explicit
`WGPU_ADAPTER_NAME` and `WGPU_POWER_PREF` overrides. This selects Intel on the
reported dual-GPU Mac and avoids the reproduced AMD path. Other platforms keep
their existing selection policy. This is an adapter mitigation, not a driver
repair or a solution proven on AMD-only Macs. The ignored Metal regression
probe now uses the app's `DeviceHandle` selection and 4,096 frames with opacity
layers; `WGPU_POWER_PREF=high` selects the affected AMD path for comparison.
The final Cargo-built probe selected Intel through that production policy:
45.5 MiB at frames 32 and 128, then 46.1 MiB at frames 1,024 and 4,096,
passing its 16 MiB post-warm-up growth limit. It creates no desktop resources.
The same binary with `WGPU_POWER_PREF=high` selected AMD and failed that check:
15.1 MiB at frame 128 and 54.9 MiB at frame 4,096 (39.8 MiB growth).

## Real-window measurements and CPU selection

The user subsequently explicitly authorized starting and stopping the app on
this desktop for this investigation. This permission applies to this run;
the repository's normal private-desktop rule is unchanged.

The patched development executable (adapter mitigation from a8ac50e7) opened
the existing one-game library on this Mac. The window was maximized at
1,680 × 954 logical pixels, scale 2 (3,360 × 1,908 surface pixels), using the
default Nexus theme, completed setup and the existing discovery settings.
These measurements are not an empty-profile release-build acceptance result.

A logged run (PID 34002, launched 18:15:30 UTC) confirmed Intel driver
allocations. From the initial to the later idle allocation snapshot, AMD
allocation groups stayed at 43 allocations / 23,188 bytes; Intel groups grew
from 173,478,135 to 175,874,511 bytes. Those totals include anonymous VM;
they are not interchangeable with physical footprint. Stack logging added
64.4 MiB of “Performance tool data” by the later snapshot. The macOS app scan
timed out in that instrumented run, so it is not a setup speed measurement.

Without stack logging (PID 34784, launched 18:22:42 UTC), Home held 312.5 MiB
physical footprint at 47 and 60 seconds and 310.4 MiB at 300 seconds, with a
444.9 MiB startup peak. At 47 seconds, the malloc zone held 33.6 MiB of live
allocations; IOAccelerator regions had 275.1 MiB resident, and IOSurface
regions another 49.4 MiB. These categories do not add directly to footprint,
but GPU/display resources dominate this baseline.

A separate GPU navigation run (PID 35639) completed one full tour of Library,
Collections, Store and all 17 Settings sections, returning Home at 494.2 MiB
(518.4 MiB peak). A second tour stopped on a spatial-focus failure in the
controller script. It is not a completed ten-tour result, and one warming
tour does not establish continuing growth. The controller report recorded
zero real keyboard or pointer events.

With `CASKADE_RENDERER=cpu` on the same build and window (PID 36250), footprint
was 93.6 MiB at 30.7 seconds and 94.5 MiB at 60 seconds after visiting Library
and Collections; the peak was 160.5 MiB. That script stopped on the same
spatial-focus issue before Store. This is a renderer comparison, not the
final navigation acceptance run.

The integrated GPU avoids the reproduced AMD retention but still has too
large a resource baseline for the experimental 200 MiB target here. Commit
642904f4 temporarily made Intel macOS builds use vello_cpu automatically.
That policy was subsequently removed at the user’s request (decision below). CPU rendering already
pauses ambient loops through `data-paint="cpu"`. The existing e2e driver now
supports `focus-href` (focus the app link, then use normal controller confirm)
and `wait-paint`, so exact navigation and the selected renderer can be checked
without depending on spatial geometry or OS input permissions.

## Automatic-CPU experiment (subsequently reverted)

The rebuilt development executable was launched at 18:48:55 UTC (PID 37970)
with both `CASKADE_RENDERER` and `MallocStackLogging` unset. Its SHA-256 was
`27238dfe5e41f9ddaa437ef5bd34a155c4ddea6b87fe8aad9a561bae2ece8438`.
The run used the same maximized Retina window, one-game profile, admin account
and completed setup. Discovery and online lookups were enabled. The unbundled
development executable reported a missing 7-Zip sidecar (also present in the
comparison runs), and sync waited for `KeychainUnavailable`; this is not a
validation of those integrations. Clippy ran during the initial settling
period and finished before the five-minute snapshot and all navigation tours.

The generated script first confirmed `data-paint="cpu"`, settled on Home for
five minutes, then completed ten identical tours through Library, Collections,
Store and all 17 Settings sections. It executed 517 steps, including 170
Settings section switches, and returned a successful report with
`cpuRenderer=true`, zero real keyboard events and zero real pointer events.
The test instance quit cleanly; its window and navigation preferences were
restored afterward. No installed app bundle was replaced.

The startup samples below use seconds since process launch. Each tour sample
was taken three seconds into its ten-second pause back on Home.

| Sample | Elapsed seconds | Physical footprint | Process peak |
| --- | --- | --- | --- |
| start-30 | 30.1 | 93.2 MiB | 167.2 MiB |
| start-60 | 60.1 | 92.9 MiB | 167.2 MiB |
| start-300 | 300.1 | 117.4 MiB | 167.2 MiB |
| tour-1 | 349.9 | 114.7 MiB | 190.8 MiB |
| tour-2 | 384.3 | 139.4 MiB | 190.8 MiB |
| tour-3 | 417.8 | 135.4 MiB | 190.8 MiB |
| tour-4 | 451.4 | 115.0 MiB | 200.3 MiB |
| tour-5 | 485.7 | 142.0 MiB | 211.9 MiB |
| tour-6 | 519.6 | 129.8 MiB | 211.9 MiB |
| tour-7 | 554.6 | 141.0 MiB | 211.9 MiB |
| tour-8 | 588.2 | 141.1 MiB | 211.9 MiB |
| tour-9 | 622.1 | 140.5 MiB | 211.9 MiB |
| tour-10 | 655.5 | 135.8 MiB | 211.9 MiB |

Home at 300 seconds after launch was 117.4 MiB, compared with the GPU
run's 310.4 MiB.
Home after tour ten was 135.8 MiB, 3.6 MiB below tour two. The largest Home
sample after warming was 142.0 MiB; the highest increase over tour two was
2.6 MiB. Transient navigation raised the process peak to 211.9 MiB. The
original settled-Home and post-warm-up comparison targets were met by this
experimental CPU configuration and profile. An isolated empty-profile release run, Apple Silicon and
other Intel Mac models remain unmeasured; these results do not claim them.

## Renderer decision after the measurements

The user prefers higher RAM use over CPU rendering. Automatic GPU rendering
is restored on Intel macOS, and `CASKADE_RENDERER=cpu` remains an explicit
opt-in alongside the existing fallback when no usable GPU is available.
The macOS low-power GPU preference and the setup, SVG-icon and AppKit editing
fixes remain. The CPU measurements above are comparison data, not the shipped
default's acceptance result or a basis for silently choosing CPU again.

The restored default was rebuilt and checked with `CASKADE_RENDERER` unset:
the Home smoke script passed `wait-paint gpu`, reported `cpuRenderer=false`
and no real keyboard or pointer events, and quit cleanly. Build and native
Clippy for all targets passed. This verifies the selected renderer, not a new
GPU memory-budget or ten-tour drift result.

The 200 MiB figure below is retained as the original experimental comparison
target. A larger GPU baseline is accepted; unbounded growth remains a separate
bug to investigate. A GPU baseline budget needs to account for window size,
scale and driver resources. The post-warm-up drift check remains useful.

## Acceptance procedure and comparison targets

Use a release build with its commit recorded, a separate empty profile,
fixed window size and renderer, no online catalogue artwork, and record
which first-run tasks are active. The user launches the macOS app manually;
an agent must not start a native window on the developer's desktop. Attach
read-only `vmmap -summary <pid>`, `footprint -p <pid>` and `heap -sortBySize
<pid>` observations to that explicit PID.

Measure at 5, 30, 60 and 300 seconds, then after every full navigation tour.
Measure setup with discovery on separately from a completed empty profile
with discovery and online lookups off. Warm each screen once, then repeat
identical tours at least ten times. Record each screen, task state and library
count, including before/after granting access in System Settings.

The original comparison target was at most 200 MiB physical footprint after
five settled minutes on Home (main process, no web-game/video subprocess).
That figure does not select the renderer. The provisional drift target is
at most 16 MiB further growth between the second and tenth identical
navigation tours. Keep startup and transient navigation peaks separate from
settled Home. The real-window sections above record the user-authorized run
and allocation-stack capture, including the passing numerical checks on the
one-game development profile and the remaining release/empty-profile checks.
The original installed process was not restarted with stack logging; the
rebuilt development executable was profiled separately.

## Next allocation-stack capture

The CPU experiment met the comparison targets on this profile. GPU rendering
is restored as the default; its whole-app drift and an empty-profile release
check remain separate validation work. The original
installed 0.2.0 bundle does not contain these fixes.
If the patched app still grows, after saving any edits the user can quit it
and launch that patched binary from Terminal with allocation logging. For a
patched bundle installed at the usual path:

```sh
MallocStackLogging=1 /Applications/Caskade.app/Contents/MacOS/caskade
```

Repeat the same navigation for a short run and leave that process open for
read-only `malloc_history`/memory-graph inspection. Stack logging adds its
own overhead, so use this run to identify live allocation stacks, not to
compare the physical-footprint budget. The repository reserves real-display
launches for the user; an agent must not launch this command on their behalf.
