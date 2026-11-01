# Local safety patch for inno 0.7.1

The unmodified published crates.io package is committed in `a2cce3d`. Original source, Cargo metadata, and licenses remain in this directory. No binary layout, version dispatch, metadata interpretation, or extraction semantics have been replaced. The local changes only enforce resource limits.

- Reject Pascal strings and raw wizard-image strings larger than 16 MiB before allocation, using `io::ErrorKind::FileTooLarge`.
- Bound each decoded primary/secondary metadata stream to 64 MiB; allow exact-limit EOF, reject further output.
- Reject metadata and payload LZMA dictionaries above 64 MiB before constructing their decoder.
- Reject PE optional-header data-directory counts above 16 before allocating the directory Vec (`FileTooLarge`).
- Reject combined metadata record counts and wizard-image counts above 100,000 before iteration/allocation.
- Retain bounded retries of ambiguous historical format versions; if none succeeds, preserve a resource-limit failure.

These are fixed parser ceilings. Caskade additionally applies its configurable raw metadata input, entry, output, expansion, disk, and cancellation limits. Inputs above a parser ceiling are explicitly unsupported; increasing a caller limit does not bypass these ceilings.

## Regression and validation

The robustness input `crates/caskade-installer/tests/regressions/26-inno-oversized-allocation.exe` is 76,735 bytes, SHA-256 `9032e1aa906d4d834dd9009adf427a4096633a561aed2e4f9e288bc6773c4bd0`. The inno fuzz target found it during seed initialization, before mutation, and requested a 1,107,296,359-byte wizard-image allocation. The deterministic T5 test is `tests/inno_regressions.rs::ambiguous_version_oversized_wizard_image_does_not_allocate`. Before the patch it aborted under a 512 MiB address-space cap with “memory allocation of 1107296359 bytes failed”; afterward the invalid interpretation is rejected before allocation and a bounded alternate interpretation succeeds, preserving this valid sliced installer. Direct oversized Pascal strings must return `FileTooLarge`.

All 18 committed Inno fixtures remain metadata regressions, including the three encrypted versions and all sliced versions; the 15 unencrypted fixtures must still extract to their exact golden trees. Additional tests exercise oversized Pascal strings and a valid LZMA metadata envelope advertising an oversized dictionary without allocating that dictionary.

## Draft upstream bug report — not filed

Title: Bound untrusted metadata allocations and decompression for hostile installer input

Affected upstream version: inno 0.7.1.

Caskade processes user-supplied installer files without executing their stubs. Fuzzing the metadata entry point found that a small ambiguous installer can request a much larger allocation before input exhaustion is discovered. In inno, the observed allocation originates in `ReadBytesExt::read_raw_pascal_string`, called by `Wizard::read_images`; the wire length is trusted by `vec![0; length]`. Review also found unbounded LZMA dictionaries and decoded-header growth in the metadata/fallback paths.

Expected behavior: reject impossible or policy-exceeding lengths and dictionaries before allocation; bound decoded metadata even when the compressed input is small. Return a structured resource-limit error rather than aborting or exhausting memory.

Reproduction: invoke Caskade `inspect_bytes` with the regression input above. For the original instrumented executable, run the inno libFuzzer target against that single input with `-rss_limit_mb=1024`. The deterministic regression and the minimal local patch document the recommended checks. The constants could become explicit upstream parser options in a future release.

This report is a local draft. It has not been filed or sent to upstream.

## Second probe-fuzzer allocation regression

`tests/regressions/26-inno-pe-data-directory-allocation.exe` (SHA-256
`eecdb400db23f91651a2e154905303f8fceb8c2531a37785b6abf9f402db1641`)
is a 155 KiB-class hostile PE input. The probe target requested
17,805,165,888 bytes in `DataDirectories::new_zeroed`, because
`NumberOfRvaAndSizes` was passed directly as a Vec length. The local patch rejects
counts above the Windows IMAGE_NUMBEROF_DIRECTORY_ENTRIES ceiling (16) before
allocation, and preserves this resource-limit error through SetupLoader dispatch
instead of erasing it as NotInnoFile. This is a conservative parser ceiling, not a new PE layout parser.

T5 tests replay the real input through `inspect_bytes`, require a typed limit
error, accept a complete 16-entry table, and reject 17/u32::MAX counts before
reading input. Other PE section/resource entry counts are u16-bounded; they do
not expose this u32 allocation path.

Additional drafted upstream report (not filed): inno 0.7.1 trusts the PE
optional header's NumberOfRvaAndSizes in `vec![DataDirectory; count]`. The input
above reproduces an approximately 17.8 GB request before a 155 KiB input is
exhausted. Validate the count against supported directory entries and optional
header bounds before allocation; return a structured size error. Microsoft
reference: https://learn.microsoft.com/en-us/windows/win32/debug/pe-format .
