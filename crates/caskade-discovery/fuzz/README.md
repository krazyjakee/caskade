# Discovery hardening and robustness (#67, #103)

Ten independently registered libFuzzer targets cover binary VDF/appinfo, text VDF, .lnk, .url, Desktop Entries, XML/binary plist, Battle.net product.db protobuf, SQLite, Wine/.reg text and actual Epic manifest source parsing. `run.py --list-targets` reads Cargo.toml, so future registered targets automatically enter the nightly matrix even before a custom seed set exists. No harness executes an application or reads a live launcher. Epic and SQLite harnesses use private temporary synthetic files.

Build every target under the machine heavy lane:

```sh
flock /tmp/caskade-heavy.lock cargo +nightly fuzz build --fuzz-dir crates/caskade-discovery/fuzz --target-dir crates/caskade-discovery/fuzz/target
```

Run one bounded target (ASan is enabled by cargo-fuzz):

```sh
python3 crates/caskade-discovery/fuzz/run.py epic --seconds 600
```

The driver builds under `/tmp/caskade-heavy.lock`, then runs single-core at nice 19 under `/tmp/caskade-fuzz.lock`. It seeds committed synthetic metadata and prefixes, sets input/node/parser budgets, a 64-KiB input ceiling, ten-second per-input timeout and 1-GiB RSS ceiling. Every run retains its original log and JSON exit/time/execution-count evidence under ignored logs/. SQLite disables the ASan ODR check as cargo-fuzz does, to avoid amalgamation false positives; memory instrumentation stays enabled. `--skip-build` is only for a previously built matching source tree.

`.gitea/workflows/discovery-nightly.yml` runs only the trusted repository master, on schedule or manual dispatch, with full-SHA actions. All ten targets receive 600 seconds, serially. Tool caches are read-only. Discovery mutations use a separate namespaced writable corpus volume and the existing bounded corpus-retention helper, never executable-tool caches. Crashes and mutated corpora are uploaded for 14 days; compact measured JSON and performance evidence are kept for 30.

A minimized failure must become an ordinary deterministic parser/source regression, with a reviewable original source description and fixture-owner consent. Never add raw launcher userdata, credentials, personal shortcuts or a binary copied from a user profile. See `../tests/HARDENING.md` for aggregate live checks and anonymized contributor intake.

## Auditing seven actual green days

Configuration and short local runs are not seven elapsed nightly days. Download the `discovery-evidence-<target>-<run>` and `discovery-performance-<run>` artifacts for the trusted master runs, retain their run IDs/provenance, and unpack them under one directory. Then run:

```sh
python3 crates/caskade-discovery/fuzz/history.py downloaded-evidence --required-days 7
```

The validator requires every current target to have a successful measured >=600-second run with positive executions, plus all three OS-profile 1,000-game scans strictly below 2,000ms, on seven consecutive UTC dates. Smoke runs, missing targets, failed budgets, duplicate dates and one green day cannot satisfy the week. Unit tests use explicitly fabricated validator inputs, never nightly evidence:

```sh
python3 crates/caskade-discovery/fuzz/test_history.py
```

Seven real nightly days and contributor collection are deferred follow-ups on #71 by the primary’s Linux-evidence decision. No elapsed-week or contributor-data claim is made by this implementation.
