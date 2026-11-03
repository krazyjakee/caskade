# Discovery hardening evidence (#103)

## Synthetic performance

`performance.rs` generates 1,000 Steam games over ten libraries at test time, on each Linux, Windows and macOS HostEnv profile. Existing authored D1/D2/D4 launcher fixtures supply the additional applicable launchers (18 launcher source families across the three profiles, including Bottles, Minigalaxy, Faugus and itch). The Mac helpers are shared with the #96/#97 tests. No binary, large machine tree or benchmark userdata is committed.

Run serially in release mode:

```sh
CASKADE_DISCOVERY_PERF_OUTPUT=target/discovery-performance flock /tmp/caskade-heavy.lock cargo test -p caskade-discovery --all-features --release --test performance -- --ignored --nocapture --test-threads=1
```

Generation/build are excluded from the measured interval. The public default full scan, including launcher detection, source execution and merging, must finish strictly below two seconds **per OS-profile machine**. The test verifies all 1,000 generated IDs survive without cross-group duplication, each launcher family contributes a record, no source fails, and the report invariants hold. Timing the three different machines is not a single combined budget. JSON measurements include source counts, group counts, UTC completion time and optional CI commit SHA.

### One-source rescan

`watch::installing_and_removing_a_game_each_produce_one_delta` times one Steam rescan after a new manifest appears, over the same `steam-linux-shared` fixture on every host, and logs it with five unchanged rescans before it. The work is the same everywhere, so the spread is the host's file system: GitHub-hosted runners measured 24.6 ms on Linux, 52.3 ms on macOS and 210 ms and 243 ms on Windows (ci.yml runs 41634212530 and 41623171279, 3 Nov 2026), where every file open goes through NTFS and Defender's real-time scan. Windows stays there in steady state: five unchanged rescans took 198, 185, 175, 164 and 152 ms, then the measured one 159 ms (run 41640251814). The budget is 200 ms, and 400 ms (about twice Windows' steady state) on Windows.

## Aggregate-only live invariants

Live reads are explicit and ignored in the hermetic default suite:

```sh
CASKADE_DISCOVERY_LIVE=1 CASKADE_DISCOVERY_LIVE_OUTPUT=target/discovery-live flock /tmp/caskade-heavy.lock cargo test -p caskade-discovery --all-features --test it hardening::live_full_scan -- --ignored --nocapture --test-threads=1
```

The check rejects panics, source failures, empty groups, duplicate global external IDs, launcher/tool/redistributable groups, and nonexistent advertised `Certain` install directories. These are logical-game primary records; identity-only shortcut/uninstall alternatives are routing metadata rather than independent installs. Unknown optional directories are not invented. Output contains only numeric counters and public source-family names: no titles, home paths, URIs, registry keys or account/store IDs. Failures should be reproduced from fresh original synthetic metadata before any report is shared.

## Contributor corpus intake (follow-up, never automatic raw collection)

The contributor runs the allowlisted export locally, with a new private destination:

```sh
caskade-discovery dump --anonymize /private/path/new-discovery-dump
```

Do not share a raw home directory, registry export, launcher database, CLI scan JSON or private diagnostic log. Dump export decodes structured VDF/protobuf/SQLite rather than byte-replacing length-delimited data; it scrubs user/home/account fields, enforces size/privacy limits and emits a confined fixture `machine.toml`, `root/` and `registry/`. The contributor must review the **anonymized output itself** for remaining private values and grant permission before sending it; anonymization does not replace consent or human review. Keep aggregate scan counts separate from any fixture bytes.

After consent, coordinate with the fixture owner (#70, Engineer2): validate the scrubbed output, minimize the failing metadata, reconstruct SQLite from SQL and binary formats from JSON descriptions, record provenance and expected aggregate/logical results, add the deterministic regression, then run `cargo xtask discovery build` and `cargo xtask discovery check` before committing. Add only generated/anonymized approved paths, never an unreviewed uploaded tree. Existing `fixture_dump` round-trip and CLI tests cover export/replay; each actual contributed failure needs its own new regression.

No contributor corpus is fabricated here. Obtaining consented contributor dumps, adding their regressions and observing seven complete green days of fuzz and performance runs remain explicit #71 follow-ups. Windows/macOS native live parity is also tracked there.
