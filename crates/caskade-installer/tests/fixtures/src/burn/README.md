# Genuine Burn mini installer

Original WiX bundle authoring chains the committed `windows/msi-embedded` fixture.
`InstallFolder` is overridable and persisted, then passed to MSI INSTALLDIR/TARGETDIR for install and uninstall. It installs per user and
contains no custom fixture bootstrapper program or simulated archive bytes.
Build with `cargo xtask fixtures burn` on Linux with Wine/Xvfb. Official WiX3.14.1
and Wine Mono9.4.0 download hashes are pinned in builders.json; binaries stay in
ignored target/fixture-build. The bundle is below the 3 MB per-file limit and the
corpus remains below the global 80 MB cap. WiX emits build-time bundle GUIDs and
cabinet timestamps; rebuild output is an intentionally reviewed fixture update.

Extraction golden reuses the exact existing MSI payload tree. Native Windows
acceptance is `native_burn_bundle_installs_silently_and_actual_fixture_game_runs`;
Wine-only pre-evidence is generated separately and never called Windows proof.
