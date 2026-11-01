# Discovery identity and merge contract

`merge::merge(env, entries, config)` returns `GameGroup`s. The orchestrator calls
it after collecting source results; `Config.source_priority`, exclusions and
minimum confidence are forwarded. It never launches processes or writes files.

Entries connect through typed external IDs, recognised launcher links, equal
canonical install paths, or directory containment of a launch executable.
Names never connect entries. Sorted component keys keep path matching at
O(n log n), with union-find for transitive identities.

The default metadata order is launcher sources, Game Explorer/GameUX,
uninstall/registry/Flatpak/Snap, shortcuts/desktop entries, then deep scans.
`source_priority` lists exact SourceIds to prefer ahead of those defaults.
Source/key and path/title break ties deterministically. The primary entry gets
missing size, icon, artwork kinds, install folder, history, uninstall, tracking
and Windows prefix/runner metadata from alternatives. Existing values win.
Alternatives remain available in `entries`; confidence is the strongest evidence
in the group.

`uri::identities` recognises the plan's launcher schemes. `target_identities`
reads exact URI argv tokens and Legendary's `launch <app>` form; it never parses
or runs a shell command. Heroic's legacy path and current query forms work.
Epic app names can match full namespace/catalog/app identities. itch cave ids,
Lutris database rows and Heroic sideload ids have separate launcher namespaces;
they are not store game ids. Local database keys are scoped to native versus
Flatpak launcher installations, including raw URI fallbacks beside an explicit
Flatpak target. Global Steam/GOG/Epic identities still connect the same game
across launcher installations.

Sources should call `uri::mark_proxy(&mut entry)` for launcher-generated links
that only contribute identity. The stable rule is `identity.proxy`; Steam's
`steam.shortcut.proxy` is also understood. Such entries never supply path anchors,
so a launcher executable or broad working directory cannot join unrelated games.
A component containing only proxies is dropped; a proxy attached to an owning
game remains an alternative. Its IDs enrich the owner, while its metadata and
proxy-role marker do not. A real owner remains primary even when a proxy source
is listed first in the configured preference. Real native/proton Steam non-Steam games remain
ordinary entries. `uri::is_proxy` names every proxy kind. The library import
(`caskade-sources`) stores only owners as launch entries, and folds proxy rows
saved by earlier versions, with their sessions, into the primary entry.

`identity::canonical_path` uses captured HostEnv volumes and fixture resolution,
canonical filesystem links/junctions and existing ancestors, the installer's
plain Windows/path-variant code, explicit prefix drives and Wine Z:, file URIs,
`/run/host`, and explicit Flatpak virtual XDG mappings. Prefix drives other than
C: require a dosdevices mapping. `PathContext` permits callers to provide the
prefix/Flatpak identity directly. Non-Unicode names keep lossless component
identity. Missing or unreadable path evidence never invents host drive mappings;
valid external-ID evidence can still group those entries. Comparison paths are
never writable capabilities.

Built-in exclusions remove Caskade-managed folders (`.caskade/install.json`),
known launcher/emulator/redistributable programs and classifier category evidence.
Classifier tags are `category.launcher`, `category.emulator`, `category.redist`,
`category.driver`, `category.sdk` and `category.tool`. User path globs (slash or
Windows separators), `(source,key)` exclusions and external IDs apply to the
whole logical group, so another shortcut cannot resurrect an excluded game.
Invalid globs return a configuration error. Proxy wrapper paths are not game
paths for exclusion matching.

Tests cover every §8.1 example, five-source Steam/GOG groups, distinct Doom
folders, real Steam proxy fixtures, configured metadata preference, exclusions,
all protocol families, volume case policy, Wine/Flatpak paths, links and hostile
URI text. Native Windows junctions/registry and macOS volume behaviour remain on
#71's deferred-native checklist under the 27 October Linux-evidence decision.

Launcher-owned uninstall aliases use Evidence::Rule `alias.launcher_owned` (score 0). They attach IDs and only missing uninstall data to a real game; their wrapper title, path, icon, runtime and category evidence never enrich the owner. Orphan alias groups are dropped. Shortcut-only identity proxies continue to use uri::mark_proxy.

ID-less launcher-owned aliases can attach by an exact canonical owner install directory. Alias paths never participate in parent containment. An alias with explicit IDs uses those IDs instead, so an inconsistent install path cannot bridge two owners.
