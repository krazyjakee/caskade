# Shared application probes (#102)

This is the consumer API for emulator detection. It describes installed applications as data; matching emulator definitions, profiles and imports belongs to caskade-core's emulator detection (`emulation/detect.rs`). Its Flatpak implementation must call this file-based reader rather than `flatpak list`.

All APIs accept an injected `HostEnv` and `ScanCtx` (use `ScanCtx::new(Cancel::new())` if no scan configuration is needed). They capture no additional environment, run no subprocesses, load no Windows COM objects, execute no applications and write no icons/metadata. `SourceError::Cancelled` is propagated. Malformed sibling metadata warns through ScanCtx; callers should read `take_warnings`. Fixed input and entry caps prevent unbounded inventories.

Stable entry points:

- `path::executables(env, ctx, &["retroarch", "dolphin-emu"])`: first executable per name in captured PATH order. Windows uses captured PATHEXT; empty/relative path elements never imply the process cwd. Supports Windows/Linux/macOS fixture profiles independently of the compiling OS.
- `flatpak::applications` / `flatpak::find` (`linux-sources`): user, system and installations.d deployments with matching `[Application] name`, an active deployment and a files directory. Runtime refs and export-only launchers are not installs. IDs and `LaunchTarget::Flatpak` are returned without any game-only filtering.
- `appimage::inspect` / `appimage::applications` (`deep`): static type-2 AppImage/squashfs metadata via the shared installer reader, including Desktop Entry Categories. No mount, runtime launch or extraction. Standard inventory roots are ~/Applications and ~/.local/bin; explicit files can be inspected anywhere allowed by HostEnv.
- `bundles::inspect` / `bundles::applications`: bounded XML/binary Info.plist, exact bundle identifier/version/category and confined CFBundleExecutable/icon paths. Standard roots are the captured system/user Applications locations. No Games-category filter, so emulator bundles remain available.
- `emulator_candidates(env, ctx, &["retroarch.exe", "dolphin.exe"])`: hands Emulator-only Linux Desktop Entries directly to the consumer; hands Windows shortcuts over only when their exact executable leaf matches the consumer’s definition names (ASCII case-insensitive). With an empty matcher list, no Windows shortcut is guessed to be an emulator. The game sources continue to exclude emulator-only entries.

`Application` and `EmulatorCandidate` are serializable neutral records. The consumer matches bundle IDs, Flatpak IDs and executable names from its own definitions, keeps explicit `launch` argv/cwd, then evaluates profile requirements without rerunning these probes. Presence of a record is never evidence that it is a game, a particular emulator, or runnable on a different architecture.

Nine fixture tests exercise all three OS profiles: PATH order/PATHEXT/executable mode, user/system/extra Flatpak deployment state, real committed AppImage metadata, binary bundle plist identity/version/icon, Emulator-only desktop and .lnk handoff, cancellation and external symlink sentinels. All fixtures are original synthetic metadata; no real user applications are executed. Native OS parity is deferred on #71.

Original code; no implementation was ported from another project.
