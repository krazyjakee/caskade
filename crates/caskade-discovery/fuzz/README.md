# Discovery hardening and robustness (#67, #103)

Ten independently registered libFuzzer targets cover binary VDF/appinfo, text VDF, .lnk, .url, Desktop Entries, XML/binary plist, Battle.net product.db protobuf, SQLite, Wine/.reg text and actual Epic manifest source parsing. `run.py --list-targets` reads Cargo.toml, so future registered targets are listed automatically even before a custom seed set exists. No harness executes an application or reads a live launcher. Epic and SQLite harnesses use private temporary synthetic files.

Build every target under the machine heavy lane:

```sh
flock /tmp/caskade-heavy.lock cargo +nightly fuzz build --fuzz-dir crates/caskade-discovery/fuzz --target-dir crates/caskade-discovery/fuzz/target
```

Run one bounded target (ASan is enabled by cargo-fuzz):

```sh
python3 crates/caskade-discovery/fuzz/run.py epic --seconds 600
```

The driver builds under `/tmp/caskade-heavy.lock`, then runs single-core at nice 19 under `/tmp/caskade-fuzz.lock`. It seeds committed synthetic metadata and prefixes, sets input/node/parser budgets, a 64-KiB input ceiling, ten-second per-input timeout and 1-GiB RSS ceiling. Every run retains its original log and JSON exit/time/execution-count evidence under ignored logs/. SQLite disables the ASan ODR check as cargo-fuzz does, to avoid amalgamation false positives; memory instrumentation stays enabled. `--skip-build` is only for a previously built matching source tree.

CI doesn't run these targets: the nightly fuzz and performance workflow was removed when CI became one workflow (`.github/workflows/ci.yml`, #35). Run them by hand as above, ten minutes per target, and keep `crates/caskade-discovery/fuzz/logs/` for evidence. The 1,000-game performance budget runs with `flock /tmp/caskade-heavy.lock cargo test -p caskade-discovery --all-features --locked --release --test performance -- --ignored --nocapture --test-threads=1` (`CASKADE_DISCOVERY_PERF_OUTPUT=<dir>` keeps its measurements).

A minimized failure must become an ordinary deterministic parser/source regression, with a reviewable original source description and fixture-owner consent. Never add raw launcher userdata, credentials, personal shortcuts or a binary copied from a user profile. See `../tests/HARDENING.md` for aggregate live checks and anonymized contributor intake.

## Auditing seven actual green days

Configuration and short local runs are not seven elapsed days. With no nightly CI, the evidence comes from a full daily run by hand on a clean master checkout: every target's `logs/<target>/<stamp>/result.json` and the performance measurements, kept with the commit they ran on, gathered under one directory. Then run:

```sh
python3 crates/caskade-discovery/fuzz/history.py downloaded-evidence --required-days 7
```

The validator requires every current target to have a successful measured >=600-second run with positive executions, plus all three OS-profile 1,000-game scans strictly below 2,000ms, on seven consecutive UTC dates. Smoke runs, missing targets, failed budgets, duplicate dates and one green day cannot satisfy the week. Unit tests use explicitly fabricated validator inputs, never real evidence:

```sh
python3 crates/caskade-discovery/fuzz/test_history.py
```

Seven real green days and contributor collection are deferred follow-ups on #71 by the primary’s Linux-evidence decision. No elapsed-week or contributor-data claim is made by this implementation.
