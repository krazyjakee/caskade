# Shipped emulation catalogue

`caskade_sources::emulation::catalog()` parses and validates the 23
embedded TOML files. Desktop startup passes that catalogue to
`EmulationService`; invalid shipped data is logged and falls back to an empty
catalogue. `definition_files()` yields source names and text so plugins and
user files can be appended before `Catalog::load` (later ids replace earlier
ones). Loading definitions performs no host I/O or emulator execution.

The catalogue contains 473 platforms, 20 regions and 21 emulator definitions.
Platform recognition is independent of emulator availability: every platform
in the pinned RomM list (463 slugs) and Playnite list (96 platforms) has a
definition, including platforms with no IGDB id, known ROM extensions or
shipped emulator profile. RomM imports those games normally; playing one
still needs a compatible emulator. Existing Caskade platform ids are preserved.

`platforms.toml` is generated. `platform-sources.json` records factual names
and identifiers with pinned source URLs; `platform-overrides.toml` holds the
hand-maintained Caskade ids, extensions, BIOS metadata and recommendations.
Edit the overrides and run `python3 scripts/platform-catalog.py` from the
repository root to regenerate offline. To update the source snapshot, review
the commit pins in the script and run
`uv run --with PyYAML==6.0.3 scripts/platform-catalog.py --refresh`.
`python3 scripts/platform-catalog.py --check` verifies the committed generated
file without rewriting it.
The coverage test checks every source platform, including ones without emulator
support. Explicit `ids.romm` slugs take precedence over IGDB ids, keeping
platforms with missing or incorrect server metadata recognizable. The pinned
Playnite list's Vectrex IGDB id is corrected from 67 (Intellivision) to 70.

The definitions ported from Playnite (RetroArch, PCSX2, DuckStation, Dolphin,
PPSSPP, melonDS, mGBA, Cemu, Ryujinx, RPCS3) say so in their headers; the rest
(MesenCE, Rosalie's Mupen GUI, gopher64, Flycast, Ymir, ares, MAME, Azahar,
xemu, Xenia Canary, shadPS4) are authored for Caskade from
`docs/research/emulator-sources.md`.
Caskade ids stay independent of external databases: e.g. `nintendo_snes` has
Playnite id `nintendo_super_nes`. The recommended RetroArch core order is
encoded by the `[[profile]]` order. A profile is offered only when its
`requires` files exist; platform support does not imply every core is built
for every OS or machine architecture.

Each emulator declares tracking behavior. `default` delegates to install-kind
tracking (process tree for ordinary executables, process name for Flatpak,
original process for app bundles). Ryujinx explicitly tracks its named process
because its entry process may relaunch. It recognizes existing Ryujinx-class
installations and has no install source.

Definitions also say how Caskade installs an emulator (`[install.<os>]`:
GitHub releases, Dolphin's update endpoint, a URL or Flathub), the flags that
make a launch fullscreen or windowed (`[display]`), the config an install
writes so the first launch skips setup wizards (`[[setup]]`), and where
RetroArch's cores download from (`[requires_from]`). Platforms list the
emulators to install for them (`recommended`). Caskade downloads emulators
and cores from their projects when the user asks; nothing is redistributed.
The core's [`EmulatorSetup`](../../caskade-core/src/emulation/setup/mod.rs)
coordinates installs, updates and BIOS retrieval. `install_sources_match_released_files`
checks each asset pattern against the file names released when it was
written.

The RetroArch core paths are editable data: Windows portable `cores`, native
Linux XDG config, sandboxed Flatpak app config and macOS Application Support.
Required-file checks prevent offering absent cores. Distro-specific core paths
can be supplied by a later definition override. The command contains argv
tokens rather than shell fragments; spaces in ROM/core paths stay intact.

Firmware metadata lists stable filenames and known hashes, never bytes.
Optional boot-ROM alternatives are marked optional where HLE is available.
Saturn's regional files are required by Beetle Saturn. BIOS requirements can
vary by emulator and region; the catalogue does not model a firmware download
or universally pin one PS2 dump/PS3 firmware revision. PS2 reference dumps
are alternatives, not a requirement for every listed revision. Those remain the
emulator's own setup. RPCS3 has a direct executable profile; its installed-game
importer is intentionally absent at this stage.

Windows platform/profile data is adapted from Playnite under MIT. See
[THIRD-PARTY.md](../THIRD-PARTY.md) for the source commit, paths, full license
and factual BIOS metadata references. Linux/macOS rules and region aliases are
authored for Caskade. Updating a definition requires reviewing the per-OS
snapshots in `tests/snapshots`, not downloading a real emulator or a ROM.

BIOS files are looked up in the platform's BIOS folder override, otherwise
in the definition's `bios` variable (RetroArch's `system` folder, DuckStation's
and PCSX2's `bios` folders, per OS and Flatpak), otherwise in `system/` and
`bios/` next to the emulator.

Validation: `cargo test -p caskade-sources --test it emulation_definitions::`.
The test validates all embedded files, every platform/profile link, every
OS/install-kind variable expansion and the exact resolved argv snapshots.

The macOS bundle identifiers were checked against the projects' build files:
[PCSX2](https://github.com/PCSX2/pcsx2/blob/master/pcsx2/Resources/Info.plist.in),
[DuckStation](https://github.com/stenzek/duckstation/blob/master/src/duckstation-qt/Info.plist.in),
[Dolphin](https://github.com/dolphin-emu/dolphin/blob/master/Source/Core/DolphinQt/Info.plist.in),
[PPSSPP](https://github.com/hrydgard/ppsspp/blob/master/macOS/Info.plist),
[melonDS](https://github.com/melonDS-emu/melonDS/blob/master/res/melon.plist.in),
[mGBA](https://github.com/mgba-emu/mgba/blob/master/src/platform/qt/CMakeLists.txt),
[Cemu](https://github.com/cemu-project/Cemu/blob/main/src/CMakeLists.txt), and
[RPCS3](https://github.com/RPCS3/rpcs3/blob/master/rpcs3/rpcs3.plist.in).
