# Settings service (#110)

Core and adapters register namespaced `SectionDefinition`s with schema version,
JSON Schema and validated defaults. `typed<T>` generates the schema through
schemars. `register_core(paths)` registers `core.general`, `core.appearance`,
`core.launchers`, `core.discovery`, `core.storage`, `core.updates`, the
installer scaffold, `core.onboarding` (first-run setup: done or not, sounds),
`core.metadata` (`online`: background Hasheous and IGDB lookups),
`core.appUpdates` (`check`: Caskade looks for new versions of itself) and
`core.accounts` (`ratingBoard`: the age rating board games show and age
limits are picked in; admins only).
Emulation and theming engines are outside this app-core milestone. Storage
roots themselves are authoritative library records, not duplicated preferences.

`SettingsService::with_bus(store, bus)` emits `SettingsChanged` only after the
library DB commit. The adapter bridges that event to the UI. `get`, `set` and
`reset` return a version/revision/value snapshot. Writes compare-and-set the
exact previous JSON envelope, including across independent service instances;
clients can require their last revision. No-op writes stay silent. Defaults are
returned without writing until the user saves or resets a section.

A migration registered for N must produce N+1. Get migrates older persisted
values atomically, validates the result and emits once; repeated reads reuse the
migrated record. Missing/failed migrations, future versions, invalid schemas or
invalid values leave the old DB value untouched. HTTP/file schema references do
not have a retriever. Values have a 64 KiB budget; the persisted envelope has a
small separate allowance. Validation errors name fields without printing values.

Typed `settings_sections/get/set/reset` commands live in the Tauri adapter and
are included in generated bindings. The frontend `api/settings.ts` maps those
commands; `SettingsProvider` invalidates affected section queries on events.
`useSettingsSection` serializes edits and reads the latest backend revision
before applying each updater, avoiding lost rapid edits. Delayed subscriptions
clean up under StrictMode. Loading/save errors are visible, and reset writes the
registered defaults. No localStorage or implicit mock fallback is used.

General, Appearance, launcher and discovery preferences use this service while
preserving their panel layouts. The cache directory is a read-only backend
summary. Discovery scan jobs/review are #101; storage root CRUD/picker/moves are
#2. Their controls and sections build on this registry instead of creating a
second settings store. Emulator functionality remains outside M0–M2.
