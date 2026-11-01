# Library discovery

The panel uses persisted settings and `DiscoveryProvider`; native commands and
events come from generated bindings. Device checks use the backend OS profile,
not the browser. There is no production fixture fallback.

The controller owns startup reconciliation, scheduled refresh, cancellation and
incremental watching. Launcher switches select source IDs; OS switches select
the supported source groups for that OS. Deep options map to the discovery
configuration, with absolute custom paths and all deep scanning off by default.
Canonical duplicate merging remains enabled. Launcher history import is a
separate `discovery.import` setting. No inert prototype controls remain visible.

Scan phases and completed source reports show counts, warnings and errors.
`Maybe` results remain in a server-held review list. Bulk decisions send only
retained IDs and the snapshot revision; stale or invented IDs are rejected.
Accept and ignore decisions persist as bounded source/key pairs. Approvals do
not rewrite classifier confidence or evidence, and accepting a subset does not
mark unrelated library records absent. Ignored records can be allowed again.

Controller tests use injected fixture sources and an in-memory SQLite library.
The IPC integration test covers commands, typed events, accept/ignore, forgery,
stale decisions and a second scan. Vitest covers the generated adapter and
visible settings/actions. Native notification delivery and hardware navigation
remain on the app-core deferred checklist in issue #22.

Hidden games are separate from ignored records: a card's actions hide the
game itself (`Game.hidden`), whatever source found it, and keep its entries
and playtime. Re-imports match the same source identities and leave it hidden,
and a merge with a duplicate keeps it hidden. The Hidden games section lists
them and shows one again.
