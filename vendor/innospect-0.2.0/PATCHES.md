# Local safety patch for innospect 0.2.0

The unmodified published crates.io package is committed in `e5f376c`. Original source, Cargo metadata, and licenses remain in this directory. No binary layout, version dispatch, metadata interpretation, or extraction semantics have been replaced. The local changes only enforce resource limits.

- Add the typed `Error::ResourceLimit` variant.
- Bound metadata and cached-chunk decoded streams to 64 MiB; read at most the limit plus one byte to detect overflow.
- Reject stored metadata/chunks above 64 MiB and LZMA1/LZMA2 dictionaries above 64 MiB before allocation.
- Reject UTF-16/ANSI string fields above 16 MiB and combined metadata/individual record counts above 100,000.
- Serialize cached-chunk population and bound the sum of retained decoded chunks to 64 MiB per installer. A temporary decoded chunk is also individually bounded.

These are fixed parser ceilings. Caskade additionally applies its configurable raw metadata input, entry, output, expansion, disk, and cancellation limits. Inputs above a parser ceiling are explicitly unsupported; increasing a caller limit does not bypass these ceilings.

## Regression and validation

The robustness input `crates/caskade-installer/tests/regressions/26-inno-oversized-allocation.exe` is 76,735 bytes, SHA-256 `9032e1aa906d4d834dd9009adf427a4096633a561aed2e4f9e288bc6773c4bd0`. The inno fuzz target found it during seed initialization, before mutation, and requested a 1,107,296,359-byte wizard-image allocation. The deterministic T5 test is `tests/inno_regressions.rs::ambiguous_version_oversized_wizard_image_does_not_allocate`. Before the patch it aborted under a 512 MiB address-space cap with “memory allocation of 1107296359 bytes failed”; afterward the invalid interpretation is rejected before allocation and a bounded alternate interpretation succeeds, preserving this valid sliced installer. Direct oversized Pascal strings must return `FileTooLarge`.

All 18 committed Inno fixtures remain metadata regressions, including the three encrypted versions and all sliced versions; the 15 unencrypted fixtures must still extract to their exact golden trees. Additional tests exercise oversized Pascal strings and a valid LZMA metadata envelope advertising an oversized dictionary without allocating that dictionary.

## Draft upstream bug report — not filed

Title: Bound untrusted metadata allocations and decompression for hostile installer input

Affected upstream version: innospect 0.2.0.

Caskade processes user-supplied installer files without executing their stubs. Fuzzing the metadata entry point found that a small ambiguous installer can request a much larger allocation before input exhaustion is discovered. In inno, the observed allocation originates in `ReadBytesExt::read_raw_pascal_string`, called by `Wizard::read_images`; the wire length is trusted by `vec![0; length]`. Review also found unbounded LZMA dictionaries and decoded-header growth in the metadata/fallback paths.

Expected behavior: reject impossible or policy-exceeding lengths and dictionaries before allocation; bound decoded metadata even when the compressed input is small. Return a structured resource-limit error rather than aborting or exhausting memory.

Reproduction: invoke Caskade `inspect_bytes` with the regression input above. For the original instrumented executable, run the inno libFuzzer target against that single input with `-rss_limit_mb=1024`. The deterministic regression and the minimal local patch document the recommended checks. The constants could become explicit upstream parser options in a future release.

This report is a local draft. It has not been filed or sent to upstream.
