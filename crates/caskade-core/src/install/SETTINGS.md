# Installer preferences

`core.installer` version 3 is registered by `SettingsService::register_core`.
Its JSON Schema supplies the Settings → Installer controls, labels, enum choices
and numeric limits. The previous empty version 1 section migrates to defaults;
version 2 gains `downloadExecution: ask`. Invalid persisted documents remain
errors rather than silently resetting.

| Preference | Default | Effect |
| --- | --- | --- |
| `windowsMethod` | `run_installer` | Rank native Windows installer runs or extraction first. `run_installer` tries a silent run, then extraction, then an interactive run. |
| `execution` | `allow` | Allow, ask before, or reject plans that execute installer code. |
| `downloadExecution` | `ask` | The same for games downloaded from a link (deep links, pasted or dropped URLs, magnet links, torrents, updates); the stricter of the two applies. A static strategy that fails falls back to installer code only under `allow`; otherwise the job asks. |
| `askBelowConfidence` | `0.5` | Pause for a choice below this plan confidence. |
| `language` | empty | Let the installer choose, or request its supported language switch. |
| `letInstallersCreateShortcuts` | `false` | Translate to each handler's shortcut switches; unsupported presets produce warnings. |
| `compatibilityRunner` | `proton` | umu with UMU-Proton, detected GE-Proton/Steam Proton, or detected system Wine. |
| `sourceHandling` | `keep` | Keep, trash or delete an unchanged selected local source after managed installation and DB persistence. |
| `keepDownloads` | `false` | Retain successful URL-job downloads; failures retain retry data. |
| `concurrentDownloads` | `2` | Limit downloads to 1–8; lowering drains active downloads and increasing wakes waiters. |

The application injects the shared settings service into `InstallService`.
Each run reads a validated snapshot before planning/downloading and converts it
to the pipeline's `InstallerConfig`/`Policy`. A running job keeps that snapshot;
another job or retry reads the current preferences. Server-owned retained
alternatives are checked against the newly allowed plan set on retry.

Runner selection filters the job's explicit `HostCaps`, using existing runner
and Proton-build detection. Maintenance operations use raw capabilities and the
manifest's recorded prefix family. Managed entries retain Wine/umu identity,
relative prefix paths and no absolute machine runner path. A per-launch resolver
reads current settings for build paths while preserving an existing prefix's
family; external discovery entries retain their own runner hints. Missing
selected builds fail explicitly. umu can bootstrap through the existing bounded
runtime downloader. Native Windows installs use the native runtime; macOS uses
Wine. A root without executable/symlink support cannot host the current
game-local prefix and is refused before committing it.

Source cleanup runs only after a successful managed install is saved to the DB.
It excludes URL inputs, hand-offs and failed/questioned/cancelled installs.
Canonical source/root/alias-store overlap checks and bounded before/after content
snapshots preserve changed sources, links, special files or managed metadata.
Cleanup failure keeps the successful game and reports `source_retained`.
Trash uses the OS recycle-bin API; it never falls back to permanent deletion.

`tests/installer_settings.rs` drives real queued plans, extraction and local HTTP
downloads against private fixtures. Unit tests cover live queue limits, retained
changed sources and prefix-family mapping. The opt-in Linux trash test runs in
a subprocess with private HOME/XDG directories:

```sh
flock /tmp/caskade-heavy.lock env CASKADE_TEST_NATIVE=1 cargo test -p caskade-core --lib install::source::tests::native_trash_uses_an_isolated_os_trash --locked -- --ignored --exact
```
