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

## Acceptance procedure and budget

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

The macOS empty-library target is at most 200 MiB physical footprint after
five settled minutes on Home (main process, no web-game/video subprocess),
and at most 16 MiB further growth between the second and tenth identical
navigation tours. These are acceptance targets, not a claim that the observed
build meets them. Keep first-run peaks separate. A whole-app result and
allocation-stack profile of navigation growth remain required before #156
can be considered fully resolved; the existing process was not restarted
with malloc stack logging.

## Next allocation-stack capture

The app-wide growth is still unresolved. After saving any edits, the user
can quit Caskade and launch the installed bundle from Terminal with:

```sh
MallocStackLogging=1 /Applications/Caskade.app/Contents/MacOS/caskade
```

Repeat the same navigation for a short run and leave that process open for
read-only `malloc_history`/memory-graph inspection. Stack logging adds its
own overhead, so use this run to identify live allocation stacks, not to
compare the physical-footprint budget. The repository reserves real-display
launches for the user; an agent must not launch this command on their behalf.
