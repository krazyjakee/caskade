# Manifest format fixtures

`install.v1.windows.json` uses Windows relative separators in file inventory,
artifact paths, plan paths, launch targets and the recorded uninstall program.
`install.v1.posix.json` and `install.v1.macos.json` use POSIX paths, including a
`.app` launch target. These are authored wire-format fixtures, not claims that
the files were produced by native Windows or macOS execution on this machine.
Tests load all three on any host and check that reserialization emits `/` in
typed filesystem fields. Native three-OS execution is covered by #22.

`install.v0.json` is an explicit pre-release migration contract. It uses a
handler-version map, `files[].hash` for SHA-256, and has no relocation record.
The v0 -> v1 migration turns the map into handler records, renames hashes and
adds the `unscanned` relocation placeholder. It does not guess a relocation
verdict or rewrite the source file on read. This fixture does not claim v0 was
previously released. Each future schema adds a fixture and an explicit step.
`root.v0.json` migrates `id` to `root_id`; `root.v1.json` is the current schema.
Unknown future versions fail explicitly and remain byte-for-byte unchanged.

The schema-v1 document is flat JSON with an unsigned `schema_version` and the
fields of `InstallRecord`. Construct via `InstallManifest::new(record)`, then
read the immutable record using `record()` or obtain construction data with
`into_record()`. The constructor validates nested shared plan/step/launch types.
`ManifestSource::from_input` preserves complete HTTP(S) URLs and reduces an
absolute local source selection to its logical file name; local provenance is
relative, with an optional SHA-256 checksum. The persisted plan input uses the
same source snapshot. `InstallPlan::relative_dir` retains its defined meaning
relative to the identified storage root; every game destination is game-relative.
Opaque arguments, environment values, registry values and artifact facts remain
execution data, not paths interpreted by the manifest reader.

`InstallManifest::write(game_dir)` and `RootManifest::write(root_dir)` write
inside `.caskade` using a newly created temporary file, sync it, then atomically
replace the destination. Read/write operations are confined with cap-std; a
symlinked `.caskade` directory is refused. Callers provide the current directory
location instead of storing its absolute path in the document. `root_id` is an
opaque stable identity provided by the root-registration layer, not a path.

`verify(game_dir, &manifest)` (also `manifest.verify(game_dir)`) checks every
recorded regular file and returns `VerifyReport` with missing files and typed
change reasons. Optional SHA-256 detects equal-size changes; unhashed files are
checked by file type and size. New user saves are ignored. A leaf symlink cannot
impersonate a regular file, and parent symlinks cannot escape the directory
capability. No execution, repair or mutation happens during verification.
