# Flat package fixture provenance

`cargo xtask fixtures macos-pkg --prepare` builds genuine flat packages from
GNU cpio odc/newc payloads, gzip or modern pbzx/XZ chunks, a bomutils-generated
Bill of Materials, PackageInfo, and a checksummed XAR container. The bounded
Python XAR writer follows the upstream xar heap/TOC format. It is builder code,
not the installer parser. No proprietary installer engine or synthetic executable
bytes are used. The app is the existing portable shell-entrypoint fixture with
real framework symlinks; native installer compatibility is checked by #22.

`mac-pkg` is an app-only component, `mac-pkg-pbzx` has a component directory and
multiple raw/XZ chunks, `mac-pkg-unused-scripts` has an unreferenced Scripts member,
`mac-pkg-scripts` references a harmless postinstall and requires native delegation,
and `mac-pkg-system` targets /Library/CaskadePkgFixture for native delegation.
Package IDs are org.caskade.fixture.<fixture-id>; native tests remove only their
own installation directory and forget their own receipt.

bomutils is a pinned GPL-2.0-or-later build tool kept below ignored target/.
No builder executable is distributed with Caskade or committed here.
