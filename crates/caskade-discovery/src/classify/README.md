# Game classifier v1 (#79)

`classify_inventory(metadata, relative_paths, config)` is a pure scoring entry
point for OS records and folder inventories. `classify_path(env, host, path,
metadata, config)` first reads a bounded tree, then uses installer discovery to
choose a launch target. All host capabilities are injected; only the CLI calls
HostCaps::detect. Fixture CLI runs use an explicit x86_64 profile and never read
live launcher directories.

Results contain `score`, `verdict` (`likely`, `maybe`, `not_a_game`), typed
`Evidence`, an optional launch target and warnings. A classifier result alone
never claims Certain. Maybe entries belong in review, not automatic import.

Default contributions:

| Evidence | Score |
| --- | ---: |
| Engine fingerprint (all patterns in a rule) | +70 |
| Store SDK or metadata fingerprint | +35 |
| Explicit Game category | +65 |
| Game publisher | +15 |
| Non-game publisher | -25 |
| Graphics/input libraries | +8 |
| Inside a caller-supplied game folder | +10 |
| Launcher/emulator/redistributable/driver/SDK/tool identity | -100 and exclusion cap |

Multiple engine layouts and multiple SDKs do not stack their scores. Every
matching rule and file still appears in the evidence. Publisher names are weak
signals, not hard identity decisions. Installer `redists.toml` and
`exe-denylist.toml` are embedded directly, avoiding divergent copies. A game's
nested redistributable folder does not make the parent game a redistributable.
A driver game's title is not confused with a graphics-driver package. An
explicit Game category can use a launcher program without becoming the launcher.

Stable exclusion tags include `category.launcher`, `category.emulator`,
`category.redist`, `category.driver`, `category.sdk`, `category.tool`.
The first three are consumed by the merge engine. Excluded identities stay below
Maybe even if they contain game-engine files or misleading Game metadata.
Launcher URI hints never establish shared game identity without a validated
scheme-specific game-ID parser. Generic `Launcher.exe` is not an identity.

Defaults: Maybe >=20; Likely >=60; 20,000 tree entries; depth 12. Configuration
requires 0 <= Maybe < Likely <=1000, 1..=1,000,000 entries and depth <=128.
The walker counts skipped entries too, sorts only bounded directory batches,
ignores symlinks/special files, and never writes icons or metadata. Relative
inventory paths cannot contain traversal, NUL, drive prefixes or absolute roots.
Windows-style game-folder comparisons ignore case; Unix paths retain it.

The 265-row original synthetic corpus includes 143 uninstall records and 122
folder inventories, 132 game labels and 133 non-game labels. At the default
thresholds it reports 100% Likely precision, 87.9% Likely recall, and 100% recall
when Maybe review entries are included. These numbers describe the authored
synthetic corpus; they are not a claim about a live user's software collection.
Every redistributable, driver and SDK row remains below Maybe, including rows
with misleading engine/SDK/category evidence. Snapshot scores make rule changes
reviewable. Filesystem tests check read-only roots, outside symlink sentinels,
entry/depth limits, existing launch targets and the absence of generated icons.

CLI:

```sh
caskade-discovery classify /games/example --json
caskade-discovery classify /games/example --maybe-score 25 --likely-score 70
caskade-discovery classify /games/Original --root /path/to/scenario --json
```

All fingerprint, publisher and launcher data is original Caskade data derived
from format filenames and supplied design requirements. No third-party
implementation or corpus was ported.
