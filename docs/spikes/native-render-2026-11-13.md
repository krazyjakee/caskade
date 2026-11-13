# Native rendering investigation, 13 Nov 2026

Measured master `d23dceda84b9a1822ca6a3ffdff37df2d2df9282` on an AMD Ryzen 9
5900X, Linux/X11/NVIDIA. The immediate leads are development-build paint cost,
idle backdrop repaints, and stale virtual-grid dimensions after resizing.
The managed streaming host has an independent CPU problem.

The [hardware GPU follow-up](native-gpu-2026-11-13.md) records explicit RTX
3090 selection, device-clock timestamps, real cached artwork and a capture
overhead control. It separates GPU execution from private-window presentation.

No application behaviour was changed. The running desktop app was only
observed: process/thread counters, logs, database game count and a screenshot
of its own window. All scripted input and additional windows used private
Xvfb and D-Bus instances. The disposable fixture runs had a network namespace
with no external network interface.

## Running app

A 20-second `/proc` sample, before compiling the instrumented binaries:

| Process | CPU (% of one core) | RSS (MiB) |
| --- | ---: | ---: |
| Native, debug, 8,466-game real library | 100.3 | 941.9 |
| Its managed Sunshine child | 100.4 | 348.9 |

The native main thread accounted for **98.5%**; Sunshine's
`confighttp::tcp` thread accounted for **99.9%**. Neither process had swap in
this sample. These RSS figures include mapped files and are not the spec's
anonymous-memory-plus-swap budget. The view was Library → Recently Played,
in a 1280×1400 window with 11 narrow card columns.

`perf` was unavailable (`perf_event_paranoid=4`, no passwordless sudo).
No system profiling setting was changed, and the live process was not
stopped or attached to a debugger. CPU percentages are tick deltas divided
by `SC_CLK_TCK` and monotonic elapsed time, not lifetime `ps %CPU`.

## Fixture controls

Both builds enabled `bench,smoke,dioxus-native/log-times`. Each run used
10,000 plain fixture games plus 29 console fixtures, a new XDG profile,
and no tray. Home used `vello_hybrid`. The Xvfb screen was 1440×1000, but
fresh app windows defaulted to **1280×800**. Screen size alone does not set
the app's window size. The selected graphics adapter was not recorded;
these results do not measure a real-display NVIDIA swapchain.

| Home | Debug CPU, final 10 s | Release CPU, seconds 25–45 |
| --- | ---: | ---: |
| Default motion and backdrop | 88.33% | 29.70% |
| Reduced motion, static backdrop retained | 0.40% | 0.24% |
| Hero timing changed to steps | — | 29.80% |
| Backdrop pseudo-elements removed | — | 0.24% |

Debug runs lasted 35 seconds. Early exploratory debug runs overlapped the
release compilation; repeat the A/B without compilation before selecting
development-profile overrides. Release runs followed compilation and
ran sequentially. The user's busy app and streaming host remained running
throughout, so this is an exploratory comparison on a working machine.

The hero timing control did not reduce Home's work: this Home component
does not mount the shared hero's `fx` and `glow` parts. The continuous
animation rules in the theme remain worth reviewing for other views, but
they do not explain this Home measurement. Removing the backdrop does.
The CSS used for both controls is preserved in the probe below.

Settled debug Home frames spent **170–298 ms building scene commands**;
release frames spent about **1.9–2.0 ms** there. Median document-resolution
time across the final 20 logged resolves was **5.7 ms debug**, **0.449 ms
release**. Median release renderer duration was **33 ms**, about **30 ms
in presentation**, with rendering itself below 1 ms. Resolve and renderer
timings describe different portions of a frame; neither includes all
event-loop and Dioxus work. The last 20 timings are a short settled tail,
not a full distribution or statistically controlled speedup estimate.

The fixture library lacks real cover artwork. Its memory use is not a
measurement of artwork caching or a substitute for the one-hour budget.

## Scrolling

The existing 240-step library benchmark uses redraw intervals, including
async page arrivals, rather than GPU timestamps or a frame-by-frame proof
that every newly exposed card has loaded. These runs use `vello_cpu` and
softbuffer. The first pair used the fresh 1280×800 window; the second pair
explicitly seeded `native-window.json` to obtain 1440×1000.

| Actual app window | Layout | p50 (ms) | p95 (ms) | Max (ms) | Most games mounted | Scroll seconds |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 1280×800 | Grid | 7.38 | 13.61 | 20.21 | 36 | 4.32 |
| 1280×800 | List | 12.53 | 17.25 | 20.99 | 24 | 4.51 |
| 1440×1000 | Grid | 10.20 | 16.90 | 20.65 | 49 | 4.62 |
| 1440×1000 | List | 15.22 | 20.04 | 27.18 | 28 | 4.92 |

The correctly sized release fixture runs are below the spec's 45/50 ms
grid/list p95 limits. This does not establish performance on the user's
artwork-heavy library, after resizing, or during real pointer/controller input.

An exploratory debug grid run was stopped before completing its traversal;
its last 20 renderer durations had a median of 794 ms. It produced no valid
scroll report and is not included as a completed benchmark. Renderer
durations must not be divided by release redraw intervals to claim a speedup.

## Resize reproduction

On a private 1920×1600 desktop, launch release with 100 plain fixture games,
reduced motion and the CPU renderer. Open Library → Recently Played at
1440×1000, then resize the same window to 1000×1000 without navigating.

Five fixture games initially occupy five of six tracks at about 178 px per
card. After shrinking, all five remain in one row at about 105 px per card.
The old six-track template survives; current width should yield three
tracks and two rows. The shell itself adapts to the new width.

[Before resizing](native-render-2026-11-13/resize-before.png) ·
[After resizing](native-render-2026-11-13/resize-after.png)

`VirtualLibrary` reads geometry when Dioxus renders it and subscribes to
mount/scroll/data changes, but has no resize subscription. Its inline
`repeat({columns}, minmax(0, 1fr))` template consequently stays stale while
Blitz relays it out. `ShelfRow` also measures at mount/scroll and warrants
the same review. Extra columns mount extra cards, but their share of the
live CPU cost has not been quantified.

## Issues and first work

1. [#146: refresh virtual-grid dimensions after resizing](https://github.com/krazyjakee/caskade/issues/146).
   The smallest confirmed application fix: observe geometry after resize
   layout and update the virtual window without idle polling.
2. [#147: optimize development-build paint dependencies](https://github.com/krazyjakee/caskade/issues/147).
   Attribute the expensive scene-command work, then benchmark targeted
   `profile.dev.package` overrides and their rebuild-time tradeoff.
3. [#148: reduce animated-backdrop idle work](https://github.com/krazyjakee/caskade/issues/148).
   Compare static/default cadence and cached/composited rendering on a real
   display; keep resolve, scene construction, render and present separate.
4. [#145: managed streaming host HTTP thread spins](https://github.com/krazyjakee/caskade/issues/145).
   Reproduce independently and establish whether the host or its client
   interaction causes the spin. Count it in the all-process budget.

[#111](https://github.com/krazyjakee/caskade/issues/111) remains the acceptance
tracker; [#71](https://github.com/krazyjakee/caskade/issues/71) already tracks
large-library memory. These short runs do not close either.

## Repeating the probe

[measure.py](native-render-2026-11-13/measure.py) archives the short protocol;
[measurements.json](native-render-2026-11-13/measurements.json) contains the
valid original samples, scroll reports and phase summaries. Invalid early
custom-CSS controls were discarded. The script explicitly sets window size,
guards the desktop, checks for external network interfaces, refuses an
existing output directory, and stops only its own child by PID.

```sh
rtk proxy flock /tmp/caskade-heavy.lock cargo build -p caskade-native --release --features bench,smoke,dioxus-native/log-times
# Use the binary from your configured Cargo target directory.
binary=/path/to/release/caskade-native
rtk proxy unshare -rn scripts/private-desktop.sh --size 1440x1000 -- python3 docs/spikes/native-render-2026-11-13/measure.py "$binary" default /tmp/caskade-home-default
rtk proxy unshare -rn scripts/private-desktop.sh --size 1440x1000 -- python3 docs/spikes/native-render-2026-11-13/measure.py "$binary" reduced /tmp/caskade-home-reduced
rtk proxy unshare -rn scripts/private-desktop.sh --size 1440x1000 -- python3 docs/spikes/native-render-2026-11-13/measure.py "$binary" grid /tmp/caskade-grid --width 1440 --height 1000
```

Other modes are `list`, `stepped` and `flat`. Build without `--release` for
debug, keep actual dimensions identical across A/B runs, and run without
concurrent compilation. Home samples for 45 seconds and reports the last
20 seconds; scrolling exits when the benchmark finishes (300-second bound).
The probe samples only its main child; use `scripts/idle-bench/` for the
all-process acceptance measurement.

The archived probe was verified with a complete 1440×1000 release grid run
(17.46 ms p95, 49 games mounted) and a reduced-motion Home run (0.238% CPU).
An invocation outside the desktop wrapper was rejected before creating an
output profile or starting the app. Both verification records are in the JSON.
