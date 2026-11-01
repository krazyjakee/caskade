# Start Menu source fixture

Original generated Shell Links and InternetShortcut metadata only; no actual user shortcuts or game binaries. Both per-user and all-users Programs roots contain game, launcher, duplicate, broken and unrelated shortcuts. .lnk JSON uses the MS-SHLLINK writer shared with #67. Names, paths and payloads are synthetic.

The portable Café game has explicit argv, saves working directory, icon and Unity-layout placeholders. A second game tests captured-variable expansion. Steam and Epic shortcuts have their own separate launcher manifests and must enrich those groups without creating extras. The absent Steam shortcut must not create a game. A relative Shell Link resolves from the shortcut directory, independent of the process cwd. Uninstall, support, manual, website, malformed and missing-target inputs exercise clean exclusion/warnings. Placeholder exe/dll files are never executed.
