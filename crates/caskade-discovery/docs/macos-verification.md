# macOS discovery verification (#95–#97)

The implementation closes on Linux fixture evidence under the 27 October user
decision. Native macOS execution is deferred on #71; this document is a runner
and real-machine checklist, not a claimed Mac result.

## Portable fixture proof

From the repository root, with the machine's heavy lane:

```sh
flock /tmp/caskade-heavy.lock cargo test -p caskade-discovery --all-features --locked --test it -- macos_apps:: macos_launchers::
```

The #91 CrossOver/Whisky adapter has a separate portable proof:

```sh
flock /tmp/caskade-heavy.lock cargo test -p caskade-discovery --all-features --locked --test it prefixes::crossover_and_whisky_bottles_on_macos
```

The macOS HostEnv fixtures are authored at test time. The bundle suite covers XML
and binary Info.plist, game and non-game categories, launcher bundle IDs,
synthetic App Store receipt markers, ICNS, exact confined executable paths,
broken/malformed apps, symlinks, duplicate copies, cancellation and entry budgets.
The launcher suite covers Steam, Epic, Galaxy, Heroic, Battle.net, Riot and itch,
their combined scan, generated Steam app aliases, and distinct games sharing a
Galaxy/Riot executable. SQLite-backed Galaxy/itch cases require `sqlite`; Linux
and native macOS CI use all features. The Apple cross-Clippy gate omits SQLite
because this Linux machine lacks the Apple SDK for its bundled C build.

Launcher locations:

| Source | macOS metadata |
| --- | --- |
| Steam | `~/Library/Application Support/Steam` |
| Epic | `~/Library/Application Support/Epic/EpicGamesLauncher/Data/Manifests` |
| GOG Galaxy | `/Users/Shared/GOG.com/Galaxy/Storage/galaxy-2.0.db` |
| Heroic | `~/Library/Application Support/heroic` |
| Battle.net | `/Users/Shared/Battle.net/Agent/product.db` |
| Riot | `/Users/Shared/Riot Games/RiotClientInstalls.json` and `Metadata` |
| itch | `~/Library/Application Support/itch/butler.db` |

Launcher executable names come from bounded bundle Info.plist reads. A client
executable cannot escape its bundle. CrossOver/Whisky prefix discovery belongs
to #91; its tests and prefix adapter remain independent of these native paths.

## Runner setup and default proof

Register the repository-scoped `caskade-macos` runner using [ci.md](ci.md).
Provision the pinned Rust toolchain, cargo-nextest and Python; do not share a
user's launcher database as a build cache. The existing discovery workflow's
ordinary macOS matrix job runs all-feature fixture tests and emits JUnit.
No account sign-in, app installation or personal launcher inventory is required
for this job.

Before considering native fixture coverage complete:

- [ ] Run `python3 scripts/discovery-ci.py build`, `clippy`, `test` and `features`
  on the Mac; retain the JUnit and source SHA.
- [ ] Confirm both SQLite-backed launcher fixtures ran (not just the cross gate).
- [ ] Repeat the bundle suite on the runner filesystem and note case/Unicode
  behavior, aliases and symlinks.

## Explicit real-Mac proof

Use a disposable or expressly authorized Mac profile with all of:

- A native macOS Steam game installed and visible in Steam's local manifests.
- A native macOS Epic game installed and visible in its local manifests.
- A categorized Mac App Store game with `Contents/_MASReceipt` present.

No particular commercial title or account is required by the tests. Never place
credentials, actual receipts, private paths, game titles or account data in the
fixture corpus or CI artifacts.

Run the opt-in ignored check only after provisioning those three games:

```sh
CASKADE_DISCOVERY_LIVE=1 CASKADE_TEST_NATIVE=1 python3 scripts/discovery-ci.py live
```

Or select the one test (macOS only):

```sh
CASKADE_DISCOVERY_LIVE=1 flock /tmp/caskade-heavy.lock cargo nextest run -p caskade-discovery --all-features --locked --profile discovery-live --run-ignored only -E 'test(real_mac_steam_epic_and_app_store_game_are_discovered)'
```

The check requires positive counts for Steam, Epic and an App Store-family game,
asserts that the selected sources succeeded, and prints aggregate counts only.
It only describes launch targets; discovery never launches a game. Receipt
presence cannot distinguish an Arcade entitlement from other Mac App Store
acquisitions. Keep the signed receipt contents private.

- [ ] Record the real Mac's OS/architecture, source SHA and aggregate counts.
- [ ] Confirm the generated Steam `.app` alias merges into its Steam group.
- [ ] Inspect the described executable/URI targets locally; verify launch parity
  manually outside the read-only discovery process.
- [ ] Verify current launcher caches and note any format/version changes.
- [ ] In particular, verify GOG Galaxy for Mac accepts the described
  `/command=runGame /gameId=... /path=...` argv. These client flags are shared
  with the Windows implementation and remain unverified on a real Mac; if
  unsupported, use a separately verified native Galaxy URI/command instead.
- [ ] Attach only redacted aggregate/JUnit evidence to #71, then mark its #97
  deferred item complete. Default Linux fixtures do not complete these boxes.
