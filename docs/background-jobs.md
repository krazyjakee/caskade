# Background work

`caskade-jobs` owns the process-wide FIFO resource queue. CPU and disk stages
share one slot across services, rather than each service starting its own pool.
This deliberately conservative limit also works on single-core hosts. A scan
uses at most two source workers (one on a single-core host); installer header
inspection is similarly capped, and plugin compilation disables Wasmtime's
parallel compilation pool.
The 7-Zip sidecar is capped at two decoder threads, reserving a core when possible.

Queued stages include library scans and watched rescans, ROM scanning, emulator
detection, artwork downloads and decoding, installer input inspection and
execution, verification, storage reconciliation, moves, source cleanup and
plugin compilation/execution. Installer downloads retain their existing
cancellable download queue and live concurrency preferences. Game processes,
input listeners, process monitoring, and small UI asset reads remain responsive
independently of the resource queue.

For new work:

- Use `caskade_jobs::blocking(label, closure).await` for CPU/disk stages. It waits
  for its slot before asking Tokio for a blocking thread.
- Use `global().run(label, future).await` for bounded async work, such as a plugin
  call. Nested stages reuse the same slot.
- Use `global().sync(label, closure)` on dedicated watcher threads. Use
  `caskade_jobs::inherit` when a queued stage starts child source threads, so
  they retain its slot after the caller's timeout.
- Use `inherit_async` for child tasks that are part of the same stage. Hook
  timeouts start after queue admission; waiting for resources must never count
  as a plugin failure. Detached plugin calls retain their parent's slot until
  the host actually finishes them, even after the bus stops waiting.
  Cold backend preparation has a separate 30-second budget before the call's
  execution timer starts. Plugin calls, activation and reload take their
  resource slot before the instance mutex to keep a consistent lock order.
- Retain an `activity(label)` guard for the complete lifetime of an orchestration
  or an operation already limited by a download queue. This reports work between
  heavy stages; it does **not** grant a resource slot.

Keep labels short, e.g. “Scanning library” or “Updating metadata”. Drop activity
guards on success, failure or cancellation, and stop reporting once a task waits
for user input. RAII removes cancelled waiters; a blocking worker keeps its slot
until it actually finishes, even if its async caller has been aborted. Take
cross-stage file locks inside a `run` scope, as artwork downloads do, to avoid
holding a file lock while another stage waits for that same lock in the queue.

Install and update jobs that fail carry a classified failure (cause, stage,
file, redacted details, recovery actions; #158). Only transient failures retry
on their own, a bounded number of times with growing delays; the rest wait for
the user in Activity. Interrupted installs are rolled back at startup. See
`crates/caskade-installer/README.md`, "Failures and recovery".

The Tauri bridge coalesces bursts into at most ten revisioned snapshots per second
and serves an initial snapshot
with the application version. The persistent status bar subscribes before
loading that snapshot, ignores older revisions, and periodically reconciles to
recover from missed events. The Activity job feed also covers installer stages
and tasks waiting for user input. The version stays visible when idle, busy or
disconnected; the IGDB and Hasheous credits are in Settings → About.
