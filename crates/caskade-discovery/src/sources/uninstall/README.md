# Windows uninstall source

`UninstallSource::new()` implements the read-only Source contract. `read_records` exposes bounded ARP metadata, including InstallDate (an installer service date, never a play timestamp). It walks HKLM/HKCU in both explicit registry views, with per-key access warnings and total key/value/string limits. HostEnv supplies every registry read and path mapping.

Visible independent records pass the shared classifier and preserve Maybe/Likely. Launcher-owned records use `alias.launcher_owned`: the merger drops orphan aliases and enriches only an existing owner’s missing uninstall command and IDs. Known IDs match first; ID-less library aliases may match an exact canonical install directory, never a parent/child folder. No title matching or speculative store IDs.

Commands are decoded into explicit argv; no shell or installer is run. Unquoted executable paths with spaces need an existing complete .exe path. DisplayIcon image files are resolved locally; exe resources are not mislabelled as standalone images. No icon files or source metadata are written.

The authored 100-key public-product-shape corpus documents its provenance in tests/fixtures/src/uninstall-windows. Eight tests cover all views, ARP visibility/fields, command handling, limits/cancellation, classifier injection, actual Steam merge and builtin routing. Native registry parity is deferred on #71. Original implementation, no ported code.
