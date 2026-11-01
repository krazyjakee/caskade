# Native installer commands (#52)

The webview submits paths/URLs or stored entry/root/job IDs. Plans, manifests,
executable paths, argv and external rewrite roots are not accepted over IPC.
The core owns the job state and the installer implements filesystem execution.

Commands generated in `src/api/bindings.ts`:

- `installStart(input, rootId)` and `fetchStart(input, rootId)` return an initial queued snapshot.
- `jobsList()`, `jobGet(id)`, `jobCancel(id)` observe/cancel jobs.
- `jobAnswer(id, revision, {type:"select", index})` selects from the current backend question. Stale revisions and invalid indexes fail without dismissing the question. `{type:"cancel"}` declines it. Observed outside-root directories and reinstall-after-move notices require explicit adoption/reinstall rather than automatic directory moves.
- `jobRetry(id, candidateIndex)` retries a stopped install using a retained backend alternative. A source that now produces different candidates requires a new review.
- `uninstallGame(entryId)`, `verifyGame(entryId)`, `moveGame(entryId, targetRootId)` operate on managed library records. Move and uninstall share the launch exclusion; native installers retain their recorded uninstall behavior.
- `chooseInstallFiles()` returns selected native paths (empty on cancel); `chooseInstallFolder()` returns a path or null. The picker uses the dialog plugin. No shell or filesystem plugin permission is granted to the webview.

`jobChanged` carries a complete `JobSnapshot` on each change. `phase` is
`download`, `install` or `move`; the terminal `status` is `completed`, `failed`
or `cancelled`, and ambiguity is `needsUser`. Clients merge by increasing
revision because asynchronous delivery and initial list responses may arrive
in different orders. `dependencies`, `warnings`, `candidates` and the selected
candidate index feed the Advanced view. Verification returns file differences
on the snapshot without mutating the installed tree.

## Bundled 7-Zip

Before direct Cargo builds on a new checkout:

```sh
python3 scripts/tauri-sidecar.py
# Linux Windows-cross checks also need:
python3 scripts/tauri-sidecar.py --target x86_64-pc-windows-gnu
```

Tauri dev/build commands run this automatically. The script reuses
`scripts/7zz-download.py` and `7zz-pins.json` (official 26.05, package SHA-256
verified before extraction), stages ignored `binaries/7zz-<target>`, and never
executes a downloaded installer. `bundle.externalBin` packages the target
program. Windows also bundles its matching `7z.dll` adjacent to `7zz.exe`;
every bundle includes the upstream license. The native adapter supplies this
path to `InstallService::with_sidecar`, which verifies the extracted program
and Windows DLL hashes using the installer's existing pinned loader.

Use `--target` for cross packaging; the macOS payload is universal. Windows
MSVC/ARM64 and macOS signing/notarization behavior remain native verification
items on #22. Signing that changes the official binary bytes requires an
explicitly verified artifact-manifest update, as described in
`scripts/7zz-packaging.md`; no altered binary is silently trusted.

## Reconcile on discovery scans (#61)

The configured storage source is shared by storage adoption, the app discovery
controller and the library provider. Before each app discovery scan imports
external results, it refreshes registered managed roots. Root identity and
bounded remount hints are checked first. Reconciliation repairs aliases and
recorded references from the old manifest anchor; it does not move game files.
The library import keeps the installation identity and existing play sessions.

`discoveryStatus` and `discoveryChanged` expose `storage` (null until scanned),
a `StorageScanReport` containing every root and game status. Unresolved choices
remain typed `question` values on the affected games and also produce readable
warnings. A Fragile move offers `reinstall_after_move`; a duplicated identity
requires choosing a directory. Neither question authorizes an automatic move
or reinstall. Offline or malformed roots preserve existing library records.
Cancelling a discovery scan cancels and drains the active storage operation
before the controller reports cancellation.

The app-level regression suite is `cargo test -p caskade-desktop --test
reconcile_scan`: external rename, stable history, alias retarget, Fragile
questions, and offline-root retention. Real private Wine-registry repair is
covered by the installer's opt-in `reconcile_wine` test; native Windows/macOS
behavior remains on #22.
