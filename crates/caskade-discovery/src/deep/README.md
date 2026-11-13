Deep scans are disabled by default. `Config.deep` enables custom folders,
removable drives, portable games, arbitrary home Wine prefixes, or full disk.
The `Deep` source exposes their games through the regular scan and merge API.

`scan_with_options(env, ctx, options, progress)` also returns managed Caskade
roots, which the built-in source reports as warnings. A valid `.caskade/root.json` stops descent;
its games are not turned into external games. `watch_paths(env, config)` returns
the roots selected by the configuration, including custom folders outside home.

Limits apply separately to each enabled scan kind and span all its roots.
Defaults are 2 seconds, 20,000 entries and depth 8. The builtin divides its source
time budget between enabled kinds. A timeout retains completed candidates and
adds a warning; cancellation returns `SourceError::Cancelled`. Progress includes
kind, root, visited entries, candidate count, and completion. Checks occur between
directory entries, around classification and launcher readers, and on every
AppImage metadata read/seek. Individual operating-system I/O calls cannot be
preempted. There are hard limits on depth, entries, roots, and retained path bytes.

Walks use capability-scoped directory handles and skip links and special files.
The inventory classifier supplies game evidence. Launch candidates must have a
bounded PE/ELF/Mach-O header or a conventional start-script header; the installer's
deny-list excludes installers, uninstallers and support programs. An optional
architecture hint rejects incompatible PE/ELF candidates. Equal launch ranks
remain `Maybe` with explicit alternatives. Full-disk results are always `Maybe`,
and system, cache, build and dependency directories are excluded.

Portable AppImages reuse the installer's bounded metadata reader. Embedded game
categories establish game evidence; the image is never mounted, extracted, or
executed. Separate images have separate launch paths and no shared install dir.
Removable drives reuse the Steam library reader, Xbox marker/config reader, and
Wine prefix reader. Other game folders are classified from their inventories.
No scan reads account credentials, invokes a launcher, changes ACLs, or writes
icons or source metadata.
