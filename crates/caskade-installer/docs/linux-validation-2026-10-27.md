# Linux installer validation — 27 October 2026

The user decision on 27 October is to close implementation issues on Linux
evidence and record unavailable native Windows/macOS checks on #22. This report
is evidence for that closure round, not a claim of native Windows/macOS results.
#22 remains open; #61 still depends on application storage integration #54.
The I5 application issues #51–#55, #57 and #59 are outside this closure batch.

## Validated revisions

- Starting origin/master: `dbf30102246d6a11f9c5bd3dd65eb0950f72033a`.
- Cargo checks and native profile used that installer tree:
  `152f800bf7de5703461fa09387d75b35a7f6832a`.
- Master repair: `3aed1ce6a138c7b0e1998807c68c8e8a610a0b4b`. Only the separate Wine
  helper changed: `flock --close` prevents its persistent wineserver inheriting
  the heavy-lane descriptor. The six-case Wine matrix passed after the repair.
  No Rust installer implementation changed during this validation round.

Commands ran serially under the machine's heavy lane. The native profile used
an isolated Xvfb display, the verified official 7-Zip 26.05 console, and `/dev/shm`
as an executable second filesystem with more than 4 GiB free. Wine helper tests
used their own prefix/XDG directories and did not alter the user's Wine prefix.

## Results and exact commands

| Check | Result |
| --- | --- |
| Linux all-feature build | PASS |
| nextest `ci` | 502 passed, 23 skipped |
| nextest `native` | 519 passed, 1 skipped, 368.905 seconds |
| Additional Wine T7 matrix | all 6 fixtures passed |
| xtask tests | 21 passed |
| Fixture stale/hash/size checks | PASS: 258 outputs, 57,655,431 / 80,000,000 bytes; each binary ≤ 3,000,000 bytes |
| Linux strict all-target Clippy | PASS |
| Windows GNU strict all-target cross-Clippy | PASS |
| Apple Darwin strict all-target cross-Clippy, without fetch | PASS |

```sh
flock /tmp/caskade-heavy.lock cargo build -p caskade-installer --all-features --locked
flock /tmp/caskade-heavy.lock cargo nextest run -p caskade-installer --profile ci --all-features --locked
CASKADE_TEST_NATIVE=1 CASKADE_TEST_WINE=1 CASKADE_TEST_MOVE_ROOT=/dev/shm \
CASKADE_7ZZ=<verified-7zz-path> INSTA_UPDATE=no \
flock /tmp/caskade-heavy.lock xvfb-run -a cargo nextest run \
  -p caskade-installer --profile native --run-ignored all --all-features --locked
# The helper acquires/releases flock --close around each Wine command:
xvfb-run -a python3 crates/caskade-installer/tests/native_windows_support/wine.py
flock /tmp/caskade-heavy.lock cargo test -p xtask --locked
flock /tmp/caskade-heavy.lock cargo run -p xtask --locked -- fixtures check
flock /tmp/caskade-heavy.lock cargo clippy -p caskade-installer --all-features --all-targets --locked -- -D warnings
flock /tmp/caskade-heavy.lock cargo clippy -p caskade-installer --all-features --all-targets --locked --target x86_64-pc-windows-gnu -- -D warnings
flock /tmp/caskade-heavy.lock cargo clippy -p caskade-installer --all-targets --locked \
  --target x86_64-apple-darwin --no-default-features \
  --features windows-formats,linux-formats,macos-formats,disc,cli -- -D warnings
```

The fixture command uses Cargo's `--locked` before the xtask argument separator;
passing it after `fixtures check` is an invalid xtask argument. That invocation
mistake was corrected and the complete fixture check reran successfully.

## Native Linux proof

The native profile included real execution, beyond the normal fixture/fake tests:

- Inno and NSIS installed under cached umu/Proton, launched, passed registry
  self-checks and ran their own uninstallers (55.487 s and 52.171 s respectively).
- An Aliased Proton installation moved to a different filesystem, launched and
  self-checked without rewriting, then ran its own uninstaller (73.181 s).
- Debian apt and Fedora dnf delegates installed, launched the fixture and
  uninstalled in disposable containers (4.493 s and 9.217 s).
- Flatpak installed/uninstalled in an isolated user installation and launched
  inside its real sandbox, restoring private user references (4.179 s / 4.649 s).
- The SHA-verified sidecar ran RAR/multipart/disc/fallback and adversarial cases.
- A private real Wine registry survived external rename → scan/reconcile → CLI
  managed move, with the destination registry and manifest verified.

The extra Wine matrix covered Inno 5.6.1 / 6.0.5, NSIS 2.51 / 3.11 and embedded /
external-cab MSI minis. Each installed to a spaced/non-ASCII path, matched its
shared golden tree, created the expected Uninstall registration without shortcuts,
and removed payload, custom registry values and Uninstall registrations through
its own uninstaller. These are Wine results, not native Windows results.

The native profile intentionally excludes the separate nightly performance
binary and explicit live-response refresh test; it is not a fresh full T10 or
continuous-fuzz result. Existing issue evidence for those workloads is retained.
Windows/macOS execution, the Steam Deck checklist and hand-checked preset versions
remain in the consolidated deferred-native checklist on #22.

Machine-local raw logs/JUnit and receipts are recorded under
`.git/coord/validation-20261027/`; the commands and counts above are the durable
reviewable summary. The failed private Wine prefix was stopped and removed after
its logs were preserved; the repaired helper cleaned its successful prefix.
