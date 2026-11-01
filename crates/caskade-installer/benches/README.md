# Installer T10 performance gates (#50)

All workloads are generated under `/tmp` and removed by RAII. They are never
committed. Run on an idle Linux native runner, under the shared heavy lane:

```sh
CASKADE_7ZZ=/path/to/pinned/7zz CASKADE_PERF_OUTPUT=target/performance \
  flock /tmp/caskade-heavy.lock cargo test -p caskade-installer \
  --all-features --locked --release --test performance -- \
  --ignored --nocapture --test-threads=1
flock /tmp/caskade-heavy.lock cargo bench -p caskade-installer \
  --all-features --locked --bench installer -- --noplot
```

`performance.rs` enforces real API-level assertions. Criterion retains timing
distributions for the same probe, discovery and extraction workloads. Optional
`CASKADE_BENCH_LARGE=1` repeats the 10 GiB extraction in Criterion; the default
RSS assertion always runs that workload once in a fresh child process.

| Gate | Workload | Bound |
| --- | --- | --- |
| Probe | Actual committed Inno installer extended to a 20 GiB sparse file; builtin registry | <16 MiB read, <200 ms |
| Extraction | Same generated 128 MiB TAR through SafeDir and pinned official 7zz, five alternating-order samples, medians | Rust/reference <=1.5 |
| RSS | Generated 10 GiB sparse TAR, complete output written and byte count/size/tail checked, isolated worker VmHWM | <256 MiB |
| Discovery | Exactly 100,000 real regular files (99,999 nonempty data assets and actual fixture-game ELF), 100 directories | <1 s |

Setup, destination cleanup and payload validation are outside extraction timing;
actual archive parsing, safety validation, writing and static re-probing are
included. Both extractors use independent destinations and the same archive.
The ratio is not estimated from compressed lengths or an unrelated 7zz run.
RSS isolation prevents an earlier 100k-file walk's allocator high-water mark
from contaminating the streaming-extraction measurement. `/tmp` needs at least
12 GiB free; the TAR source is sparse but the extracted output is not.

The native nextest profile excludes these nightly-only heavy gates. Performance
failures remain hard failures; runner hardware must meet the budgets rather than
silently skipping or relaxing them. First local numbers and the final evidence
are recorded on issue #50; JSON measurements and Criterion reports are nightly
artifacts.

## Local evidence (2026-10-26)

Rust 1.98 release build on the Linux development host, official pinned 7zz 26.05:

| Assertion | Measured |
| --- | --- |
| 20 GiB installer probe | 1,272,296 bytes, 14.79 ms |
| 128 MiB TAR extraction medians | Rust 111.96 ms; 7zz 108.39 ms; ratio 1.033 |
| 10 GiB extraction | 6,544 KiB peak RSS; 15.82 s |
| 100,000-file discovery | 499.40 ms |

Criterion estimates: probe 13.59 ms, discovery 511.55 ms, TAR extraction 109.50 ms.
The first discovery assertion failed at 1,943.55 ms; production fix `7d37b61`
reuses directory handles and parallelizes bounded header reads without dropping
renamed-executable detection. The same unchanged workload now meets the budget.
These measurements are local evidence; runner-specific I4/T10 acceptance remains
on #22 until the native/nightly workflows execute there.
