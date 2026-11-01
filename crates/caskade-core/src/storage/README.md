# Storage roots and moves (#2)

`StorageService::new(store, settings, shared_launch)?.with_paths(paths)` owns
registered roots. `register_root(path, label)` creates/adopts the physical
`.caskade/root.json` identity, refuses overlapping locations and ambiguous online
copies, and selects the first root as default. `resolve`, `list_roots`,
`set_default`, `edit_root`, `capabilities` and `remove_keep_files` are the app
boundary. The default cannot be removed. Keep-files removal explicitly drops
its managed entries; disk files remain available for adoption.

`config/storage-roots.json` is an atomic independent catalogue, so losing the
library DB does not lose custom storage paths/identities/labels/defaults.
`restore_roots` rebuilds that registry; the #54 StorageSource reconciles and
imports each root's install manifests. Its bounded volume hints handle changed
mount points without searching whole disks. `initialize` restores roots, registers
`AppPaths::games` (`~/Games/Caskade`) as the default when none exists yet, then
recovers known pending moves before source scanning. Offline roots remain
registered, and physical identity mismatches never become writable authority.

Capabilities are measured in an exclusively created temporary probe directory
and cleaned afterwards: writable, executable, symlink and case behaviour, plus
filesystem free/total space. Linux mount metadata supplies filesystem/device
labels; other hosts may report unknown labels. The root picker and backend
recheck availability; #59 injects selected-root capabilities into each plan.
An existing game-local prefix cannot move to a target lacking symlinks: the
operation reports the reason before changing files. External-prefix fallback
is not silently invented by storage.

`storage.roots` holds validated per-root `{title}` templates in the settings
registry; `core.storage.alwaysUseDefault` controls picker skipping. The next
install passes the selected root and its template into the actual pipeline;
downloads/staging remain inside that root. No client-provided path is accepted
for moving/uninstalling an existing entry.

`move_entry(entry_id, target_root_id, job_id, sink, cancel)` acquires the shared
LaunchService game lease, rejects persisted running sessions, validates both
root identities and the manifest, checks target capabilities/space, then holds
both executor root leases through relocation and the DB update. Entry/game
IDs, user fields, sessions, collections and relative launch paths are retained.
External/delegated/Pinned installs and aliases belonging to another live copy
are refused. The #52 job queue supplies progress, cancellation and Activity UI.

The relocation engine stages cross-device data in `.caskade/staging`, copies
in bounded chunks, verifies bytes, and renames into place. Every compensation
is written and synced before mutation. A durable commit precedes source
removal. `recover_pending` uses DB-held trusted move intents: uncommitted moves
roll back, committed moves finish cleanup and the managed-location update.
Imported files cannot supply arbitrary recovery paths. Filesystem/registry,
alias, prefix and manifest rewrites reuse the installer relocation backends.
Linux directory renames and data are synced; native Windows/macOS parity is
tracked on #22.

The frontend uses generated commands. Settings, game details and lazy library
actions share one root inventory, refreshed on focus and periodically. An
offline managed copy is marked unavailable without hiding its game or changing
another launcher's copy. Moves and uninstalls wait for actual job completion
before a move-all/remove operation proceeds; failures remain visible.

Evidence includes genuine fixture launch after a managed move, real Proton
ext4-to-tmpfs install/move/self-check/own-uninstall, process-exit recovery during
copy/rename/reference rewrites and after commit, registry/alias rollback,
root catalogue rebuild, IPC CRUD, picker and availability tests.
