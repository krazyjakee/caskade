# Managed storage source

`StorageSource` imports managed installs as the `caskade-storage` source.
Construct it with the library store, bus and an injected `Arc<Reconciler>`.
`refresh(token)` scans roots already registered by core `StorageService` and
emits one `library.scan.completed`. After registering/adopting a root, call
`adopt_root(&StorageRoot, token)` to reconcile and import it immediately.

For automatic drive-letter/mount recovery, configure
`StorageSource::new(...).with_remounts(storage_service, env, &paths)`.
Restore the root catalogue before the first refresh.
The injected host environment supplies current volumes. The bounded
`config/storage-volume-hints.json` stores only each root's volume-relative
folder, outside SQLite; recovery checks that exact folder on at most 128 mounts
and authenticates its `root.json` identity. It never traverses a drive. Several
matching copies require an explicit choice; malformed, absolute or traversing
hints fail without changing the registry or game files.

The root registry owns creation, physical `root.json` identity, capabilities,
default selection and durable catalogue restoration. This source checks the
registered identity again before using it. It delegates bounded traversal and
reference/alias repair to `Reconciler::scan_root`, then imports the repaired
manifest through the same `core::install::from_manifest` mapper as new installs.
No root-lease nesting occurs: legacy ID minting runs in a separate root lease
after reconciliation. Every read/write path is validated as a real directory
inside that root; symlink games and metadata are refused by the installer layer.

Each manifest carries a UUID in `ids["caskade-install"]`, independent of root
path and relative directory. New installs mint it in the installer pipeline;
legacy manifests acquire it once during adoption. Copies with a duplicate
install ID require a directory choice rather than silently taking over a live
entry or alias. Root mount changes update the root registry; folder/root moves
reconcile before database mapping. The transactional store preserves canonical
game IDs, entry IDs, source identity, user metadata and every actual session.

Managed entries contain `(root_id, relative_dir)` and relative launch/prefix
paths. Aliases retain their existing stable IDs; machine-local runner paths are
resolved by the launch service. A normal discovery refresh cannot replace a
managed install with an external absolute path. A managed storage import may
update its own authoritative relative location after repair.

`StorageRootReport` carries per-game status, imported game/entry IDs, warnings,
errors and typed reconcile questions for the storage/reconcile UI. Unresolved
legacy or fragile installs with one authoritative location remain visible as
Broken entries for explicit repair. Ambiguous physical copies are left alone.
Offline roots, malformed manifests, cancellation and depth-limit scans never
erase history or falsely mark unread entries absent. Confirmed removed folders
become Partial; the game and its sessions remain in the library.

Tests use private generated roots and the authored installer manifest schema.
They cover durable-catalogue and database deletion/rebuild, legacy UUID
persistence, automatic and explicit mount changes,
Explorer renames with INI rewrites, alias retargeting and movable relative Proton
prefixes, copied IDs, unresolved legacy manifests, offline/cancelled/limited
scans, removed folders and symlink refusal. Native Windows junction/drive-letter
and macOS mount parity belong to the deferred native checklist on #22.
