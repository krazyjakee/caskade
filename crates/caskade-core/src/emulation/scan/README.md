# Pure ROM scanning (#118)

`ScanRequest` binds a saved `RomScanner` to the resolved profile extensions,
compatible platforms, region data, global exclusions and explicit limits.
`scan_folder` enumerates the selected root; `scan_files` transforms supplied
metadata without filesystem or database access. `scan_folder_with_progress`
reports enumeration, matching and grouping. Call the IO boundary from a blocking
worker and retain its cancellation token for the entire job.

The enumerator skips symlinks, hidden entries and opted-out cloud placeholders.
Content opens use a root directory capability. It reads bounded playlist text
and lists ZIP/7z metadata through the installer; it never extracts or executes
ROMs. File/entry count, depth, playlist text, archive metadata/read totals and
playlist graph work all have limits. Archive inspection warnings remain in the
report so a reconcile caller can preserve known entries in an incomplete scan.
Native Windows recall attributes and macOS dataless flags are isolated at the
IO boundary; their equivalent metadata flags are testable on every host.

M3U/CUE/GDI references are confined to the scan folder. CUE grammar is shared
with the installer input resolver. Referenced playlists and tracks are absorbed
transitively, even when an intermediate CUE is not itself an accepted extension.
Unaccepted standalone playlists do not hide otherwise playable files. Missing,
cyclic and excessively nested playlists produce warnings and do not consume
other files. Paths are matched exactly first, then with an unambiguous case
fold for Windows-authored playlists.

Output games group by folder, platform and cleaned title, with numeric disc
order. `GameRom` paths remain scanner-relative; archive matches retain the
archive path for the emulator and never create extracted paths. Region and
filename properties remain available to review.

Tests use the generated `caskade-emulation-fixtures` tree: exactly 1,000 physical
files, 963 expected groups, 983 top-level ROMs and 20 two-disc sets. The timed
scan excludes fixture generation and validates every title and ordered ROM path.
No copyrighted ROM payload, real BIOS or emulator is required.
