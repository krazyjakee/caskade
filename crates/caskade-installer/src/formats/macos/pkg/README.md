# Flat macOS packages

The reader expands app-only component packages without executing installer code.
It accepts a root component or nested component directories, odc/newc cpio,
gzip/XZ payloads and modern pbzx with raw or XZ chunks. App bundle metadata is
validated after import. Framework symlinks and ordinary permissions go through
SafeDir; hardlinks resolve only to an extracted file. Setuid/setgid are stripped.
Unreferenced Scripts members are skipped and lower fidelity to Partial. Active
script references, system install locations, and product Distribution files
require the native installer, since Distribution JavaScript/component selection
cannot safely be reproduced as data-only expansion.

The primary explicitly approved this bounded XAR subset instead of
apple-flat-package/apple-xar: their unconditional native bzip2/xz dependencies
break the pure-Rust cross-build requirement. No new C dependency is used here.
The parser verifies TOC and member checksums (SHA1/MD5/SHA256/SHA512 when present),
rejects overlapping heap spans and outer XAR links, bounds compressed TOC (2 MiB),
expanded TOC (16 MiB), XML nesting (64), file count (100,000), member metadata and
names, and limits decompressor memory to 64 MiB. pbzx buffers one chunk of at most
16 MiB. Payload writes additionally obey the executor's safety limits, progress,
cancellation, collision policy and capability-rooted staging directory.

Native delegation uses injected installer/pkgutil paths. Root invokes installer
directly; other users use injected osascript with a fixed AppleScript and every
argv item escaped by AppleScript's `quoted form of`. The OS authentication dialog
is the only elevation prompt. Caskade execution policy still applies. Package
receipt IDs and `pkgutil --forget` are recorded; forgetting a receipt does not
remove installed files. Full system uninstall needs native receipt/file tracking.

`macos_pkg` verifies actual fixture expansion, re-probing, launch, script skipping,
policy/delegate argv, hostile XML/heap/CPIO/pbzx inputs and cancellation. The
ignored `macos_pkg_native` profile runs the dedicated system-targeting fixture on
the disposable macOS runner, checks its receipt and actual launch, and removes
only its own fixture directory/receipt. It never runs installer on this Linux host.
The native runner remains the acceptance gate for macOS installer compatibility
and authentication behavior (#22).

Public robustness entry points: `parse_toc(bytes, actual_heap_size)` and
`Xar::inspect(&mut ReadSeek, XarLimits)`. Neither executes package code.

Format references: [upstream XAR archive writer](https://github.com/mackyle/xar/blob/master/xar/lib/archive.c), [XAR compression MIME types](https://github.com/mackyle/xar/blob/master/xar/lib/lzmaxar.c), and [bomutils builder](https://github.com/hogliux/bomutils). Legacy LZMA-alone XAR members are rejected cleanly; application/x-xz uses the bounded XZ decoder.
