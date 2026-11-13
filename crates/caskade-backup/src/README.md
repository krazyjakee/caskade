# App-data backups (#191)

`Backups::new(store, paths)` makes and checks backups of Caskade's own data:
the library database, which holds the games, entries, play sessions,
collections, edits, artwork choices, launch actions and scripts, settings,
local accounts and plugins' stored state. Games' files and saves, and cloud
saves, are not part of it.

## Archive

A backup is one zip, `caskade-backup-<UTC yyyymmdd-hhmmss>.zip`:

- `library/library.sqlite`: the database snapshot.
- `artwork/<hash>/…`: the artwork cache's files for the chosen artwork
  components (`entry.json`, `original.webp`, thumbnails; never `.lock`).
- `plugins/…`, `plugin-data/…`, `themes/…`: those folders under `<data>`,
  regular files only (symlinks aren't followed, names that aren't UTF-8 are
  skipped).
- `manifest.json`, written last: format (`FORMAT`, 1), app version, schema
  version (the snapshot's newest migration), creation time, OS, the
  automatic backup set, each component's file count and size, rows per
  table, the exclusions, and every file's path, size and SHA-256.

`verify(path, cancel, report)` reads an archive back and returns its
manifest. It refuses a newer format (`BackupError::Newer`), and calls an
archive `Damaged` when the manifest is unreadable, a listed file is
missing, its size or SHA-256 differ, an entry isn't listed or appears
twice, a path leaves its component's folder (absolute, `..`, `\`, `:`),
there are more than a million files, or there is no library database.
Entry reads are capped at the size the manifest claims.

## Components

`Component::Library` is always included. The optional ones are `Artwork`
(images that can't be downloaded again: chosen or imported, web games',
executables' icons), `DownloadedArtwork` (IGDB's, which can be fetched
again and can run to gigabytes), `Plugins`, `PluginData` and `Themes`.
An image used both ways counts as the user's.

`plan(&options)` is the dry run: each component's file count and size,
whether it's selected, the exclusions, the estimate, the destination's
free space and whether it exists.

## Snapshot

`VACUUM INTO` copies the live database in one read transaction: writers
carry on, and the copy holds everything committed before it began, the
WAL's included. The copy then loses what `policy` excludes, is vacuumed
again so the deleted rows don't survive in free pages, and must pass
`PRAGMA integrity_check`. Play sessions open at that moment are copied as
open; the launch service's recovery at the next start (on the restored
profile) finishes them as it does after any exit.

`policy::EXCLUDED_TABLES` and `EXCLUDED_SETTINGS` list what is left out and
why. **Every table and settings key is backed up unless it's listed there**,
so a new table needs no backup code. List it only when it holds credentials
or tokens, a cache Caskade fills again, or the state of work under way.
Files kept outside the database need a `Component` (and its folder in
`export::files`).

Never in a backup: credentials (they stay in the OS keychain), web games'
browser data (`<data>/web`), games' files and saves, managed emulators
(`<data>/emulators`), downloaded ROMs (`<data>/romm-cache`), caches
(`<cache>`, `<data>/g` aliases, the compiled-plugin cache and its key),
pending installs and moves, logs, and window state in `<config>`.

## Export

`export(options, progress, cancel)` runs one backup at a time
(`BackupError::Busy` otherwise) and reports `Progress` per phase
(`snapshot`, `writing`, `verifying`). The destination must exist
(`DestinationMissing`, e.g. an unplugged drive), except the default
`<data>/backups`, which is created. Before writing it checks the free space
against the estimate plus a second copy of the database (`NoSpace`).

The snapshot and the archive are written beside the destination's backups
as hidden `.caskade-backup-<id>.partial*` files, the archive synced, read
back through `verify` and compared with the manifest it wrote, and only
then renamed to its final name (never over an existing file). On failure
or cancellation both partial files are removed; after a crash the next
backup to that folder removes them. Earlier backups are never touched.
Writing and checking run on the shared resource queue
(`caskade-jobs`), under a "Backing up" activity.

Tests inject a full disk, a crash before publishing and the free space.

## Sync state

What devices, library sync and copies keep, in a backup:

- **Left out** (`policy.rs`): `paired_devices`, which holds the grants each
  device presents: devices pair again after a restore. `sync_peers`, what
  other devices said they have: the restored library sends everything again
  on its next exchange, and last writer wins sorts it out. The `sync.device`
  setting, this device's endpoint id and name: a restore on another machine
  mustn't take over this one's identity. `shares` and `share_recipients`,
  whose grants are credentials too. The device key never is in the database:
  it's in the OS keychain.
- **Kept**: the change log and its clocks (`sync_changes`, `sync_fields`,
  `sync_seen`) and which local game each other device's game is
  (`sync_game_ids`), so a restored library still knows what it changed and
  when, and matches the same games; and everything synced itself (accounts,
  collections, sessions, save versions' records).
- **Elsewhere**: copies' stores (`<cache>/copies/`) are caches, and presence
  (#240) lives in memory: neither is in a backup.

## Restore (#192)

`restore::inspect` (or `Backups::inspect`) verifies an archive, refuses a
newer format or schema (the manifest's, or a migration in its database this
Caskade doesn't have or has with another checksum), checks the database's
integrity, and reports the components, whether it needs migrating, whether
it was made on another OS, the storage locations whose folders aren't here
and the games found elsewhere whose folders aren't here. It changes nothing.

A restore replaces the profile; it never merges two libraries. It runs in
two steps, so nothing reads or writes the library while it changes:

1. `restore::stage` (`Backups::stage_restore`) extracts the chosen
   components into `<data>/restore/staged`, hashing every file again,
   migrates the staged database to this Caskade's schema, and writes
   `<data>/restore/journal.json`. Artwork goes straight into the cache,
   which is content-addressed, so nothing there is replaced. The running
   profile is otherwise untouched; a cancelled or failed staging leaves no
   journal. `discard_staged` forgets a staged restore.
2. At the next start, before the library opens (`recovery::open_library`
   in the app), `restore::apply_pending` moves the library database (its
   WAL and shared-memory files aside), the chosen `plugins`,
   `plugin-data` and `themes` folders and `<config>/storage-roots.json`
   (which would add this profile's locations back) into
   `<data>/restore/rollback`, and the staged ones into place. Once the
   library opens, `finish` deletes the rollback; if it doesn't,
   `roll_back` puts everything back and the old library opens.

Every move is a rename recorded by the journal's state (`staged`,
`applying`, `applied`, `rollingBack`); a target that wasn't there before is
marked `<name>.absent`, so a rollback removes it. A start that finds an
interrupted apply carries it on, and an interrupted rollback finishes it.
`take_outcome` returns how the last restore ended, once.

Components not chosen stay as they are. Plugins whose files aren't
restored come back disabled and without grants, so they ask again like a
newly installed plugin; with their files they keep them. Credentials are
never in a backup: accounts sign in again through their usual flow.
Restored games keep their ids; storage locations whose drives aren't here
show as offline, as any disconnected location does, and games found
elsewhere wait for the next scan. Nothing moves or overwrites game or
save files.

When the library can't open, its error dialog offers Restore from
backup…, and `caskade --restore <backup>` stages one before starting.

## Settings → Backup and automatic backups (#193)

`core.backups` (`BackupSettings`, admins only like every namespace no
section owns): the folder (empty for `<data>/backups`), the optional
components (artwork, plugins, their files and themes by default; IGDB
artwork off), the schedule (`off`, the default, `daily` or `weekly`) and
how many automatic backups to keep (7). `Backups::set_settings` registers
it.

`back_up_now` uses those settings and is never removed by retention.
`run_due(now)` makes the automatic backup when it is due: an interval after
the last one that worked, so a long gap is made up once, at the next look
(two minutes after start, then every five minutes and whenever the settings
change: `watch`). A failed one is tried again after 30 minutes, 1, 2 and 4
hours, then waits for the next interval; a clock set back (a last backup in
the future) makes one due now. A backup already running makes a due one
wait, uncounted. After an automatic backup verifies, `prune` deletes the
oldest backups of the same set beyond the number to keep: only
`caskade-backup-*.zip` files whose manifest names the set (`setId`, chosen
on the first automatic backup and kept in the `backup.state` setting with
the schedule's times, failures and the last outcome). Manual backups, other
sets' backups, unreadable files and anything else stay. Nothing asks the
user: a missing folder or a full disk becomes the last outcome
(`LastBackup.error`, an `ErrorReport`).

`status()` / `subscribe()` give the running job (`BackupJobKind`, progress
at most every 100 ms per phase), the last outcome, when the next automatic
backup is due, the default folder, a staged restore and how the restore
before this start ended (`set_last_restore`, from `restore::take_outcome`).
`cancel()` stops the job. `inspect_job` and `restore_job` are
`Backups::inspect` and `Backups::stage_restore` as jobs, under a "Checking
a backup" or "Restoring a backup" activity.

In the app, `commands/backup.rs` forwards the status as `BackupChanged`
and supervises `watch`. `backup_status` and `backup_plan` are reads; the
rest are for admins (`permissions::Access::Admin`). `backup_restart`
applies a staged restore by restarting, refused while a game runs or an
install, move or update is under way. `backup_reveal` shows only files
named like backups in the backup folder or the last backup's.

The webview's Settings → Backup tab (`views/settings/backup/`, admins
only) chooses the folder through `usePathPicker` (or a drop), the
components with their sizes from the plan, Back up now with progress,
Cancel and Show in folder, the schedule and retention, and restores: a
backup chosen or dropped is inspected and previewed (components, an older
Caskade's, another OS's, locations and games not found here) before
Restore and restart. `api/backup.ts` falls back to an in-memory fixture
(`backupFixtures.ts`) in the browser preview.

## On another device (#243)

`backup::peers::PeerBackups` keeps backups on another of the
user's devices, so losing a drive doesn't lose the library, without a cloud.

- **Sending.** `core.backups.keepOnDevice` names a paired device (Settings →
  Backup → Also keep backups on). Every sync exchange with it offers this
  device's newest verified backups (as many as `keep`) in the first-round note
  of the blob exchange (`sync::blobs`, `BlobKind::announce`). That device asks
  for the ones it lacks, and they travel as `backup` blobs on the granted sync
  session. A backup made while the device is away is offered again until it has
  it.
- **Keeping.** The holder keeps each device's backups apart from its own,
  content-addressed under `<data>/peer-backups/blobs/`, with
  `<data>/peer-backups/index.json` naming each: the device it came from (and
  that device's name), its file name, size and time. It keeps the newest `keep`
  per device, and each blob is checked against its SHA-256 before it's kept.
  Two exchanges with a device can run at once and both bring the same backup:
  arrivals are kept one at a time, so the second finds the first in the index
  instead of collecting it as unasked for.
- **Restoring.** Every exchange also lists what the holder keeps
  (`PeerBackups::held`). A fresh install paired with the holder sees its
  backups of every device in Settings → Backup → Restore → On your other
  devices (`backups_on_devices`). Restore… fetches one here
  (`backup_fetch_from_device`, which asks the holder to sync until it arrives)
  and opens the usual preview and staged restore. `PeerBackups::fetch` asks for
  it from the call until it arrives or the fetch gives up, and keeps it from
  collection while the app runs, so the restore can read it. A `sync_now` that
  comes while an exchange with that device runs waits for it and runs another,
  so an exchange starts after the request. Both commands are admin-only,
  like restoring.
