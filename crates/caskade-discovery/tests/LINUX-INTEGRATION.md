# D1 Linux evidence (#78)

`fixtures/machines/linux-integration` is generated entirely from original,
reviewable metadata. Its `expected.json` is the authored inventory of 20 games
and their alternative launcher records. It combines native/Flatpak Steam with
a shared library, non-Steam shortcuts, native/Flatpak Heroic, Legendary, native/
Flatpak Lutris, desktop proxies and a standalone Flatpak game. Hidden/utility
desktop entries, DLC, Steam tools and Steam/Flatpak runtimes are decoys.

The integration test requires an exact match of all group members, rather than
checking only the count. A source record cannot occur in two groups. The merged
ScanReport snapshot retains alternatives, launch targets, runtime/prefix data,
installation state, play history and evidence. Only measured source durations
are normalized to zero; the fixture clock is fixed.

```sh
cargo test -p caskade-discovery --all-features --test it --locked linux_integration::
cargo run -p xtask --locked -- discovery check
CASKADE_DISCOVERY_LIVE=1 cargo test -p caskade-discovery --all-features \
  --test it --locked linux_live:: -- --ignored --nocapture
```

Live scans require explicit opt-in and run the D1 sources only. The test reports
elapsed time, source counts/warning counts/failure flags, group count and launcher
presence/kind. It never prints game names, paths, identifiers or source diagnostic
text. It checks group validity and duplicate global Steam/GOG identities without
printing a failed identity. No live metadata or scan result is copied to fixtures.
The exhaustive ground truth comes from the synthetic inventory; aggregate live
invariants are not an independent audit of the user's private game library.

The existing discovery-live CI profile automatically includes this ignored test
when a trusted runner job sets `CASKADE_DISCOVERY_LIVE=1`. Runner execution is
tracked on #71; a missing launcher on an otherwise valid runner is not a failure.

Linux dev-box evidence (27 October 2026): 466 ms, 72 merged groups, no source
failures. Raw counts: Steam 17, Steam shortcuts 3, Lutris 50, desktop 28, Flatpak 1;
Heroic/Legendary/Snap 0. Steam and Lutris were detected as native. These are counts
only; no private names, paths or identifiers are stored here.
