# Start Menu source

`StartMenuSource::new()` scans the captured per-user and all-users Programs roots. Entry, depth, file-size and aggregate metadata limits apply; malformed files warn without hiding healthy siblings. Symlink entries and special files are skipped. Paths and variables are resolved only through HostEnv.

Shared .lnk/.url parsers supply target, explicit argv, working directory and icon path. Relative link targets resolve from the shortcut directory. Existing standalone targets pass the shared classifier; the original shortcut argv/working directory are retained, rather than replaced by guessed launch arguments. Local image icons are read-only references. Shell namespace-only links that cannot be statically resolved are warned and skipped.

Known launcher URI and Steam -applaunch records use identity.proxy: they enrich existing owners, and never create an orphan game. Uninstall, documentation, website and support links are excluded by both name and target. Nothing invokes the shell, resolves a Windows COM object, launches a game or writes metadata.

Seven source tests use generated MS-SHLLINK and InternetShortcut fixtures, actual Steam/Epic source integration, portable game targets, captured variables, Unicode, duplicates, traversal confinement, malformed/oversized files and builtin routing. Native shell-link parity is deferred on #71. Original implementation, no ported code.
