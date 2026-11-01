# Errors and recovery

How Caskade reports failures and recovers from them (#160): the shared error
model, the recovery policy, guidelines for new code, and the audit inventory
of failure paths with their status. Installs and updates have their own
classified model (#158, `crates/caskade-installer/src/failure.rs`, see
`crates/caskade-installer/README.md`, "Failures and recovery"); this document
builds on it rather than replacing it.

## The error model

`caskade_core::error::ErrorReport` is what crosses to the webview when a
command fails, and what new code should build when a failure reaches people:

| Field | What it is |
| --- | --- |
| `code` | An `ErrorCode`, stable: the webview, notifications and recovery code match on it, never on the text. |
| `summary` | One plain sentence: what failed and, when known, why. |
| `detail` | The error and its causes, redacted; `null` when the summary says it all. Shown behind Show details. |
| `hints` | `RecoveryHint`s, most useful first: what the user (or Caskade) can do. |
| `transient` | Retrying unchanged may work (the network, a file in use). |
| `reference` | Eight hex characters, logged with the detail, so a report can be found in the log. |

`ErrorReport::from_error` walks an error's causes for the core's own errors
(`Error`, `LaunchError`, `SettingsError`, `StorageError`, `RommError`), I/O
and SQLite errors, and picks the code. Text the error wrote for people stays
the summary; an OS or database message ("No such file or directory (os error
2)") becomes the detail under a plain sentence for the code.

Codes: `invalid`, `forbidden`, `conflict`, `cancelled`, `refused`,
`missingFile`, `accessDenied`, `diskFull`, `storageOffline`, `busy`,
`offline`, `server`, `auth`, `integrity`, `corrupt`, `database`,
`unavailable`, `internal`, `other`.

Hints: `retry`, `retryLater`, `checkConnection`, `reconnectStorage`,
`freeSpace`, `checkPermissions`, `checkCredentials`, `signIn`, `reload`,
`restart`, `updateCaskade`.

Redaction (`caskade_core::error::sanitize`) runs on every summary and detail:
the installer's URL redaction (credentials, query and fragment go), then
`token=`, `password=`, `"secret": "…"`, `Bearer …` and similar values become
`<redacted>`.

### Across the boundary

- **Adapter**: `apps/desktop/src-tauri/src/error.rs`. A command returns
  `Result<T, CommandError>` (`CommandError` is `ErrorReport`) and maps errors
  with `.map_err(report)`, which also logs the report with its reference (at
  `debug` for `invalid`/`forbidden`/`cancelled`/`conflict`, `warn`
  otherwise). `?` takes a `caskade_core::Error` or an adapter's own `String`.
  The permission gate refuses with a `forbidden` report. Library, launch,
  storage, collection and game-update commands use it; the rest still return
  strings and move over as they are touched (see the inventory).
- **Webview**: `apps/desktop/src/api/reply.ts`. `unwrap` rejects with a
  `CommandFailure` (an `Error` whose `message` is the summary, plus `code`,
  `detail`, `hints`, `transient`, `reference`). `toFailure(error)` turns
  anything thrown into one; it also explains Tauri's "state not managed"
  answer (a part of the core that didn't start) as `unavailable`, the one
  place the webview reads a message's text.
- **Display**: `ErrorNotice` in `@caskade/ui` (anatomy root `error-notice`)
  shows a title, the summary, the hints in words, and a Show details button
  (keyboard and pad reachable) for the redacted detail and reference.
  `components/FailureNotice.tsx` renders any error through it, with an
  optional Try again that can be a view's first pad stop. Settings panels
  (`SettingsFeedback`, `SettingsLoading`), the Library, the Web tab and the
  account gate use it.
- **Jobs**: installs and updates keep #158's `Failure` (kind, stage, file,
  security note, recovery actions) and Activity's JobRecovery UI; torrents
  (#159) share it. Persistent failures go to the notification feed (#198),
  never to repeated toasts.

## Recovery policy

Self-heal by default; ask only when recovery needs a decision.

| Policy | When | Examples |
| --- | --- | --- |
| Automatic | Safe, bounded and inside what the user already started. Progress shows unobtrusively; a diagnostic record stays in the log. | Retry transient network or I/O failures with backoff and `Retry-After`; reconnect; resume verified downloads; rebuild disposable caches (IGDB answers, Hasheous index, compiled plugins); recreate app-owned folders; skip an unreadable record instead of failing a whole list; fall back to the default theme. |
| Confirm | Recovery crosses an authorization already given, or its effect is uncertain. Explain the action and its consequences; allow cancel; don't ask twice for the same scope. | Locate a moved source; replace user-modified files; reset a damaged settings section (keeping a backup); rerun an operation with external side effects; delete user data. |
| Manual | Only the user can fix it. Say what to do. | Credentials; reconnecting a drive; freeing space; a security product's quarantine; data written by a newer Caskade. |

Bounds that already exist and must be reused, not duplicated:

- Installs and updates: two automatic retries for transient failures (15 s,
  60 s), then they wait in Activity; interrupted installs roll back at start
  (`install/recovery.rs`).
- IGDB: a failure pauses every request for a minute or the server's
  `Retry-After` (up to 6 h); unanswered ids wait 6 h; cached answers serve
  while offline.
- Metadata runs: 10 min, doubling with ±20 % jitter to 1 h offline or 6 h
  when the server answers unusably; a run stops at the first server failure.
- Game and emulator update checks: a failed check leaves that install alone
  for 30 min and keeps the last answer.
- RomM: background backoff; offline servers mark entries broken with a
  reason; a suspicious mass removal is held back; downloads resume from
  `.part`.
- Hasheous: 15 min after a failed sync, a recheck every 6 h, the stale listing
  while offline.

Rules for recovery code:

- **Bound it**: a fixed number of attempts or a capped, jittered backoff, and
  one layer owns the retry (the caller of a retrying client must not retry
  too). Never loop on a permanent failure: classify first (`transient`).
- **Respect cancellation** and stop when the user cancels or the app quits;
  resume across restarts where the task is durable (installs, moves).
- **Reconcile before retrying** so a retry can't duplicate a side effect, and
  never report success until the result is verified.
- **Keep the original error** when remediation fails, and say both.
- **Never bypass security**: integrity failures, quarantines, consent and
  permission refusals are reported, not worked around, and blocked content is
  not fetched again on its own.

## Writing errors

- Say **what failed**, the **resource** (file name, folder, host, game,
  plugin; never a full URL or a secret), the **cause** when known, and the
  **effect** on what the user was doing: "The game's folder “/games/Celeste”
  is missing. It may have been moved, deleted or on a drive that isn't
  connected."
- Distinguish a confirmed cause from a possible one ("Security software can
  cause this, but Caskade can't tell for sure").
- Say what Caskade is already doing about it, and the next step if that
  fails. Don't tell people to read logs for the basic explanation.
- Plain words, no "Error:", "failed to", "invalid launch", "something went
  wrong" or raw `Debug` output. Wrap OS errors with what Caskade was doing
  (`io::Error::new(kind, format!("couldn't start “{program}”: {error}"))`):
  `ErrorReport` treats a message Caskade wrote as the summary, and an OS
  message as detail.
- Keep the kind: don't stringify an I/O, network or database error into
  `Error::Invalid` (it reads as the user's mistake and loses the code); add
  context with `io::Error::new(error.kind(), …)` or a typed variant.
- Don't swallow: no `map_err(|_| …)` that drops the cause, no `let _ =` or
  `.ok()` on a write, send or emit whose failure changes what the user sees,
  no `unwrap_or_default()` that turns a read error into "nothing there". If
  ignoring is right, say why in a comment and log it.
- Match on codes and types, never on display text.
- One notice per failure: inline next to the control, a block in place of a
  view, a job's recovery in Activity, or a notification for background work.
  No repeated toasts.

## Audit inventory

Status: **Fixed** in #160; **Existing** already handled before it; **Follow-up**
tracked here with its rationale. Policy: A automatic, C confirm, M manual.
Paths are relative to the crate or app named in the area.

### Boundary and presentation

| Area | Where | Was | Gap | Recovery / display | Policy | Status |
| --- | --- | --- | --- | --- | --- | --- |
| Commands | `src-tauri/src/commands/*` | `Result<T, String>` via `error.to_string()` (~170 commands) | No code, hints or detail; OS text as the message | `CommandError` (`ErrorReport`) + `report()`; library, launch, storage, collections, updates converted | – | Fixed (5 modules); Follow-up for the rest (mechanical, as touched) |
| Permission gate | `src-tauri/src/permissions.rs` | Refusal as a bare string | No code to tell "not allowed" apart | `forbidden` report with `signIn` hint | M | Fixed |
| Webview unwrap | `src/api/reply.ts`, `igdb.ts`, `updates.ts` | `new Error(message)`, three copies | Code and details lost | `CommandFailure` + `toFailure`, one copy | – | Fixed |
| Missing state | Tauri's "state not managed for field …" | Raw text shown | Unreadable; a subsystem failed at start | `toFailure` explains it as `unavailable` | M (restart) | Fixed (display); Follow-up: startup health record |
| Error display | ~45 `<p role="alert">` copies; `SettingsFeedback`/`SettingsLoading` | Message only, or "Settings could not be loaded." | No details, no consistent retry | `ErrorNotice` / `FailureNotice`; Settings (21 panels), Library, Web tab, account gate | – | Fixed (shared + main sites); Follow-up for remaining inline copies |
| Account gate | `src/shell/accounts/AccountGate.tsx` | A failed accounts load opened the shell | Every change then refused | Error with Try again (pad first stop, window controls) | M | Fixed |
| Console toggle | `src/shell/console/mode.tsx` | Shell switched before the window; failure only logged | Console shell over a desktop window | Reverts when the window didn't switch | A | Fixed |
| Library | `src/views/library/LibraryView.tsx` | Web tab showed "No web games yet" on error | Error looked empty | `FailureNotice` with Try again | A/M | Fixed |
| Storage | `src/views/settings/StoragePanel.tsx` | "0 registered locations" and "Add a folder…" on error; a batch move stopped at the first failure without saying what moved | Misleading empty state; partial success hidden | Error-aware count and empty state; "N of M moved; “Game” wasn't moved: …" | M | Fixed |
| Crash screen | `src/shell/ErrorFallback.tsx` (`AppCrash`) | Rendered outside `InputProvider` | A pad can't press Reload | Pad-reachable Reload, Open log folder | M | Follow-up |
| Guide button | `src/input/GuideMenu.tsx:77` | `running()` failure only logged | Guide does nothing for a pad user | Open the menu with the error | M | Follow-up |
| Plugins (webview) | `src/plugins/PluginProvider.tsx`, `PluginViews.tsx`, `loader.ts` | Load and backend start failures to the console | Plugins silently missing | `registry.notify(…, "error")` | A | Follow-up |
| Emulator setup cancel | `views/settings/emulators/EmulatorSetup.tsx:50` | Cancel failure to the console | User thinks it cancelled | Inline error | M | Follow-up |
| Onboarding | `src/onboarding/run.ts:171`, `OnboardingGate.tsx` | A refused scan can leave "Looking…"; a settings error skips setup | Possible endless wait; setup bypassed | Fail the task with its message; error with Retry | A/M | Follow-up |
| Theme switch | `src/appearance/AppearanceRoot.tsx:50` | `console.warn` | Picker shows a theme that isn't applied | Notice and reset the choice | A | Follow-up |
| Drops | `src/drop/DropProvider.tsx:80` | Listener registration error swallowed | Drag and drop silently off | Log; health record | – | Follow-up |
| Home, game sessions, collections, move dialog | `HomeView.tsx`, `GameView.tsx`, `CollectionView.tsx`, `MoveGameAction.tsx` | "Could not load …" without message or retry | No cause, no retry | `FailureNotice` with Try again | M | Follow-up |
| Install ingress | `views/activity/InstallIngress.tsx` | One error slot for several drops | Errors overwrite each other | Per-request errors (install confirmation UI is being changed elsewhere) | – | Follow-up |

### Startup, shutdown and logging

| Area | Where | Was | Gap | Recovery | Policy | Status |
| --- | --- | --- | --- | --- | --- | --- |
| Logs | `src-tauri/src/lib.rs:32` | `tracing` to stdout only; `paths.logs` created but unused | A release build keeps no record, so references can't be looked up | Rolling file log in `paths.logs` (needs `tracing-appender`), "Open log folder" in Settings → About | A | Follow-up (high) |
| Degraded subsystems | `lib.rs` (storage, installs, RomM, plugins, catalog, artwork, web player) | Start failure only logged; state not managed | Commands fail with Tauri's text; panels look empty | Startup health record + status bar badge + panel notices | M | Follow-up (high) |
| Data folders | `crates/caskade-core/src/paths.rs` | `create_dir_all` error without the folder | Startup dialog didn't say which folder | Folder named | M | Fixed |
| Startup failures | `lib.rs` `startup_failed` | `eprintln!` then exit | Nothing visible from a desktop launch | Native message box | M | Follow-up |
| Close in console mode | `commands/console.rs` | `let _ = window.close()` | A failed close left the app running | Exits, as the tray's Quit does | A | Fixed |
| Launch recovery at start | `lib.rs:154`, `launch/service/watch.rs` `recover()` | Stops at the first session that fails | Other sessions not re-attached | Per-session handling + a notice | A | Follow-up |

### Core: library, settings, storage

| Area | Where | Was | Gap | Recovery | Policy | Status |
| --- | --- | --- | --- | --- | --- | --- |
| Library page | `db/query.rs` `list_games` | One unreadable game or image record failed the whole page | Library unusable | Skip the record (logged); a bad image leaves the game without it | A | Fixed |
| Notification feed | `db/notifications.rs` | One unknown category (a downgrade) failed the feed | Feed unusable | Skip the row (logged) | A | Fixed |
| Other lists | `db/games.rs:169,206` (`entries_for_game`, root entries) | `collect::<Result<_>>` over records | One bad row fails the call | Same skip-and-log, where a partial list is safe | A | Follow-up |
| Settings registry | `settings/service.rs` | A poisoned lock: "settings registry is unavailable" until restart | Every settings panel broken | Poison-tolerant locks | A | Fixed |
| Damaged settings section | `settings/service.rs` (`Stored`, `Validation`, `Migration`) | Fails every read, message without the section | One bad section breaks its panel for good | Back up the raw value, reset to defaults, tell the user | C | Follow-up |
| Newer settings or database | `SettingsError::Version`, `MigrateError::VersionMissing` | Raw text | No guidance | `unavailable` with `updateCaskade` | M | Fixed (reporting) |
| Library database open | `db/mod.rs:52` | Raw sqlx text | No path or plain cause | Wrap with the path; point at the backup | M | Follow-up |
| Not found | `db/mod.rs` `missing()` | `Error::Invalid("X was not found")` | Reads as the user's mistake | A `NotFound` variant/code | – | Follow-up |
| Storage catalogue | `storage/catalogue.rs:67` | A damaged `storage-roots.json` fails every start | Storage broken | Rebuild from the database's roots | A | Follow-up |
| Storage move recovery | `storage/moving.rs:297` | "move recovery deferred" logged | A stuck move is invisible | Notification | A | Follow-up |
| Storage errors | `storage/mod.rs` | "storage root is offline or its identity changed" | Drive not named | `storageOffline` with plain text and hints; naming the drive is a follow-up | M | Fixed (code/text); Follow-up (name) |

### Core: launches and sessions

| Area | Where | Was | Gap | Recovery | Policy | Status |
| --- | --- | --- | --- | --- | --- | --- |
| Process monitor | `tracking/monitor.rs` | A panicked read poisoned the cache: every later launch failed until restart | Launches broken | Carry on with the cache | A | Fixed |
| Running-session lock | `launch/service/mod.rs` `try_game_lease` | Poisoned lock: "running-session lock poisoned" | Launches and moves broken until restart | Poison-tolerant | A | Fixed |
| Spawn failure | `launch/service/mod.rs` `spawn` | "No such file or directory (os error 2)" | Program not named | "couldn't start “program”: …" | M | Fixed |
| Profile before-command | `launch/service/scripts.rs` `profile_hook` | Any failure: "launch was cancelled" | Real cause only in the log | `Refused`: names the command and its exit code, signal, start error or timeout | M | Fixed |
| Plugin `launch.before` cancel | `launch/bus.rs` | Plugin's reason dropped: "launch was cancelled" | User can't tell who stopped it | `Refused`: "The plugin “id” stopped the launch: reason" | M | Fixed |
| Missing game folder | `launch/resolve.rs` | Bare ENOENT from `canonicalize` | Folder not named | Names the folder and likely causes | C (locate/reinstall) | Fixed (message); Follow-up (offer locate) |
| Missing storage root | `launch/resolve.rs` | "storage root unavailable" | No guidance | "…isn't available. Reconnect its drive…" | M | Fixed |
| ROM missing | `launch/emulator.rs:249` | "the game's ROM file is missing" | No path, no rescan | Add the path, offer a rescan | C | Follow-up |
| Emulator executable | `launch/emulator.rs:397` | Not checked before spawn | Generic ENOENT | Check; offer re-detect or reinstall | C | Follow-up |
| Session watch | `launch/service/watch.rs` `watch` | One snapshot/heartbeat error ends tracking | Playtime only recovered at next start | Bounded retry with backoff | A | Follow-up |
| After-hooks | `launch/bus.rs` `after` | Skipped hooks ignored | Save sync failures invisible | Notification for skipped hooks | A | Follow-up |
| Launch error kinds | `launch/mod.rs` `Invalid` | Catch-all "invalid launch:" prefix | Network/I/O read as user error | `ErrorReport` drops the prefix; more variants later | – | Fixed (display); Follow-up (variants) |

### Core: integrations and caches

| Area | Where | Was | Gap | Recovery | Policy | Status |
| --- | --- | --- | --- | --- | --- | --- |
| IGDB cache | `igdb/store.rs`, `cache_db.rs` | A damaged `games.sqlite` failed every lookup forever | Metadata dead until deleted by hand | Set aside as `.damaged`, start again, refill from the server | A | Fixed |
| Hasheous index | `hasheous/index.rs`, `cache_db.rs` | Same for `index.sqlite` | Matching and search dead | Same; the next sync re-indexes the zips | A | Fixed |
| Hasheous search failure | `metadata/find.rs`, `metadata/mod.rs` | Logged at debug, then "no match" recorded for a week | Games stay unmatched a week after a glitch | `Unsure`: games with nothing else to ask are deferred to the next run (bounded backoff) | A | Fixed |
| Hasheous bad zip | `hasheous/mod.rs:302` | Retried every 15 min forever if the MD5 matches | Retry storm on a corrupt zip | Delete it after an import failure; cap attempts | A | Follow-up |
| Hasheous/IGDB/RomM background failures | `hasheous/mod.rs:389`, metadata, RomM sync | `warn!` only | Users never learn the offline database is missing | `ServiceFailed` notification, one per service, resolved on success | A | Follow-up (high) |
| IGDB pause error kind | `igdb/mod.rs:626` | `io::Error::other("metadata server is unavailable…")` | Not seen as transient | `TimedOut` kind | A | Follow-up |
| Metadata "no readable game" | `metadata/page.rs` | `Error::Invalid` | Server fault reads as validation | `server` code | – | Follow-up |
| Artwork files deleted | `artwork/asset.rs`, `metadata/apply.rs` `missing()` | Rows checked, files not | Blank artwork forever after a cache clean | Treat a row whose files are gone as missing and download again | A | Follow-up (high) |
| Artwork folder deleted at runtime | `artwork/cache.rs` | `Dir` opened once | Writes fail until restart | Reopen and recreate on `NotFound` | A | Follow-up |
| Artwork worker | `artwork/cache.rs:62` | `map_err(\|_\| Worker)` | Cause lost | Keep the cause | – | Follow-up |
| RomM archive check | `romm/cache.rs` `verify` | Unpack errors became `false`, i.e. a checksum mismatch | Disk full looked like a bad download (fetched again) | Environmental errors (space, access, drive, busy) are returned as such | M | Fixed |
| RomM keychain | `romm/mod.rs:608` | Keychain error shows "no credentials" | Locked keychain looks like missing sign-in | Expose keychain state | M | Follow-up |
| RomM BIOS fetch | `romm/bios.rs:37` | Per-server failures logged | No reason shown | `errors` on `BiosFetch` | – | Follow-up |
| Icons | `icons/mod.rs:75` | Check recorded even after a transient import failure | Icon never retried | Record only when nothing transient failed | A | Follow-up |
| Update recorded | `install/update.rs:268` | "updated release not recorded" is a warning; success reported | Update badge stays | Retry the write, flag the job | A | Follow-up |
| Plugin install handlers | `install/hooks.rs:121` | Handler errors logged | "No plan" without the reason | Attach to job diagnostics | – | Follow-up |
| Update/emulator settings | `updates/mod.rs:267`, `emulation/setup/updates.rs:186` | `from_value(…).unwrap_or_default()` | A bad section silently becomes defaults | Log; salvage | A | Follow-up |
| Installer settings | `install/settings.rs:22` | Any read error fails every install | One bad section blocks installs | Defaults + notice (settings/installer.rs is being changed elsewhere) | C | Follow-up |

### Core: emulators

| Area | Where | Was | Gap | Recovery | Policy | Status |
| --- | --- | --- | --- | --- | --- | --- |
| Interrupted emulator update | `emulation/setup/jobs.rs:412` | A crash mid-swap strands `.old/` or `.staging/` | Emulator or user files stranded | Finish or undo at startup, like `recover_updates` | A | Follow-up (high) |
| Emulator downloads | `emulation/setup/download.rs:47,76` | "the download was cut short" as `Invalid`; no retry or resume | One blip fails the install | Reuse the install `RETRY_DELAYS` and `.part` resume | A | Follow-up |
| Executable bit | `emulation/setup/unpack.rs:257` | `let _ = executable_bit(..)` | Later "Permission denied" at launch | Job warning with the path | A | Follow-up |
| Shipped manifest | `emulation/setup/unpack.rs:334` | Unreadable `shipped.json` = empty | Stale libraries carried into the new install | Carry only known user folders; warn | A | Follow-up |
| ROM scan folders | `emulation/scan/enumerate.rs:21` | Bare ENOENT; one unreadable subfolder aborts | No folder named | Name it; skip unreadable subfolders with a warning | A | Follow-up |

### Standalone crates

| Area | Where | Was | Gap | Recovery | Policy | Status |
| --- | --- | --- | --- | --- | --- | --- |
| Discovery folders | `caskade-discovery` `sources/{epic,riot,bottles}` | `list(..).unwrap_or_default()` | An unreadable folder looked like "no games" | `list_or_warn`: OS errors other than not-found become scan warnings | A | Fixed |
| Discovery, other sources | `sources/prefixes/find.rs:107,163,236`, `steam/users.rs:106`, `ea/mod.rs:214` | Same pattern; `config.vdf` errors dropped | Wrong compat tool or missing playtime without a trace | Same helper (needs a `ScanCtx` there) | A | Follow-up |
| Background discovery scan | `src-tauri/src/discovery/lifecycle.rs` | `let _ = self.scan()` | A scan that couldn't start left no error | Shown in Settings → Library like a failed scan | A | Fixed |
| Discovery cancel | `src-tauri/src/discovery/mod.rs:215` | A user's cancel stored as an error | Red alert for a cancel | Don't set `error` on cancel | – | Follow-up |
| Plugin manifest | `caskade-plugin-host/src/manifest.rs` | `map_err(\|_\| "does not exist")` | Access errors called missing | Keeps the cause for anything but not-found | M | Fixed |
| Plugin grants | `caskade-plugin-host/src/hooks.rs:161` | Hook needing an ungranted permission silently unregistered | Plugin looks enabled, hook never runs | "Inactive hooks: needs X" on the plugin's summary | C (grant) | Follow-up |
| Plugin store reads | `caskade-plugin-host/src/host/mod.rs` `grants`, `host_api.rs:71,218` | Store errors become "never enabled", `None` | Plugin may overwrite data it couldn't read | Pass errors up; typed WIT error (contract change) | – | Follow-up |
| Component cache | `caskade-component-cache/src/lib.rs` | Failed write left `*.tmp<pid>` | Temp files pile up | Removed on failure | A | Fixed |
| Component cache eviction | same | None | Old entries forever | Evict unused entries | A | Follow-up |
| Theme files | `caskade-theme/src/files.rs:92` | Unreadable files treated as missing | Theme compiles empty with no problem | Report the I/O error (changes the `Files` trait) | – | Follow-up |
| ROM importers | `caskade-sources/src/roms.rs:50,82` | Failed scanner logged, `Ok(vec![])` | No import and no error | Attach to the scan report | A | Follow-up |
| Gamepads | `caskade-input/src/service.rs:212` | Init failure logged | User thinks the pad is broken | Expose availability and the reason | M | Follow-up |
| Job panics | `caskade-jobs/src/lib.rs:192` | `JoinError` stringified | Panic text lost | `into_panic` message | – | Follow-up |

### Installer (owned by #158 and the installer work in progress)

| Area | Where | Was | Gap | Recovery | Policy | Status |
| --- | --- | --- | --- | --- | --- | --- |
| Install/update jobs | `failure.rs`, core `install/recovery.rs` | Classified `Failure`, bounded transient retries, JobRecovery UI | – | – | A/C/M | Existing (#158, #159) |
| CLI errors | `src/bin/caskade-install.rs:155` | `error: …`, exit 1; JSON has only a message | `Failure` unused; no exit codes per kind | Classify, add `failure` to `--json`, exit codes (a contract change) | – | Follow-up |
| CLI output | `src/bin/cli/output.rs:64`, `cli/mod.rs:31` | `{question:?}` Debug output; 7-Zip detection `.ok()` | Raw structs; "unsupported archive" without the reason | Display text; keep the detection error | – | Follow-up |
| Resumable state | `fetch/storage.rs:67`, `fetch/engine.rs:308` | A damaged state file fails every retry | Download blocked until deleted by hand | Discard state and partial, start fresh | A | Follow-up (fetch is being changed elsewhere) |
| Download backoff | `fetch/engine.rs:263`, `fetch/resolvers/common.rs:108` | Fixed backoff without `Retry-After` or jitter; resolver backoff uncapped | Retry storms against a rate-limited host | Honour `Retry-After`, cap and jitter | A | Follow-up (same) |
| GitHub resolver | `fetch/resolvers/github.rs:124` | `map_err(\|_\| "invalid GitHub release response")` | Cause lost | Keep the serde error | – | Follow-up (same) |

### Summary

| Status | Rows |
| --- | --- |
| Fixed in #160 | 32 (6 of them partly, with a follow-up for the rest) |
| Existing | 1 (installs and updates, #158/#159) |
| Follow-up | 56 |

The follow-ups most worth their own issues: file logging and Open log folder;
a startup health record replacing Tauri's "state not managed"; a
`ServiceFailed` notification for persistent background failures; artwork
self-healing after a cache clean; recovery of interrupted emulator updates;
moving the remaining commands and inline alerts to `CommandError` and
`FailureNotice`; and the installer CLI's classified JSON errors.

## Tests

- `crates/caskade-core/src/error_tests.rs`: classification of OS, network,
  wrapped and launch errors, redaction of URLs, `key=value` and bearer
  secrets, references, the JSON shape, and that this document lists every
  code.
- `crates/caskade-core/src/cache_db.rs`: a damaged cache is set aside and
  started again (once), and other failures aren't retried.
- `crates/caskade-core/src/igdb/tests.rs`: a damaged `games.sqlite` is
  refilled from the (wiremock) server.
- Launch tests: a plugin's cancel and a failing before-command name their
  cause (`bus_tests.rs`, `emulator_tests/sessions.rs`).
- `apps/desktop/src-tauri/src/permissions_tests.rs`: refusals cross as
  `forbidden` reports.
- `apps/desktop/src/api/reply.test.ts` and
  `packages/ui/src/ErrorNotice.test.tsx`: the webview keeps codes and
  details, and the details toggle works from the keyboard.
