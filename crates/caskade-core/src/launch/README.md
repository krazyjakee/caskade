# Launch and session lifecycle (#108)

`LaunchService::new(LibraryStore)` exposes `launch(entry_id)`, `recover()` and
`shutdown()`. The Tauri adapter accepts a stored entry ID, never an arbitrary
process command. `launch` returns a `LaunchHandle` immediately; dropping the
handle keeps tracking active. Its `ticket` is the frontend response and `wait()`
is useful for tests/headless callers. Call `recover()` at startup before
accepting launches. Call `shutdown()` at app shutdown: it stops observation,
retains the open session and leaves the user's game running. Storage operations
use `try_game_lease(game_id)` on the shared application service and hold its
RAII guard across filesystem and database changes; persisted in-flight sessions
still need checking under that guard.

Managed executable/cwd paths are checked against the current storage root and
resolved through `GameEntry.alias_id` when present. Parent traversal, symlink
escapes and stale aliases are rejected. An absent alias falls back to the real
game directory. External launcher-owned entries retain their executable paths.
URI dispatch uses `xdg-open`, `open` or Windows `rundll32` with literal argv and
no shell. HTTP/file/script URIs are rejected. Flatpak targets use `flatpak run`.
Plugin runners are injected Rust implementations; an unregistered runner returns
Unsupported. Emulator execution is outside M0–M2.

Wine/Proton executable entries use their runner hint plus machine-local
`RunnerPaths` supplied through `with_runner_paths`. Defaults find the installer's
bootstrapped umu (or `umu-run` on PATH), select UMU-Proton, and use `wine` on PATH.
Managed entries never persist these host paths. Managed prefixes reuse the
installer's `PrefixRuntime` setup/wrapping after launch.before accepts the launch
and the SQLite session is reserved. Prefix symlink escapes are rejected.
External launcher prefixes use their existing drive mappings; Caskade does not
add G:, reinitialize or isolate them. OS process creation uses
`tokio::process`, with null stdin/stdout/stderr and the selected cwd/env.

Tracking polls sysinfo off Tokio worker threads. It recognizes real/alias paths,
Steam app-id environment hints and descendant processes. A snapshot from before
launch excludes unrelated existing processes. Launcher exit is independent from
game exit. A short direct executable is still observable through its child
handle even when it finishes between snapshots. Nonzero direct exits produce
`status=crashed`; external launcher game exit codes may be unavailable.

`game.started`/`game.exited` use `LaunchEvent` with game, entry and session IDs,
UTC epoch milliseconds, monotonic duration milliseconds and status. Hooks run
before spawning and after persisted completion. `launch.before` can rewrite
command/env or cancel. No environment, argv or process paths cross the event
bridge.

SQLite reserves one open session per game atomically. A pending reservation is
activated when a game is observed. Heartbeats persist a versioned observation
payload containing the target and PID/start-time identities. These absolute
runtime observations are recovery evidence, not managed installation locations
or writable capabilities; the stored entry remains root-relative. Recovery only
reattaches to recorded identities, never a reused PID alone. If the process is
gone, the session ends at its last heartbeat and unknown downtime is not billed.
If an exact identity survived, its downtime is known continuous playtime.
A crash before the first game identity was persisted can lose the short pending
launch window; Caskade fails conservatively rather than attaching another game.
Polling is approximate (default250ms plus a2s handoff grace); the grace is not
charged as extra playtime.

## Validation

```
flock /tmp/caskade-heavy.lock cargo test -p caskade-core launch
flock /tmp/caskade-heavy.lock cargo test -p caskade-core tracking
```

The default Linux suite launches the committed installer fixture game, follows
a forked child after its launcher exits, exercises stable aliases, duplicate
requests, crash detection and service restart, and persists sessions in SQLite.
A private-XDG test invokes the real Linux URI dispatcher for a `steam://` URI
with an authored fixture handler. It never changes the user's MIME association
or uses a Steam account; it is not evidence of a real Steam client lifecycle.

For the explicit real Steam check, select an already installed game, leave it
stopped, supply its app ID and absolute installation directory, and run:

```
CASKADE_TEST_NATIVE=1 CASKADE_CORE_STEAM_APP_ID=<id> \
CASKADE_CORE_STEAM_INSTALL_DIR=<absolute-dir> \
flock /tmp/caskade-heavy.lock cargo test -p caskade-core \
  launch::native_tests::native_steam_account_game_records_play_session \
  -- --ignored --exact --nocapture
```

Close that game normally to finish. This check writes only an in-memory Caskade
library and reports aggregate session/duration counts. It does not print the
selected game's ID, title, paths or account data. Windows/macOS native URI
behavior, protected-process visibility, fork/launcher parity and sleep/resume
clock behavior remain on issue22's app-core deferred verification list.

The private managed-prefix Wine fixture check needs Wine on the Linux host,
but uses no account or existing prefix:

```
CASKADE_TEST_NATIVE=1 flock --close /tmp/caskade-heavy.lock \
  cargo test -p caskade-core \
  launch::native_tests::native_wine_fixture_records_session_with_runtime_only_runner_path \
  --locked -- --ignored --exact --nocapture
```

`--close` prevents Wine's background server from inheriting the heavy-lane
lock. The test stops only its own private wineserver before deleting its prefix.
The real-account Steam test remains separate from this check.
