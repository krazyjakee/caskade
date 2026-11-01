# Interactive install observation

`RunInteractive` retains the preset argv and uses the injected Native, Wine or
Proton runtime. It never sends keys, clicks controls, or automates a wizard.

The executor watches the storage root with `notify`, takes bounded before/after
inventories (100,000 entries and depth 64 by default), and checks expected regular
files using confined directory handles. Expectations come from
`ExecConfig.interactive.expect`, or matching silent steps of the same handler,
program and runtime. Unknown installers without expectations must produce a new
regular payload file in the game tree; prefix/internal `.caskade` files do not
count as a game. Watcher failures and inventory limits are explicit errors.

Only the requested game tree is accepted automatically. Newly created folders
with matching expectations elsewhere produce
`ExecError::NeedsUser(Question::ChooseInstallDirectory)` after rollback. Native
external observation scopes must be explicitly injected through `outside_roots`.
The observer never treats a preexisting directory or symlink as a new install.
No folder outside the requested root is moved automatically.

For prefix runtimes the configured `WINEPREFIX/drive_c`, or the game's private
`.caskade/prefix/drive_c`, is also inspected. A matching new payload inside the
private staging prefix is retained under
`<root>/.caskade/interactive/<job>/payload-N` before staging rollback. The caller
must explicitly resolve the folder choice; these recovery folders are not
committed games and are never silently deleted.

Retained locations are recorded durably as informational `InstallLocation`
entries. Only cleanup entries for newly created files/directories within a
validated fresh payload folder are removed; original-file backups and unrelated
side effects/compensations remain rollback actions. A modified preexisting file
prevents preservation. Startup recovery honors the same retention record.
Cancellation, failures and process timeouts retain their existing process-tree
termination and rollback semantics. Folder decisions terminate candidate fallback.

T3 covers all eight preset families with Native, Proton and Wine argv and verifies
`G:\Game` expansion. Native Windows OS command-line readback is shared with the
T7 `tests/native_windows.rs` suite; execution on Windows remains the CI #22 gate.
