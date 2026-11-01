# caskade-installer

A standalone Rust library and development CLI for inspecting, planning,
installing and uninstalling games under a managed storage root. The library has
no Tauri dependency. Plans are data; probing never executes an input program.

## Build the development CLI

From the repository root:

```sh
flock /tmp/caskade-heavy.lock cargo build -p caskade-installer --features cli --locked
```

Run `caskade-install` from Cargo's target directory (respecting
`CARGO_TARGET_DIR`), or use:

```sh
flock /tmp/caskade-heavy.lock cargo run -p caskade-installer --features cli --locked -- --help
```

Default features include HTTP downloads; `cli` enables the binary. A local-only
CLI also builds with `--no-default-features --features cli`: local probe, plan,
install and uninstall remain available, while fetch/URL input returns a clear
feature error. Format features describe payloads, independently of the host OS.
`torrent` (off by default) adds magnet links and `.torrent` files; see
[Torrents](#torrents).

## Commands

```sh
caskade-install fetch 'https://example.test/game.zip#sha256=<64 hex digits>' --root /games
caskade-install probe ./game.zip
caskade-install probe 'https://example.test/game.zip' --root /games
caskade-install plan ./game.zip
caskade-install plan ./game.zip --host windows
caskade-install plan ./game.zip --host ./my-host.toml --json
caskade-install install ./game.zip --root /games
caskade-install install ./PortableGame --root /games
caskade-install install 'https://example.test/game.zip' --root /games --keep-downloads
caskade-install uninstall '/games/Game Folder'
caskade-install uninstall '/games/Game Folder' --external-root 'user.desktop=/home/me/Desktop'
caskade-install --json probe ./game.zip
caskade-install move '/games/Game Folder' /another-root
caskade-install reconcile /games
caskade-install reconcile /games --game 'Renamed Game' --from '/old-root/Old Game'
```

`--json` is global: place it before or after the subcommand. Paths containing
spaces must be quoted for your shell. HTTP and HTTPS URLs are accepted; plain
HTTP is recorded as insecure transport. Quote checksum fragments and URL query
strings so the shell passes them intact.

### fetch

`fetch <url>... [--root <dir>] [--job-id <id>]` resolves URLs, downloads files and
retains them for inspection or later installation. Files live under
`<root>/.caskade/downloads/<job-id>/`. Names come from safe HTTP metadata. SHA-256
hints/fragments are verified. A `.part` file and its validator metadata support
resume across process restarts; the default job ID is a stable hash of the input.
Use the same root and input (or explicit `--job-id`) to resume. A job ID must be
one safe filename component.

`fetch` accepts multiple URLs, including multipart sets. Install a downloaded
set by selecting a local member; local input resolution gathers and validates
its sibling volumes. The initial built-in resolver is Direct; service-specific
resolver integrations are separate work. A link returning HTML produces an
error instead of passing a web page to the installer.

### probe and plan

`probe <path|url> [--root <dir>] [--job-id <id>]` reports static identification,
resolved local input membership and probe I/O counters. Containers remain lazy:
an incomplete child listing is represented by `children_complete: false`.

`plan <path|url> [--host <profile>] [--root <dir>] [--job-id <id>]` reports ranked
candidates, their steps, retained alternatives and any required decision. It
does not execute the plan. URL analysis downloads into the specified root first
and keeps those files available for installation.

Built-in profiles are embedded from `tests/hosts`:

- `windows`
- `linux`, `linux-no-umu`, `linux-exfat-root`
- `macos`, `macos-no-wine`

`--host` also accepts a HostCaps TOML file. This selects the analysis environment;
it never changes the OS or runtime used by `install`. Without a profile, the CLI
detects the current host. An unavailable/unverified local 7-Zip binary is omitted
from executable capabilities; supplied planning profiles retain their declared
capabilities without running those tools.

### install

`install <path|url> --root <dir> [--keep-downloads] [--extract] [--job-id <id>]` uses the
registered archive, SFX, Inno, MSI, NSIS, Linux package and shell, portable-folder
and ROM handlers.
ZIP, 7z (including split
volumes), tar and compressed tar inputs use native extractors. RAR and other
sidecar formats require verified 7-Zip. PE self-extracting ZIP/7z archives are
opened without running their stubs; RAR SFX uses the verified sidecar.
Makeself and GOG/MojoSetup shell installers are extracted statically; confined
`gameinfo` and `start.sh` hints guide launch discovery. Their bootstrap scripts
are not executed during installation.
MSI embedded and sibling cabinets are extracted using File/Component/Directory
and Media tables. Custom actions are not executed and yield partial fidelity.
Inno extraction places `{app}` files without running installer code. NSIS 2/3
ANSI/Unicode and solid/non-solid inputs resolve `SetOutPath` into the managed
application tree. Static NSIS extraction suppresses Start-menu shortcuts and
records registry, shortcut and command declarations without replaying them.
The upstream NSIS reader uses a whole-input slice; default analysis/extraction
budgets are 128 MiB of input, 4 MiB of header/metadata and 64 MiB of decompressed
data. Library callers can configure `NsisLimits`; inputs beyond these budgets
return an explicit error.
Native Windows plans retain silent and interactive alternatives; only a strategy that
actually ran the installer records its native uninstaller. `--extract` selects
static extraction and disables code execution, including native fallback
attempts; this also makes fixture cycles portable across host operating systems.
Unimplemented installer families return
an explicit unsupported outcome; the CLI does not guess their command lines.

Portable folders are copied and the original remains intact. They must be
disjoint from the managed root and contain no `.caskade` metadata. Known ROMs
return a hand-off outcome without executing a job or deleting the input.

Managed installations stage within the storage root. Discovery and the file
inventory/manifest run before atomic commit. A cleaned source name supplies the
provisional folder name, with a safe `Game-<id>` fallback and collision suffixes.
Ambiguous discovery rolls the candidate back and returns `needs_user`.

Human progress goes to stderr with phase, step, bytes, file count and ETA when
available. Press **Ctrl-C** to cancel; the CLI waits for executor cleanup before
exiting. Interrupted downloads may retain resumable partial files. Successful
URL installations delete their downloads unless `--keep-downloads` was supplied;
local source files remain intact.

### Verified 7-Zip

The global `--seven-zip <program>` selects an official pinned sidecar. Its binary
SHA-256 (and companion DLL on Windows) must match the library's pinned release.
The CLI also uses a detected host tool if it passes that verification. A supplied
invalid binary is an error, rather than an unverified extraction fallback.

```sh
python3 scripts/7zz-download.py
caskade-install --seven-zip ./target/sidecars/7zip/26.05/linux-x64/7zz install ./game.rar --root /games
```

The downloader verifies the archive before unpacking. Extraction lists and
validates entries, quarantines external-tool output inside staging, and imports
validated files through confined writers. Native archive candidates retain the
sidecar alternative for a clean retry after rollback.

### uninstall and move

`uninstall <game-dir> [--external-root <scope=path>]...` reads the installed
manifest, runs recorded uninstall steps, reverses known external effects and
removes the game while holding the executor's root exclusion. The storage root's
`.caskade/root.json` identity remains after the last game is removed.

External scope bindings are explicit and repeatable. No desktop, AppData or
other arbitrary location is guessed. A missing binding, unsupported runtime or
cleanup failure retains the game/manifest for retry and returns a structured
report in JSON mode. Relative recorded paths remain confined to their scopes.

`move <game-dir> <target-root>` uses the relocation engine: rename on the same
volume, or copy and verify across volumes; retarget aliases/prefix links and
rewrite recorded references transactionally. Both roots are held in canonical
lock order. The target root identity is adopted by the moved manifest. Ctrl-C
cancels the job and waits for rollback.

## Torrents

With the `torrent` feature (#159), `probe` and `install` also take a magnet
link, a local `.torrent` file, or an HTTP(S) link that serves a `.torrent`
(recognized by name or by parsing as one). The torrent downloads first, then
its content goes through the same inspection, planning and installation as
any other input. The feature is off by default because the engine adds a large
dependency tree and a C build (below); `caskade-core`'s `net` feature turns it
on, so the app has it, and `--all-features` (CI) covers it.

```sh
caskade-install install 'magnet:?xt=urn:btih:<40 hex digits>&dn=Game' --root /games
caskade-install install ./game.torrent --root /games
caskade-install install 'https://example.test/game.torrent' --root /games
```

**Engine.** [librqbit](https://github.com/ikatson/rqbit) 9.0.1 (pure Rust
protocol code, Apache-2.0), with its default features off: no HTTP API, web
UI, UPnP, session persistence or Postgres; `disable-upload` only adds a
per-session switch. Its SHA-1 comes from `aws-lc-rs` (librqbit's `sha1-ring`
feature names it; built with `cc`, without CMake or bindgen on Linux, Windows
GNU and macOS, from the bindings it ships). librqbit's own TLS features are
not used, because they add `aws-lc-rs` to `rustls` next to the `ring` that
the rest of Caskade uses, and a process with both providers has no default:
its tracker client (`reqwest` 0.13) uses the ring provider the engine
installs when none is installed yet. The crate stays independent of Tauri.

**Protocol.** BitTorrent v1 and hybrid v1/v2 torrents (their v1 info hash);
v2-only torrents and magnet links with only a `btmh` hash are refused as
unsupported. Magnet links take hex or base32 `btih` hashes; their metadata
comes from peers (BEP 9) found through the mainline DHT, the link's `tr`
trackers (HTTP(S) and UDP) and peer exchange, and waits up to 3 minutes
(`TorrentConfig::metadata_timeout`) before failing as "no peers". Private
torrents (BEP 27) use their first tracker only: never the DHT, peer
exchange or local discovery. Peer connections are outgoing TCP only: the
session doesn't listen, map ports or announce on the LAN.

**Upload.** While a download runs, uploading is capped at 256 KiB/s
(`upload_limit_bps`; 0 uploads nothing), so peers that expect a little back
keep sending. When the download completes, is cancelled or fails, the
torrent is removed from its session and the session stops: nothing is
seeded afterwards and no client keeps running in the background.

**Choosing the payload.** A multi-file torrent keeps its folders under the
torrent's name. Its files are grouped by top-level folder, and top-level
files by multipart set (`game.part1.rar`/`part2`, `.7z.001`, `.z01` with its
`.zip`, `.r00` with its `.rar`, Inno `-1.bin` slices with their `.exe`, a
`.cue` with its tracks), so companions always come together. When more than
one group holds something installable (an installer, archive, package,
program or disc image; documents and artwork don't count), the job asks with
`Question::ChooseAsset` before downloading anything but the metadata: the
first choice is everything, then one per group with its size. The answer
binds to the question, so a rerun (or a restart) asks nothing new. A single
payload with extras downloads everything.

**Progress.** Download-phase events say "Finding peers for the torrent's
metadata", "Verifying downloaded pieces" (existing data, with the bytes
checked), "Connecting to peers", "Downloading from N peers at R/s" (with
bytes, total and an ETA) and "Download complete". No verified byte for 5
minutes (`stall_timeout`) fails the job as `FetchError::NoPeers`, which is
transient: the app retries it on its own a bounded number of times, and its
failure summary says nobody sharing the torrent sent anything.

**Integrity and safety.** Every piece is SHA-1-checked against the metadata
before it counts; a corrupt piece from a peer is fetched again, and pieces on
disk are verified again before a resumed download uses them. That proves
consistency with the metadata, not who published it: the installer's OS
checks and the app's confirmation and permissions still apply. Names are
untrusted: each path component must pass the staging rules (no `..`,
absolute, drive-qualified, reserved Windows or control-character names, no
trailing dot or space, no undecodable bytes), and names that differ only in
case, or are both a file and a folder, are refused. BEP 47 symlink entries
are refused; padding files are skipped. Data folders are created and
checked without following links, and after the download every selected
file must still be a regular file of the expected length before anything
is installed. Limits
(`TorrentConfig`): 16 MiB of metadata, 10,000 files, 32 path components and
100 GiB in total; free space for what is still missing (plus 16 MiB) is
checked before the download starts.

**Storage, resume and cleanup.** Data lives in
`<root>/.caskade/torrents/<info hash>/` (`meta.torrent` for a magnet link's
metadata, `data/` for the files), and the job's
`.caskade/downloads/<job>/.state/torrent` names it. Cancelling stops all
network activity and keeps the verified pieces; a later job for the same
torrent, in this run of the app or after a restart, verifies them and
fetches only the rest. After a successful install the data is deleted with
the job's download folder unless Keep downloads is on; "download again",
"locate another source" and "choose another root" discard it too. A
selected file that vanished after downloading (security software, say)
fails as a missing staged file and isn't fetched again until the user asks.

Tests (`tests/torrent.rs`, in the `it` binary with `--features torrent`)
seed fixtures from a second librqbit session on 127.0.0.1 with the DHT,
trackers and local discovery off, so CI never reaches a public swarm.

## Failures and recovery

Files can disappear or become unreadable between any two stages, and while a
stage uses them: security software quarantines an installer, a drive is
unplugged, another program holds a file open. A preflight check can't prevent
that, so the library checks again where it matters and classifies what went
wrong after the fact. [`failure::classify`](src/failure.rs) turns any pipeline
error into a `Failure`: its kind (missing source, staged or output file;
access denied; file in use; integrity; disconnected storage; disk full;
download), the stage (download, inspect, install, finalize, commit), the file,
whether it is transient, a plain-language summary, redacted details (URLs lose
credentials, query and fragment) and the recovery actions that suit it: retry,
download again, locate another source, choose another root, cancel. It walks
the error's causes and then looks at the filesystem itself: a source that is
gone or a root that can't be listed outranks what the error text says.

- **No unsafe fallback.** The executor normally tries the next strategy when
  one fails. It stops instead when an input that was there at the start has
  vanished, access is refused, a file is locked, the storage is disconnected
  or the disk is full: the next strategy would fail the same way, or run
  installer code nobody chose. The failed attempt is rolled back first.
  Unless the execution policy is `allow` (`ExecConfig::execution`), a
  failed strategy that runs no installer code never falls back to one that
  does: the job asks `ConfirmExecution` with the candidates left.
- **No question with one answer.** `RunControl::answer` takes the only
  option of a question that has one (an asset, an installer, a launch
  target, a strategy). A lone strategy that runs installer code
  (`LowConfidence`, `ConfirmExecution`) still asks, unless the user answered
  a `ChooseAsset` in this job: picking the download was the consent.
- **No false success.** After commit, the install's launch target must still
  exist. If it vanished, the new install is uninstalled (an update's alias is
  kept, so the version it replaced gets it back) and the run fails with
  `MissingExpected`; the download stays for a retry.
- **Rollback is limited to the job's own output.** Staging, the job's
  journal entries (out-of-root effects of a native installer) and, for a
  discarded download, `.caskade/downloads/<job>`. Sources, other games and the
  root's identity are never touched.
- **Retries reuse verified progress.** A kept download resumes only when its
  validators match, and is checksummed before use; a download that vanished is
  fetched again; a checksum mismatch discards it. Every run plans again from
  the input as it is now.
- **No duplicate external installers.** Each run first rolls back what an
  interrupted run left (`Executor::cleanup`: the journal and staging), so a
  native installer that may have run is compensated before anything runs
  again. The app also does this for every reachable root at startup.
- **Security software.** A file that vanished or was refused for no visible
  reason is reported with security software as a *possible* cause. Only
  Windows' `ERROR_VIRUS_INFECTED` / `ERROR_VIRUS_DELETED` count as confirmed.
  Caskade never disables security software, adds exclusions or restores
  quarantined files.

Platform limits: on Linux and macOS, deleting a file that is already open
doesn't interrupt the reader, so a quarantine there shows up at the next
stage; a removed drive usually reports `EIO`, `ENXIO` or `ENODEV`. On Windows,
security software typically holds a new file open while scanning it (reported
as in use, which is transient) or denies access to it. macOS Gatekeeper
quarantine attributes don't stop the installer reading a file and aren't
reported. Network shares that drop report stale handles (`ESTALE`) or
Windows' `ERROR_NETNAME_DELETED` as disconnected storage.

In the app (`caskade_core::install`), a job's snapshot carries the `Failure`.
Transient failures (network or server trouble, a file in use) run again on
their own twice, after 15 s and 60 s; anything else waits for the user, so a
blocked file is never downloaded again in a loop. The job counts repeats of
the same failure and says when it happened again. The Activity view shows the
failure with its actions in desktop and console mode (`job_recover`). The job
list itself is not kept across restarts: after one, the interrupted install
is rolled back and starts again from the user's request.

## Output and exit status

Human output is the default. `--json` writes one final JSON value to stdout and
suppresses human progress. Diagnostic tracing uses stderr (`RUST_LOG` controls
verbosity). Help and argument-validation messages use Clap's standard output.

| Operation | JSON value |
| --- | --- |
| probe | `{input, resolved, artifacts, io}` |
| plan | `{host, artifacts, plans}` |
| fetch | `{type: "downloaded", value: {job_id, files}}` |
| install | serialized `InstallOutcome` (`installed`, `hand_off`, `needs_user`, `unsupported`) |
| uninstall | `{type: "uninstalled", value: <UninstallReport>}` |
| move | `{type: "moved", value: {from, dir, copied, rewritten_files, rewritten_values}}` |
| reconcile | `{type: "reconciled", value: <RootReport or GameReport>}` |
| failure/cancel | `{type: "error" | "cancelled", value: {message, report}}` |

Exit statuses: **0** success (including hand-off or a successfully displayed
plan), **1** operational error/unimplemented command, **2** an install/resolver
needs a decision or command-line usage is invalid, **3** unsupported install,
**130** Ctrl-C cancellation after cleanup. Incomplete uninstall reports are
included in the failure's `report` field; unrelated errors use `null`.

## CLI verification

```sh
flock /tmp/caskade-heavy.lock cargo test -p caskade-installer --all-features --locked --test cli
flock /tmp/caskade-heavy.lock env CASKADE_7ZZ=/absolute/path/to/verified/7zz cargo test -p caskade-installer --all-features --locked --test cli rar_fixtures_complete_the_binary_pipeline_with_the_verified_sidecar -- --ignored --exact
```

The binary tests run probe → plan → install → uninstall over all 12 native archive
fixtures, verify exact payload trees and manifests, then assert a clean TestRoot
after removing the test-owned storage identity. The optional native test does
the same for all three RAR fixtures and the genuine RAR SFX. Two genuine ZIP/7z
SFX fixtures also run through the default pipeline tests. Tests cover six host profile snapshots,
both committed Makeself/GOG shell fixtures, three MSI packages and fifteen
unpassworded Inno installers and all six NSIS variants complete the same binary
cycle. Tests also cover
custom TOML profiles, human/progress output, URL caching and installation,
portable folders, ROM hand-off, structured errors and real SIGINT cancellation
during a slow HTTP response.

Gitea workflow coverage, nextest profiles and user-owned runner registration are
documented in [CI setup](docs/ci.md). Native Windows/macOS evidence is pending
runner registration; local cross-compilation is recorded separately.

The [native Windows acceptance tests](docs/ci.md#native-fixture-evidence) share
payload goldens with extraction, exercise all six Inno/NSIS/MSI mini installers
through the CLI on every PR, and retain Windows OS command-line readback reports.
The documented Wine replay is pre-evidence only; Windows runner and manual
wizard checks remain required.

Disc inputs use a verified `--seven-zip` sidecar without mounting images. PC ISO/UDF
images select their validated nested installer and keep temporary contents alive
through the transaction. Console CUE/BIN sets return a ROM hand-off with all input
members. Multiple validated installers return a user-choice question. Install
manifest provenance keeps the original disc or download URL.

Native install steps observe the host's standard user-data/desktop locations and
record scoped external file effects in the manifest before commit. Uninstall binds
the same standard scopes; `--external-root scope=path` explicitly overrides a
standard binding or supplies an additional one. Unbound scopes remain retryable
errors rather than guessed filesystem locations.

## Share links and release pages

The `fetch` feature enables built-in GitHub, Dropbox and public Google Drive
resolvers in `FetchEngine::new`. The CLI uses them for URL inputs to `fetch`,
`probe`, `plan` and `install`:

```sh
caskade-install install https://github.com/owner/game/releases/latest --root ./games
caskade-install fetch 'https://www.dropbox.com/scl/fi/token/game.zip?rlkey=key&dl=0' --root ./games
caskade-install install 'https://drive.google.com/file/d/public-file-id/view' --root ./games
```

GitHub repository URLs, `/releases`, `/releases/latest` and `/releases/tag/<tag>`
select release assets by host OS and architecture. Source/debug archives are
excluded; platform-neutral archives remain eligible. Equal scores return
`NeedsUser(ChooseAsset)` with the alternatives rather than guessing a filename.
Direct `/releases/download/...` URLs go straight to the downloader. Checksums come
from GitHub's `sha256:` digest or a matching `.sha256`/`.sha256sum` asset or checksum
list. GNU, BSD and single-hash sidecars are supported. An input `#sha256=...` is
preserved, and conflicting checksum hints fail before a download is accepted.
See the [GitHub release API](https://docs.github.com/en/rest/releases/releases).

Dropbox links retain resource keys and replace existing `dl`/`raw` switches with
`dl=1`, following [Dropbox's direct-download format](https://help.dropbox.com/share/force-download).
Google Drive resolves public `/file/d/<id>/view`, `/open?id=...` and `/uc?id=...`
links, including anonymous large-file confirmation forms, download-warning cookies
and older confirmation links. Confirmation actions stay within configured Drive
download endpoints. Private/sign-in/quota pages return an error; account/browser
cookies are never imported. Drive's anonymous confirmation HTML can change and is
covered by explicit contract fixtures rather than an authenticated recording.

For library callers, `FetchEngine::with_resolver_config(ResolverConfig)` or
`ResolverRegistry::with_builtins` configures provider endpoints and metadata limits.
Explicitly registered resolvers run before built-ins; ordinary HTTP(S) links retain
the Direct fallback. The CLI accepts a TOML file through
`CASKADE_RESOLVERS_CONFIG=/path/to/resolvers.toml`, for example:

```toml
github_web = "http://127.0.0.1:8080/github/"
github_api = "http://127.0.0.1:8080/api/"
dropbox_web = "http://127.0.0.1:8080/dropbox/"
drive_web = "http://127.0.0.1:8080/drive/"
drive_download = "http://127.0.0.1:8080/drive/uc"
drive_content = "http://127.0.0.1:8080/content/"
metadata_max_bytes = 4194304
metadata_timeout_secs = 20
metadata_retries = 2
```

Unspecified fields use production defaults. Base URLs cannot contain credentials,
queries or fragments; directory bases end in `/`. Metadata is bounded to 4 MiB and
20 seconds per request by default (configurable up to 16 MiB and 300 seconds).
A request that times out, cannot connect or gets HTTP 408, 429 or 5xx is retried
twice with backoff (`metadata_retries`, up to 10). JSON and checksum metadata that
arrives as a web page (a captive portal) is an error, not data.
Redirects retain the downloader's ten-hop and HTTPS downgrade checks. CLI Ctrl-C
cancels resolution as well as downloads and installation.

Default tests use recorded public GitHub metadata and local HTTP servers. They
exercise six host profiles plus ARM64 and run a chosen fixture asset through the
actual CLI download → install → verified manifest → uninstall cycle. No live
service is contacted by those tests. To explicitly refresh the public recording:

```sh
just test-resolvers-live
# Equivalent, including the explicit network opt-in:
CASKADE_RESOLVERS_LIVE=1 flock /tmp/caskade-heavy.lock cargo test -p caskade-installer --all-features --locked --test it fetch_resolvers_live::refresh_public_github_response -- --ignored --exact
```

The `justfile` recipe is added only to the working tree because that file contains
user-owned uncommitted work. The ignored test is committed and can be invoked with
the equivalent command above. It fetches the pinned public 7-Zip 26.05 release API
response, records its SHA-256 and capture time, and projects only download metadata;
it never downloads or executes release assets. Set `CASKADE_RESOLVERS_RECORD_DIR`
to write elsewhere. Review response and snapshot changes before committing them.
Native test profiles exclude this explicit live recording test.

## Repairing externally moved games

`reconcile <root>` scans managed game folders and repairs aliases and references
in place. `reconcile <root> --game <relative-directory>` selects one game;
`--from <previous-absolute-location>` supplies a missing legacy anchor and requires
`--game`. `--alias-dir <directory>` selects an alias store, and repeatable
`--external-root <directory>` grants access to recorded external files in those
directories. Other external-file paths are refused. The CLI checks OS process
executables and working directories and refuses a game with a live matching
process; application storage supplies its own process tracker through `RunningCheck`.

The root must already have `.caskade/root.json`. Its stable root ID survives drive
letter or mount-point changes. Each install records `relocation.last_path`: a
host-path provenance string used as the old rewrite anchor, never a writable
capability. Payload paths remain relative. A scan after a folder/root rename
updates the anchor, root ID and relative directory through the same transaction
as text/registry changes and alias retargeting. Only rewritten files receive new
inventory hashes; an unrelated changed uninstaller stays detectable.

A still-existing old location produces a typed `ChooseInstallDirectory` question
and preserves both copies. A legacy non-Free manifest with no anchor or alias
target also needs an explicit old location. Fragile games retain their verdict,
receive a warning and a typed `ReinstallAfterMove` question containing their
recorded local source hint or original URL. Storage resolves kept-source
availability; this command does not reinstall automatically. Pinned delegated
installations are reported without reference repair.

`RootReport.games` contains each game's status, rewritten-file/value counts,
question, warnings, error and resulting manifest. Individual failures do not
hide the remaining games. Exit **1** reports failed games; **2** reports a required
choice; **0** means the scan completed without either. Free/Aliased games remain
eligible for their verified own uninstaller; Rewritable/Fragile games use recorded
manifest cleanup after a move. Missing or modified uninstallers also fall back.

Library consumers construct `reconcile::Reconciler` with an injected
`Arc<relocate::Relocator>`, then call `scan_root(&root, token).await` or
`reconcile(ReconcileRequest { root, relative_dir, old_dir, job_id }, token).await`.
`prepare_references` exposes the same bounded rescan and confinement validation
for managed moves, with a conservative verdict to persist in the move transaction.
`with_repair_factory` selects separate registry/prefix capabilities for each game
on a root; `with_limits` bounds entry count and descent depth. The CLI supports
game-local `.caskade/prefix` Wine/Proton registries. Applications with prefixes
elsewhere inject an appropriate factory. Jobs share install/uninstall root
exclusion, run blocking work off the async thread, and retain their lease until a
cancelled/dropped caller's worker finishes.

The installer-side scan is ready for the application storage scanner in **#54**;
that application integration remains outside this milestone. Tests cover plain
external folder/root renames, same-inode repair, alias/copy ambiguity, transactional
rollback, uninstall policy, confined references and CLI process checks. Real Wine
proof is opt-in and uses private Wine/XDG state:

```sh
CASKADE_TEST_WINE=1 flock /tmp/caskade-heavy.lock xvfb-run -a \
  cargo test -p caskade-installer --all-features --locked --test it reconcile_wine:: -- --ignored
```

The Wine test renames a folder outside Caskade, calls `scan_root`, and uses live
`reg.exe export` to verify the rewritten value. With the CLI feature it then
moves the game into another root through the built binary, verifies the destination
prefix registry, and checks its new root identity and manifest hashes. Ordinary
tests make no Wine calls.

Managed `move` rescans and validates references under both root leases before
moving. Its `--alias-dir` and repeatable `--external-root <dir>` options grant
the same capabilities as `reconcile`; recorded external path strings alone do
not authorize writes. Existing prefix directories are validated at the source,
then registry writes use the destination prefix. If an external move has already
changed the path, run `reconcile` first. Pinned/delegated installs cannot be moved.

## DMG and Flatpak CLI routes

With `macos-formats`, `install image.dmg --root <dir>` uses the DMG handler and
an explicitly supplied native runtime/host capability snapshot. It mounts
read-only, copies and discovers app bundles, records the managed installation,
and detaches before committing or returning an error. Other hosts report
`Unsupported` (exit 3). UDZO, ULFO and SLA binary CLI cycles are ready in the
ignored `cli_dmg_native` test and consume the genuine `CASKADE_MACOS_FIXTURES`
artifact generated by the macOS runner; Linux tests simulate mounting without
claiming a native disk-image result.

With `linux-formats`, `install app.flatpak --root <dir>` and `.flatpakref` inputs
use the Flatpak delegate when the host supplies it. The result is a delegated
handoff (`flatpak:<app-id>`), and the storage root retains a Pinned manifest with
its recorded user-level uninstall command. `uninstall <managed-directory>` runs
that command and removes the manifest. This route uses the dedicated Flatpak
finalizer instead of looking for an unpacked executable. `--extract` forbids the
delegate. URL downloads follow the same retention/cleanup policy as other installs.
Default binary tests substitute a private fake Flatpak command and user directory;
they never install into the real user Flatpak state.
