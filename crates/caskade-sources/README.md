# caskade-sources

Discovery-backed `LibrarySource` providers and the D5 library importer. This
crate contains integration policy; discovery remains a read-only scanner and
core owns the domain, SQLite transactions, process tracking and hook bus.

```rust,no_run
use caskade_core::{bus::Bus, db::LibraryStore, settings::SettingsService};
use caskade_discovery::{Cancel, Config, HostEnv};

# async fn example(store: LibraryStore, settings: SettingsService) -> Result<(), Box<dyn std::error::Error>> {
let bus = Bus::default();
let config = Config::default();
caskade_sources::register_settings(&settings)?;
let sources = caskade_sources::register(&bus, store, HostEnv::live(), &config);
let options = caskade_sources::import_options(&settings).await?;
let outcome = sources.scan(&config, &options, &Cancel::new()).await?;
// Show outcome.import.review rather than importing Maybe results implicitly.
let watch = sources.watch(config, options, Default::default()).await?;
// Keep the handle for the app's lifetime; stop it before closing the DB.
watch.stop().await;
# Ok(()) }
```

`register` registers one bus provider per discovery family. Providers support
bounded async scans; watching goes through the aggregate service. It scans families
together with discovery's budgets, exclusions, identity merging and host gates.
Blocking scans and watcher initialization run away from the async/UI thread.
Use injected `HostEnv`/sources for tests; no global runtime or database exists.

Every candidate passes through `library.import` before persistence. A cancelled
primary promotes an accepted alternative; an entirely cancelled group creates
no game. Maybe groups are returned for explicit review by default. Mapping
preserves source-local keys, store IDs, state, size, external paths, structured
argv/environment, runtime and tracking hints. Built-in Wine/Proton/CrossOver
targets use `Executable + GameEntry.runtime`, which the core launch service
wraps through `PrefixRuntime`. This deliberate plan deviation keeps built-in
runners usable; `Plugin` is reserved for third-party runners in M4.

The store imports each group atomically. Existing source identities retain their
IDs, user metadata and managed root/alias contracts; newly linked groups merge
without losing sessions. Missing records become Partial only when the relevant
source scanned successfully. Disabled or failed sources retain their records;
removals preserve the library and history. Watch deltas update changed groups
and memberships without treating unrelated unchanged entries as absent.

Steam, Lutris and Heroic/Legendary aggregate playtime seeds one finished history
record on first import. The largest group total wins rather than summing
duplicate interfaces. Later identity merges retain the largest aggregate seed and
preserve every actual recorded session. Last-played uses the source timestamp; unknown timestamps
remain epoch zero, avoiding invented recent activity. The seed is marked as an
aggregate in tracking JSON. Later scans never add the same history again. The
`discovery.import` SettingsService section defaults `seedPlaytime` to true;
disabling it on first import also prevents retroactive seeding on refresh.

Committed changes emit library game events, then `library.scan.completed` with
per-source counts. The returned outcome retains source errors, warnings,
interceptor skips, review groups and imported IDs for settings/status views.

`cargo test -p caskade-sources` covers authored Linux/Windows machine rebuilds
(empty DB → scan → independent expected inventory → delete DB → same inventory),
incremental file watching, mapping, interception, history, settings, atomic
rollback and absence handling. All fixture reads are synthetic; launcher files
are never mutated. The optional `sqlite` feature enables discovery's SQLite
launcher readers and the composite rebuild tests; it is enabled by default.
