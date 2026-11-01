# Installer robustness targets

The isolated cargo-fuzz workspace has one target per untrusted-input parser:

| Target | Entry point |
|---|---|
| `probe` | whole probe pipeline (`ProbeEngine::with_builtin_formats`) |
| `archive_listing` | native archive listing parsers |
| `content_disposition` | `fetch::disposition_name` |
| `pe` | PE header/resource reader |
| `makeself_header` | `formats::linux::makeself::parse_header` |
| `appimage` | AppImage header + squashfs metadata (`formats::linux::appimage::read_metadata`) |
| `inno` | `formats::windows::inno::inspect_bytes` |
| `msi` | `formats::windows::msi::inspect_reader` |

Format work adds its own target here. The normal product workspace never
builds these binaries.

Run from the repository root:

```sh
just fuzz probe
just fuzz archive_listing
just fuzz content_disposition
# Equivalent without the working-tree justfile recipe:
python3 crates/caskade-installer/fuzz/run.py probe --seconds 600
```

`run.py` builds with `flock /tmp/caskade-heavy.lock cargo +nightly fuzz build`.
It releases that lock, then executes the instrumented binary under
`flock /tmp/caskade-fuzz.lock nice -n 19` with `-jobs=1 -workers=1`.
Only one long robustness run may use that lock. Normal builds can proceed.
`--build-only` prepares a binary without starting a run; `--skip-build` reuses
that binary when a batch has already built every target. Rebuild after parser
changes. `--seconds` defaults to 600. AddressSanitizer, debug assertions and
overflow checks use cargo-fuzz defaults. Runs stop on a report, a 30-second
single-input timeout, or the 1 GiB instrumented-process memory limit.

`seed.py` copies the project-owned fixture corpus into the ignored writable
`corpus/<target>/` directories and adds 25/50/75/99-percent prefixes. The header
parser seeds derive valid Content-Disposition fields from fixture filenames,
plus quoted/Unicode/malformed variants. Seeds have content-addressed names and
existing mutations are retained. The archive target invokes each supported
native listing parser; it never extracts or launches the optional sidecar.
Production metadata/entry/decoder limits are exercised with smaller bounded
budgets for useful iteration speed. Corpus, artifacts and logs are ignored;
the standalone lockfile makes target builds reproducible.

A run records `logs/<target>/<UTC stamp>/driver.log`, worker `fuzz-0.log`, and
`result.txt`. Preserve these on the self-hosted runner (#22), along with the
persistent corpus. A successful ten-minute local run provides local evidence;
it does not substitute for #21's literal self-hosted-runner acceptance gate.

## Converting a report into a regression

1. Preserve the reported input and worker log. Stop that target and identify its
   parser owner; do not apply a speculative repair before preserving a test.
2. Build the same target with the heavy lock. Minimize with the existing binary
   under the separate run lock:
   `flock /tmp/caskade-fuzz.lock nice -n 19 <binary> -minimize_crash=1
   -exact_artifact_path=/tmp/minimized <artifact>`.
3. Copy the minimized bytes to `../tests/regressions/<issue>-<description>.<ext>`.
   Add a T5 regression that fails against the original parser. For filesystem
   operations, use TestRoot and assert_sentinel_untouched; header-only parsers
   should assert the expected bounded error or safe parsed result.
4. Send the minimized file, reproducer command, and failing T5 case to the owner
   (archive or fetch repairs stay with their owning engineer). Land the test
   before the repair, then confirm it and a new ten-minute run pass.
5. Record the finding and fixed revision in issue evidence. Do not fabricate a
   regression file when the runs have produced no reports.

The user-owned root justfile recipe is edited in the working tree only. This
README and run.py are the committed, reviewable command definitions.
