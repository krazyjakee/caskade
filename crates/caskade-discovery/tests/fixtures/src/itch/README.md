# itch fixture provenance (#94)
Original generated schema subset/records, TOML actions and receipt descriptions;
no user database, receipt, account or game data copied. Tests create real SQLite
and gzip bytes from these reviewable sources in temporary HostEnv machines.
Format references (27 October 2026):
- https://github.com/itchio/butler database/models/cave.go and install_location.go
  (snake_case columns; custom folder or install-location + folder name).
- https://github.com/itchio/go-itchio types.go (squashed platform architecture
  columns windows/linux/osx; older p_windows/p_linux flags remain supported).
- https://github.com/itchio/hush bfs/receipt.go (gzip JSON game/upload/files).
- https://itch.io/docs/itch/integrating/manifest-actions.html (.itch.toml argv and
  platform names linux/windows/osx).
These are format references only; no code, comments or tests were ported.
