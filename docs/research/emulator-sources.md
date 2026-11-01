# Emulator download sources

Research for the automatic emulator install, setup and update feature. Everything marked as
verified was checked on 2026-10-28: GitHub with `gh api repos/…/releases/latest` (or
`releases?per_page=N`), other hosts with `curl`. Asset names, tags and dates are exactly as
they were listed that day. Anything not checked is marked **unverified**.

Platform ids are the ones in `crates/caskade-sources/emulation/platforms.toml`. Proposed new ids
use the same `<maker>_<system>` style and are marked *(new)*.

## 1. Indexes reviewed

| Source | What it has | License | Machine-readable | Verdict |
|---|---|---|---|---|
| ES-DE `resources/systems/{linux,windows,macos}/es_find_rules.xml` and `es_systems.xml` ([gitlab.com/es-de/emulationstation-de](https://gitlab.com/es-de/emulationstation-de)) | Find rules for 128/127/83 emulators (Linux/Windows/macOS): PATH names, Flatpak ids, AppImage globs, registry keys, static paths. `es_systems.xml` has a launch command for every emulator and system (e.g. `-dvd_path %ROM%`, `--no-gui`, `-b -e`). No download URLs. | MIT | Yes (XML) | **Port.** The best source for detection rules and launch arguments. It complements the Playnite port. |
| ES-DE `USERGUIDE.md` | A per-system table of the default and alternative emulators. AppImage tables (x86_64 and aarch64) that give the file name glob and whether the package is "Official", "pkgforge-dev" or "Community". | MIT | Markdown tables | **Use** to choose default emulators and AppImage sources. |
| EmuDeck (`dragoonDorise/EmuDeck` `functions/EmuScripts/emuDeck*.sh`; `EmuDeck/emudeck-we` for Windows) | Shell or PowerShell per emulator. These call `getReleaseURLGH "<owner/repo>" "<asset substring>"`, the RPCS3 update API, or hard-coded URLs (Ryujinx points at Codeberg attachments, Vita3K pins a build). | GPL-3.0 | No (code) | **Reference only.** Read it to see which repo and asset each emulator uses, but don't copy code (GPL). Linux and Windows only. |
| RetroDECK (`RetroDECK/RetroDECK`, `RetroDECK/components`) | Flatpak build manifests per component. | GPL-3.0 | YAML/JSON manifests | Low value. It builds from source inside a Flatpak. |
| RetroBat (`RetroBat-Official/retrobat`) | `build.ini` downloads emulators from RetroBat's own mirror (`http://www.retrobat.org/repo/win64/`). `retrobat-bios` repository. | "LGPL v3 for RetroBat's code, no commercial use" (custom, GitHub says NOASSERTION) | Partly | **Avoid.** It repackages binaries, is Windows only, and its BIOS repository hosts copyrighted files. |
| Playnite `source/Playnite/Emulation/Emulators/*/emulator.yaml`, `Platforms.yaml` | Detection, profiles and arguments (Windows only). No download data. | MIT | YAML | Already ported. |
| libretro `libretro-core-info` (`*.info`) and buildbot `assets/frontend/info.zip` | Per core: `firmwareN_path`, `firmwareN_desc`, `firmwareN_opt`, plus MD5s in `notes`. | MIT | Yes (INI-like) | **Port** for RetroArch BIOS requirements. |
| libretro-database `dat/System.dat` | BIOS file names, sizes and hashes per system. | CC-BY-SA-4.0 | Yes (clrmamepro DAT) | Usable with attribution and share-alike. |
| RomM `backend/models/fixtures/known_bios_files.json` | 374 entries `"<romm slug>:<file>": {size, crc, md5, sha1}` (ps2 70, psx 21, snes 17, saturn 11, ngc 10, dc 5, …). | AGPL-3.0 | Yes (JSON) | Don't copy it (AGPL). It is useful as a cross-check, and RomM already sends `is_verified`. |
| Obtainium Emulation Pack (`RJNY/Obtainium-Emulation-Pack`) | Obtainium JSON: GitHub sources and asset regexes per emulator. | Unlicense | Yes (JSON) | Android only, so little direct use. It is a good model for a "source + asset regex" schema. |
| ivan-hc/AM | Per-app install scripts for Linux AppImages. | GPL-3.0 | No (code) | Reference only. |
| Lutris runners API (`lutris.net/api/runners`) | 44 runners with Lutris-hosted tarballs. | Unclear | JSON | **Not useful.** Its versions are years old (pcsx2 v2.6.2, rpcs3 v0.0.18, dolphin 5.0-14480). |
| `DerekTurtleRoe/awesome-emulators` | A curated list. | CC-BY-SA-4.0 | Markdown | Useful for discovering emulators only. |
| Emulation General Wiki | Recommendations per system. | **unverified** (blocked by Cloudflare) | No | Read only. |
| pkgforge-dev (GitHub org) | Community AppImages where upstream ships none: `Dolphin-emu-AppImage`, `ares-emu-appimage` (MIT), `DeSmuME-AppImage`, `PPSSPP-AppImage`, `Azahar-AppImage-Enhanced`. | Varies | GitHub releases | Fallback for Linux AppImage gaps. ES-DE lists these as a source. |

## 2. Source kinds

| Kind | Meaning | Update-trackable |
|---|---|---|
| `gh-latest` | `releases/latest` carries the binaries, and the tag is the version. | Yes |
| `gh-rolling` | A fixed tag (`latest`, `continuous`) is re-published, or each build gets a commit-hash tag. Track it by `published_at` or by the asset's `updated_at`/`id`. | Yes (by date) |
| `gh-tag+url` | The GitHub tag gives the version, and a URL template on the project's own CDN gives the file. | Yes |
| `json-api` | The project's own update endpoint returns the version and URLs. | Yes |
| `forgejo` | A Gitea/Forgejo `/api/v1/repos/{o}/{r}/releases/latest`, the same shape as GitHub. | Yes |
| `flathub` | Install `flatpak install flathub <id>`. Updates come from Flatpak. | Via Flatpak |
| `page` | An HTML page or directory listing. | **Download only** |

## 3. Emulators

Each section gives the download sources per OS, the Flathub id, BIOS needs, the platforms covered,
and "Fullscreen & first run" (the flags and setup needed for one-click Play). In the tables, x64
means x86_64.

### RetroArch (`retroarch`), GPL-3.0

| OS | Kind | Source | File / regex | Verified | Format | Track |
|---|---|---|---|---|---|---|
| all | version | GitHub `libretro/RetroArch` | `releases/latest` has only `retroarch-sourceonly-1.22.2.tar.xz` | `v1.22.2`, 2025-11-20 | n/a | gives the version |
| windows | gh-tag+url | `https://buildbot.libretro.com/stable/{v}/windows/x86_64/RetroArch.7z` (portable). Also `RetroArch-Win64-setup.exe` and `RetroArch_cores.7z` | listed in the 1.22.2 directory | yes | 7z | Yes |
| linux | gh-tag+url | `https://buildbot.libretro.com/stable/{v}/linux/x86_64/RetroArch.7z`. Also `RetroArch_Qt.7z` and `RetroArch_cores.7z` | listed. ES-DE says it contains `RetroArch-Linux-x86_64.AppImage` (contents **unverified**) | yes | 7z → AppImage | Yes |
| macos | gh-tag+url | `https://buildbot.libretro.com/stable/{v}/apple/osx/universal/RetroArch_Metal.dmg` (there is also an `apple/osx/x86_64/` directory) | listed | yes | dmg (universal) | Yes |

- Flathub: `org.libretro.RetroArch` (1.22.2).
- BIOS: per core, in the `system/` directory. See section 4 and the `.info` firmware entries.
- Platforms: all 20 current ones through cores (`retroarch.toml`).
- **Fullscreen & first run:**
  - Fullscreen is `-f` / `--fullscreen` (`retroarch.c`, verified). Start content with `-L <core> <rom>`. There is no game list to hide.
  - Windowed is the default (`video_fullscreen = false`, **unverified** default).
  - There is no wizard, but launching fails without the core.
  - Portable: the Windows 7z is self-contained, with `retroarch.cfg` next to the exe. Pre-seed with `-c/--config <file>` or `--appendconfig <file>`, setting `system_directory`, `libretro_directory` and `video_fullscreen`.

### PCSX2 (`pcsx2`), GPL-3.0

| OS | Kind | Repo | Regex | Verified asset (tag, date) | Format | Track |
|---|---|---|---|---|---|---|
| windows | gh-latest | `PCSX2/pcsx2` | `^pcsx2-v[\d.]+-windows-x64-Qt\.7z$` | `pcsx2-v2.8.2-windows-x64-Qt.7z` (`v2.8.2`, 2026-09-04) | 7z portable (also `-installer.exe`) | Yes |
| linux | gh-latest | same | `^pcsx2-v[\d.]+-linux-appimage-x64-Qt\.AppImage$` | `pcsx2-v2.8.2-linux-appimage-x64-Qt.AppImage` | AppImage | Yes |
| macos | gh-latest | same | `^pcsx2-v[\d.]+-macos-Qt\.tar\.xz$` | `pcsx2-v2.8.2-macos-Qt.tar.xz` (arm64/x64 split **unverified**, only one asset) | tar.xz → .app | Yes |

- Nightlies are prereleases (`v2.9.147`, 2026-10-27) with the same asset names, minus the installer.
- Flathub: `net.pcsx2.PCSX2`.
- BIOS: **required**. Any PS2 BIOS dump (e.g. `scph39001.bin`, `scph70012.bin`) goes in the `bios` folder. If the configured file is missing, PCSX2 scans the folder (`FindBiosImage`, verified).
- Platforms: `sony_playstation2`.
- **Fullscreen & first run:**
  - Flags (help text verified): `-fullscreen`, `-nofullscreen`, `-batch` (exits when the game shuts down), `-nogui` (hides the main window, implies batch), `-bigpicture`, `-slowboot`/`-fastboot`, `-portable`, `-datapath <dir>`, `--`.
  - Windowed is the default.
  - First run shows a setup wizard while `[UI] SetupWizardIncomplete` is true.
  - Portable: `portable.txt` or `portable.ini` next to the exe (`portable.txt` may contain a data path), or `-datapath`. Settings go to `<data>/inis/PCSX2.ini`.
  - Pre-seed `PCSX2.ini` with `[UI] SetupWizardIncomplete = false` and `[Folders] Bios = <dir>`. The keys are verified in `QtHost.cpp` and `Pcsx2Config.cpp`.

### DuckStation (`duckstation`), CC-BY-NC-ND-4.0 (Flathub metadata; GitHub reports NOASSERTION)

| OS | Kind | Repo | Regex | Verified asset | Format | Track |
|---|---|---|---|---|---|---|
| windows | gh-rolling | `stenzek/duckstation` | `^duckstation-windows-x64-release\.zip$` | yes (tag `latest`, "Latest Rolling Release", 2026-10-25) | zip | Yes, by `published_at` |
| linux | gh-rolling | same | `^DuckStation-x64\.AppImage$` | yes | AppImage | Yes |
| macos | gh-rolling | same | `^duckstation-mac-release\.zip$` | yes (universal **unverified**) | zip → .app | Yes |

- `releases/latest` returns the rolling tag `latest`. There is also a `preview` prerelease (2026-10-28) and occasional versioned tags (`v0.1-12083`, 2026-10-13). Variants: `-sse2`, `arm64`, `armhf`, and an installer.
- Flathub: `org.duckstation.DuckStation` is **end-of-life** (`is_eol: true`, last build 0.1-9482, 2025-08). Use the AppImage.
- BIOS: **required**. Any PS1 BIOS (`scph5501.bin`, `scph1001.bin`, …), auto-detected in `[BIOS] SearchDirectory`.
- Platforms: `sony_playstation`.
- **Fullscreen & first run:**
  - Flags (verified): `-fullscreen`, `-nofullscreen`, `-batch`, `-nogui`, `-bigpicture`, `-fastboot`/`-slowboot`, `--`.
  - Windowed is the default.
  - First run shows a setup wizard while `[Main] SetupWizardIncomplete` is true.
  - Portable: `portable.txt` or `settings.ini` next to the exe. For an AppImage, that is next to `$APPIMAGE` (verified in `core.cpp`).
  - Pre-seed `settings.ini` with `[Main] SetupWizardIncomplete = false` and `[BIOS] SearchDirectory = <dir>`.

### Dolphin (`dolphin`), GPL-2.0+

| OS | Kind | Source | File | Verified | Format | Track |
|---|---|---|---|---|---|---|
| all | json-api | `https://dolphin-emu.org/update/latest/beta/` (releases) and `/update/latest/dev/` (dev builds). The JSON has `shortrev`, `date`, `hash`, `artifacts[{system,url}]` | `beta`: `2609a`, 2026-10-08. `dev`: `2609-127`, 2026-10-25 | yes | n/a | **Yes** |
| windows | json-api | artifact `"Windows x64"` | `https://dl.dolphin-emu.org/releases/2609a/dolphin-2609a-x64.7z` | yes | 7z portable | Yes |
| linux | json-api / flathub | release: `"Linux x86_64 (Flatpak)"` → `dolphin-2609a-x86_64.flatpak` (a bundle). AppImage only on `dev`: `"Linux x64 (AppImage)"` → `dolphin-master-2609-127-x64.AppImage` | yes | flatpak bundle / AppImage | Yes |
| macos | json-api | `"macOS (ARM/Intel Universal)"` → `dolphin-2609a-universal.dmg` | yes | dmg (universal) | Yes |

- There are no GitHub releases (`releases/latest` returns 404).
- Flathub: `org.DolphinEmu.dolphin-emu` (2609a).
- Community AppImage of the release: `pkgforge-dev/Dolphin-emu-AppImage`, `Dolphin_Emulator-2609a-anylinux-x86_64.AppImage`.
- BIOS: none. The GameCube IPL (`IPL.bin`) is optional.
- Platforms: `nintendo_gamecube`, `nintendo_wii`.
- **Fullscreen & first run:**
  - Flags (verified in `CommandLineParse.cpp`): `-e/--exec <file>`, `-b/--batch` (no UI, needs `--exec`), `-u/--user <dir>`, `-C/--config <System>.<Section>.<Key>=<Value>`.
  - Fullscreen is `-C Dolphin.Display.Fullscreen=True`. Windowed is the default.
  - First run: an analytics consent prompt while `Analytics.PermissionAsked` is unset (`Main.cpp`, verified). Pass `-C Dolphin.Analytics.PermissionAsked=True` or pre-seed `Dolphin.ini [Analytics] PermissionAsked = True`.
  - Portable: `portable.txt` next to the exe, `$DOLPHIN_EMU_USERPATH`, or `-u`.

### PPSSPP (`ppsspp`), GPL-2.0+

| OS | Kind | Repo / URL | Regex | Verified asset (`v1.20.4`, 2026-05-16) | Format | Track |
|---|---|---|---|---|---|---|
| windows | gh-latest | `hrydgard/ppsspp` | `^PPSSPP-v[\d.]+-Windows-x64\.zip$` | `PPSSPP-v1.20.4-Windows-x64.zip` | zip portable | Yes |
| linux | gh-latest | same | `^PPSSPP-v[\d.]+-anylinux-x86_64\.AppImage$` | `PPSSPP-v1.20.4-anylinux-x86_64.AppImage` (+ `.zsync`) | AppImage | Yes |
| macos | gh-latest / gh-tag+url | same: `^PPSSPPSDL-macOS-v[\d.]+\.zip$` (SDL build). Better: `https://www.ppsspp.org/files/{v with _}/PPSSPP_macOS.dmg` | `PPSSPPSDL-macOS-v1.20.4.zip`. The site lists `/files/1_20_4/PPSSPP_macOS.dmg` | zip / dmg | Yes |

- Flathub: `org.ppsspp.PPSSPP`.
- BIOS: none.
- Platforms: `sony_psp`.
- **Fullscreen & first run:**
  - Flags (verified in `Core/CmdLine.cpp`): `--fullscreen`, `--windowed`, `--pause-menu-exit` (exit when the game ends), `--escape-exit`.
  - Windowed is the default. No wizard.
  - Portable: on Windows the data stays next to the exe unless an `installed.txt` there points it to Documents (verified in `Windows/main.cpp`). `memstick_dir.txt` redirects the memstick.

### melonDS (`melonds`), GPL-3.0

| OS | Kind | Repo | Regex | Verified asset (`1.1`, 2025-11-18) | Format | Track |
|---|---|---|---|---|---|---|
| windows | gh-latest | `melonDS-emu/melonDS` | `^melonDS-[\d.]+-windows-x86_64\.zip$` | `melonDS-1.1-windows-x86_64.zip` | zip | Yes |
| linux | gh-latest | same | `^melonDS-[\d.]+-appimage-x86_64\.zip$` | `melonDS-1.1-appimage-x86_64.zip` (an AppImage inside a zip). Also `-ubuntu-x86_64.zip` | zip → AppImage | Yes |
| macos | gh-latest | same | `^melonDS-[\d.]+-macOS-universal\.zip$` | `melonDS-1.1-macOS-universal.zip` | zip → .app | Yes |

- Flathub: `net.kuribo64.melonDS`.
- BIOS: optional for DS (FreeBIOS is built in): `bios7.bin`, `bios9.bin`, `firmware.bin`. **Required** for DSi mode: `dsi_bios7.bin`, `dsi_bios9.bin`, `dsi_firmware.bin`, `dsi_nand.bin`.
- Platforms: `nintendo_ds`.
- **Fullscreen & first run:**
  - Flags (verified in `CLI.cpp`): `-f/--fullscreen`, `-b/--boot auto|always|never`.
  - Windowed is the default. No wizard.
  - Portable: a `portable` directory next to the exe, or next to the `.app` on macOS (verified in `main.cpp`). Config is `melonDS.toml`.

### mGBA (`mgba`), MPL-2.0

| OS | Kind | Repo | Regex | Verified asset (`0.10.5`, 2025-03-09) | Format | Track |
|---|---|---|---|---|---|---|
| windows | gh-latest | `mgba-emu/mgba` | `^mGBA-[\d.]+-win64\.7z$` | `mGBA-0.10.5-win64.7z` | 7z (also `-win64-installer.exe`) | Yes |
| linux | gh-latest | same | `^mGBA-[\d.]+-appimage-x64\.appimage$` (lowercase extension) | `mGBA-0.10.5-appimage-x64.appimage` | AppImage | Yes |
| macos | gh-latest | same | `^mGBA-[\d.]+-macos\.dmg$` | `mGBA-0.10.5-macos.dmg`. There is also `-osx.dmg`; which one is for older or Intel Macs is **unverified** | dmg | Yes |

- Flathub: `io.mgba.mGBA`.
- BIOS: optional `gba_bios.bin` (`-b <file>`).
- Platforms: `nintendo_gba`, `nintendo_gb`, `nintendo_gbc`.
- **Fullscreen & first run:**
  - Flags (verified in `commandline.c`): `-f/--fullscreen`, `-b/--bios`, `-C/--config key=value`.
  - Windowed is the default. No wizard.
  - Portable: `portable.ini` next to the exe (`mCoreConfigPortableIniPath`).

### Cemu (`cemu`), MPL-2.0

| OS | Kind | Repo | Regex | Verified asset (`v2.6`, 2025-02-06) | Format | Track |
|---|---|---|---|---|---|---|
| windows | gh-latest | `cemu-project/Cemu` | `^cemu-[\d.]+-windows-x64\.zip$` | `cemu-2.6-windows-x64.zip` | zip | Yes |
| linux | gh-latest | same | `^Cemu-[\d.]+-x86_64\.AppImage$` | `Cemu-2.6-x86_64.AppImage` (also `cemu-2.6-ubuntu-22.04-x64.zip`) | AppImage | Yes |
| macos | gh-latest | same | `^cemu-[\d.]+-macos-12-x64\.dmg$` | `cemu-2.6-macos-12-x64.dmg` (x64 only, so Rosetta on Apple Silicon) | dmg | Yes |

- The last 4 releases are all stable. There have been none since 2025-02.
- Flathub: `info.cemu.Cemu`.
- Keys: `keys.txt` (disc keys) is needed only for encrypted WUD/WUX. Decrypted dumps and `.wua` need none. Online play needs `otp.bin` and `seeprom.bin` (optional).
- Platforms: `nintendo_wiiu`.
- **Fullscreen & first run:**
  - Flags (verified in `LaunchSettings.cpp`): `-g/--game <path>`, `-f/--fullscreen`, `-m/--mlc <dir>`.
  - Windowed is the default.
  - First run: the "Getting Started" dialog appears when no config exists (`isFirstStart`, `CemuApp.cpp`). Pre-seed `settings.xml`.
  - Portable: a `portable` directory next to the exe, or a legacy `settings.xml` next to the exe (verified).

### Ryujinx, Ryubing fork (`ryujinx`), MIT

| OS | Kind | Source | File | Verified | Format | Track |
|---|---|---|---|---|---|---|
| all | json/redirect | `https://update.ryujinx.app/download/query?os={win\|linux\|linuxai\|mac}&arch={x64\|arm64}&rc=stable` (links on `ryujinx.app/download`) | n/a | links verified on the page, endpoint **unreachable** from here (TCP refused) | win zip, linux tar.gz, linuxai AppImage, mac arm64 | **unverified** |
| all | forgejo | `https://git.ryujinx.app/api/v1/repos/Ryubing/{Ryujinx,Canary}/releases` (EmuDeck uses this; assets `*linux_x64.tar.gz`) | n/a | **unreachable** (connection refused) | | unverified |

- The GitHub orgs `Ryujinx/Ryujinx` and `Ryubing/Ryujinx` both return 404.
- Flathub: `io.github.ryubing.Ryujinx` (1.3.3, 2025-10-10).
- Firmware: **required**. Needs `prod.keys` (and `title.keys`) plus a Switch firmware dump, both from the user's own console. They cannot be downloaded legally.
- Platforms: `nintendo_switch`.
- **Fullscreen & first run:**
  - `--fullscreen` (current `ryujinx.toml`, from the upstream `CommandLineState.cs`) and `-r/--root-data-dir <dir>`. Both **unverified** for current Ryubing, because its source host was unreachable.
  - Windowed is the default.
  - Portable: a `portable` directory next to the exe (**unverified**).
  - It won't boot games until keys and firmware are installed. Firmware is installed through the GUI (Tools → Install Firmware), and no CLI flag is known.

### RPCS3 (`rpcs3`), GPL-2.0

| OS | Kind | Repo | Regex | Verified asset (`build-6c44fff7…`, 2026-10-28) | Format | Track |
|---|---|---|---|---|---|---|
| windows | gh-latest (binaries repo) | `RPCS3/rpcs3-binaries-win` | `^rpcs3-v[\d.]+-\d+-[0-9a-f]+_win64_msvc\.7z$` | `rpcs3-v0.0.43-20415-6c44fff7_win64_msvc.7z` (+ `.sha256`) | 7z portable | Yes |
| linux | gh-latest | `RPCS3/rpcs3-binaries-linux` | `^rpcs3-v[\d.]+-\d+-[0-9a-f]+_linux64\.AppImage$` | `rpcs3-v0.0.43-20415-6c44fff7_linux64.AppImage` | AppImage | Yes |
| macos x64 | gh-latest | `RPCS3/rpcs3-binaries-mac` | `_macos\.7z$` | `rpcs3-v0.0.43-20415-6c44fff7_macos.7z` | 7z → .app | Yes |
| macos arm64 | gh-latest | `RPCS3/rpcs3-binaries-mac-arm64` | `_macos_aarch64\.7z$` | `rpcs3-v0.0.43-20415-6c44fff7_macos_aarch64.7z` | 7z → .app | Yes |
| all | json-api | `https://update.rpcs3.net/?api=v3&c=<commit>&os_type={windows\|linux\|macos}&os_arch={x64\|arm64}` | `latest_build.version` = `0.0.43-20415`, plus `{download,size,checksum(SHA-256)}` | yes | | **Yes** (with checksum) |

- The main repo `RPCS3/rpcs3` has `v0.0.43` (2026-09-30) with **no assets**. Every master merge publishes a build in the binaries repos.
- Flathub: `net.rpcs3.RPCS3`.
- Firmware: **required**, `PS3UPDAT.PUP`. Install with `rpcs3 --installfw <PUP>` (verified in `rpcs3.cpp`). Sony serves it:
  - `https://fus01.ps3.update.playstation.net/update/ps3/list/us/ps3-updatelist.txt` lists 4.93 with `CDN=…/2026_0318_a2b60b6ac1d2e49e230144345616927c/PS3PATCH.PUP`.
  - The full `PS3UPDAT.PUP` in the same directory answers 200 with 206,197,916 bytes. That file name is inferred from the patch's directory.
- Platforms: `sony_playstation3`.
- **Fullscreen & first run:**
  - Flags (verified): `--no-gui`, and `--fullscreen`, which is accepted only together with `--no-gui`. Also `--installfw`, `--installpkg`, `--config`, `--allow-any-location`.
  - It refuses to run from a temporary directory unless `--allow-any-location` is passed.
  - First run: a welcome dialog while `[main_window] infoBoxEnabledWelcome` is true. The key is verified. The file name `GuiConfigs/CurrentSettings.ini` is **unverified**.
  - Portable: a `portable/` directory next to the exe (verified). On Windows it defaults to the exe directory, and `RPCS3_CONFIG_DIR` overrides it.

### Azahar, 3DS (`azahar`), GPL-3.0. Lime3DS merged into it (`Lime3DS/lime3ds-archive` is archived)

| OS | Kind | Repo | Regex | Verified asset (`2126.2`, 2026-10-08) | Format | Track |
|---|---|---|---|---|---|---|
| windows | gh-latest | `azahar-emu/azahar` | `^azahar-windows-msvc-[\d.]+\.zip$` | `azahar-windows-msvc-2126.2.zip` (also `-installer.exe`, and msys2/mxe builds) | zip | Yes |
| linux | gh-latest | same | `^azahar\.AppImage$` (**no version in the name**) | `azahar.AppImage` | AppImage | Yes (by tag) |
| macos | gh-latest | same | `^azahar-macos-universal-[\d.]+\.zip$` | `azahar-macos-universal-2126.2.zip` (also arm64 and x86_64) | zip → .app | Yes |

- The release also has libretro core zips for each OS: `azahar-libretro-linux-x86_64-2126.2.zip`, and so on.
- Flathub: `org.azahar_emu.Azahar` (2126.2). The old id `io.github.lime3ds.Lime3DS` is still listed at 2119.1.
- BIOS: none for decrypted games. Azahar does not take encrypted dumps.
- Platforms: `nintendo_3ds` *(new)*.
- **Fullscreen & first run:**
  - Flags (verified in `citra_qt.cpp`): `-f/--fullscreen`, then the ROM path as a positional argument.
  - Windowed is the default. No wizard is known.
  - Portable: a `user` directory next to the exe (verified, `GetExeDirectory()/USERDATA_DIR`).

### Vita3K (`vita3k`), GPL-2.0

| OS | Kind | Repo | File | Verified (`continuous`, 2026-10-27, "Vita3K Build: 4236") | Format | Track |
|---|---|---|---|---|---|---|
| windows | gh-rolling | `Vita3K/Vita3K` | `^windows-latest\.zip$` | yes | zip | Yes (by date or the build number in the body) |
| linux | gh-rolling | same | `^Vita3K-x86_64\.AppImage$` | yes (+ `.zsync`, also `ubuntu-latest.zip`) | AppImage | Yes |
| macos | gh-rolling | same | `^macos-latest\.dmg$` (x64 **unverified**), `^macos-arm64-latest\.dmg$` | yes | dmg | Yes |

- Flathub: none.
- Firmware: **required** for many games. There are two packages, both on Sony's servers, listed at `http://fus01.psp2.update.playstation.net/update/psp2/list/us/psp2-updatelist.xml` (verified):
  - The system PUP, 3.74: `…/2022_0209/rel_f2c7b12fe85496ec88a0391b514d6e3b/PSP2UPDAT.PUP`.
  - The `preinst` recovery package (`…/pre_efd1ef6c…/PSP2UPDAT.PUP`, 128 MB). It is probably the "font firmware" Vita3K asks for (**unverified**).
  - Install either with `--firmware <pup>`.
- Platforms: `sony_psvita` *(new)*.
- **Fullscreen & first run:**
  - Flags (verified in `config.cpp`): `-F/--fullscreen`, `-r/--installed-path <TitleID>` (run an installed app), a positional `content-path` (install and run a .vpk or .zip), `--pkg` + `--zrif`, `--firmware`, `--frontend sdl` (plain window, quits with the app), `-c/--config-location`.
  - Games must be installed first, then launched by title id.
  - First run: the initial setup screens show while `initial-setup: false` in `config.yml`. Pre-seed `initial-setup: true` and `pref-path`.

### xemu, Xbox (`xemu`), GPL-2.0

| OS | Kind | Repo | Regex | Verified asset (`v0.8.139`, 2026-10-21) | Format | Track |
|---|---|---|---|---|---|---|
| windows | gh-latest | `xemu-project/xemu` | `^xemu-[\d.]+-windows-x86_64\.zip$` | `xemu-0.8.139-windows-x86_64.zip` (exclude `-dbg-` and `-pdb`) | zip | Yes |
| linux | gh-latest | same | `^xemu-[\d.]+-x86_64\.AppImage$` | `xemu-0.8.139-x86_64.AppImage` | AppImage | Yes |
| macos | gh-latest | same | `^xemu-[\d.]+-macos-universal\.zip$` | `xemu-0.8.139-macos-universal.zip` | zip → .app | Yes |

- Flathub: `app.xemu.xemu`.
- BIOS: **required**. These need user dumps:
  - The MCPX boot ROM (`mcpx_1.0.bin`, 512 B).
  - A flash BIOS (e.g. `Complex_4627.bin`, 1 MB).
- The HDD image can be downloaded legally: `xemu-project/xemu-hdd-image` (archived), release `1.0`, `xbox_hdd.qcow2.zip`. EmuDeck uses it.
- Platforms: `microsoft_xbox` *(new)*.
- **Fullscreen & first run:**
  - Flags (verified): `-dvd_path <iso>`, `-config_path <xemu.toml>`, and the QEMU `-full-screen`.
  - Windowed is the default, unless `display.window.fullscreen_on_startup` is set.
  - First run: a welcome window while `general.show_welcome` is true (verified).
  - Portable: `xemu.toml` next to the exe (verified).
  - Pre-seed `[general] show_welcome = false` and `[sys.files] bootrom_path / flashrom_path / hdd_path`. Those key names are **unverified**.

### Xenia Canary, Xbox 360 (`xenia`), BSD-3-Clause

| OS | Kind | Repo | File | Verified (tag `48b318a`, name `48b318a_canary_experimental`, 2026-10-27) | Format | Track |
|---|---|---|---|---|---|---|
| windows | gh-rolling (one tag per commit) | `xenia-canary/xenia-canary` | `^xenia_canary_windows\.7z$` | yes | 7z portable | Yes (by date) |
| linux | gh-rolling | same | `^xenia_canary_linux\.AppImage$` (experimental) | yes | AppImage | Yes |
| macos | gh-latest | `xenios-jp/XeniOS` (a port, BSD-3) | `^xenios_macos_universal\.dmg$` | `release-v2.0.1-build-9730`, 2026-06-08 | dmg | Yes |

- `xenia-canary/xenia-canary-releases` is **stale**: its last release is 2026-03-24 (`xenia_canary_windows_.zip`, `xenia_canary_linux.tar_.xz`).
- Xenia master: `xenia-project/release-builds-windows`, `v1.0.2844-master` (2026-02-18), `xenia_master.zip`. Windows only.
- ES-DE also lists "Xenia Edge" (`xenia_edge*.AppImage`). Its repository is **unverified**.
- Flathub: none.
- BIOS: none.
- Platforms: `microsoft_xbox360` *(new)*.
- **Fullscreen & first run:**
  - `--fullscreen` (cvar `DEFINE_bool(fullscreen, false)`, verified). The target is positional.
  - Windowed is the default. No wizard.
  - Portable: on by default on Windows (`portable` cvar default `true`). Elsewhere, `portable.txt` (verified).

### shadPS4, PS4 (`shadps4`), GPL-2.0

| OS | Kind | Repo | Regex | Verified asset (`v.0.20.0`, 2026-10-14, note the `v.` prefix) | Format | Track |
|---|---|---|---|---|---|---|
| windows | gh-latest | `shadps4-emu/shadPS4` | `^shadps4-win64-sdl-.*\.zip$` | `shadps4-win64-sdl-2026-0.20.0.zip` | zip | Yes |
| linux | gh-latest | same | `^shadps4-linux-sdl-.*\.zip$` | `shadps4-linux-sdl-0.20.0.zip` | zip (AppImage inside **unverified**) | Yes |
| macos | gh-latest | same | `^shadps4-macos-sdl-.*\.zip$` | `shadps4-macos-sdl-2026-0.20.0.zip` | zip | Yes |

- These are SDL (CLI) builds. The Qt GUI is a separate repo, `shadps4-emu/shadps4-qtlauncher`, which has **prereleases only** (`shadPS4QtLauncher-2026-10-26-811fb03…`, with `shadPS4QtLauncher-{linux,macos,win64}-qt-<date>-<sha>.zip`).
- Flathub: `net.shadps4.shadPS4` (0.20.0).
- Firmware: some games need modules dumped from the user's PS4 in `sys_modules/` (README). Fonts too.
- Platforms: `sony_playstation4` *(new)*.
- **Fullscreen & first run:**
  - Flags (verified in `src/main.cpp`): `-g/--game <path or ID>` (or positional), `-f/--fullscreen true|false`, `-b/--big-picture`, `--override-root`, `--add-game-folder`.
  - The SDL build has no wizard.

### Eden, Switch (`eden`), GPL-3.0

| OS | Kind | Source | Regex | Verified asset (`v0.2.1`, 2026-06-01) | Format | Track |
|---|---|---|---|---|---|---|
| all | forgejo | `https://git.eden-emu.dev/api/v1/repos/eden-emu/eden/releases/latest` | | yes | | Yes |
| windows | forgejo | same | `^Eden-Windows-v[\d.]+-amd64-msvc-standard\.zip$` (or `-amd64-clang-pgo.zip`) | `Eden-Windows-v0.2.1-amd64-msvc-standard.zip` | zip | Yes |
| linux | forgejo | same | `^Eden-Linux-v[\d.]+-amd64-clang-pgo\.AppImage$` | `Eden-Linux-v0.2.1-amd64-clang-pgo.AppImage` (also `-gcc-standard`, `-legacy`, `-steamdeck`, `-rog-ally`) | AppImage | Yes |
| macos | forgejo | same | `^Eden-macOS-v[\d.]+\.dmg$` | `Eden-macOS-v0.2.1.dmg` | dmg | Yes |

- Flathub: `dev.eden_emu.eden` (0.2.1).
- The GitHub mirror `eden-emulator/Releases` returns **HTTP 451 (DMCA)** since 2026-03-05.
- Firmware: **required**, `prod.keys` plus firmware from the user's console.
- Platforms: `nintendo_switch`.
- **Fullscreen & first run:**
  - `-f -g <rom>` (per ES-DE `es_systems.xml`; yuzu lineage). `-u/-input-profile` is verified in `src/yuzu/main.cpp`.
  - Portable: a `user` directory next to the exe (verified, `GetExeDirectory()/PORTABLE_DIR`).

### Citron, Switch, GPL-3.0 (flag only)

- `citron-neo/emulator`, latest `2026-04-27` (date tag), assets named nightly, e.g. `citron_nightly-0237a9b88-linux-x86_64.AppImage`, `Citron-windows-nightly-0237a9b88-x64-msvc.zip`, `Citron-macOS-nightly-0237a9b88.dmg`.
- No releases since April. Many `Citron-AppImage` repositories were taken down by the DMCA notice. Not on Flathub. **Don't offer it as a default.**

### ares, multi-system (`ares`), ISC

| OS | Kind | Repo | Regex | Verified asset (`v148`, 2026-05-30) | Format | Track |
|---|---|---|---|---|---|---|
| windows | gh-latest | `ares-emulator/ares` | `^ares-windows-x64\.zip$` | yes | zip | Yes (by tag) |
| linux | flathub / community | Flathub `dev.ares.ares` (148), or `pkgforge-dev/ares-emu-appimage` `^ares-v\d+-anylinux-x86_64\.AppImage$` | `ares-v148-anylinux-x86_64.AppImage` | yes | AppImage | Yes |
| macos | gh-latest | same | `^ares-macos-universal\.zip$` | yes | zip → .app | Yes |

- Upstream ships **no Linux binary**.
- BIOS: per system, e.g. PS1 BIOS and N64DD IPL.
- Platforms (candidates): `nintendo_n64`, `nintendo_nes`, `nintendo_snes`, `sega_genesis`, `sega_mastersystem`, `nintendo_gb`/`gbc`/`gba`, and more.
- **Fullscreen & first run:**
  - Flags (verified in `desktop-ui.cpp`): `--fullscreen`, `--system "<name>"`, `--no-file-prompt`, `--settings-file <path>`, `--setting k=v`, `--kiosk`.
  - Windowed is the default. No wizard.

### Rosalie's Mupen GUI, N64 (`rmg`), GPL-3.0

| OS | Kind | Repo | Regex | Verified asset (`v0.9.0`, 2026-06-14) | Format | Track |
|---|---|---|---|---|---|---|
| windows | gh-latest | `Rosalie241/RMG` | `^RMG-Portable-Windows64-v[\d.]+\.zip$` | `RMG-Portable-Windows64-v0.9.0.zip` | zip portable | Yes |
| linux | gh-latest | same | `^RMG-Portable-Linux64-v[\d.]+\.AppImage$` | `RMG-Portable-Linux64-v0.9.0.AppImage` | AppImage | Yes |
| macos | none | | | none | | n/a |

- Flathub: `com.github.Rosalie241.RMG`.
- BIOS: none. The 64DD IPL is optional.
- Platforms: `nintendo_n64`.
- **Fullscreen & first run:** flags (verified) are `-f/--fullscreen`, `-n/--nogui`, `-q/--quit-after-emulation`, `--disk <64DD>`.

### gopher64, N64 (`gopher64`), GPL-3.0

| OS | Kind | Repo | Regex | Verified asset (`v1.1.42`, 2026-10-15) | Format | Track |
|---|---|---|---|---|---|---|
| windows | gh-latest | `gopher64/gopher64` | `^gopher64-windows-x86_64\.exe$` | yes | bare exe | Yes |
| linux | gh-latest | same | `^gopher64-linux-x86_64$` | yes | bare ELF (chmod +x) | Yes |
| macos | gh-latest | same | `^gopher64-macos-aarch64\.zip$` (arm64 only) | yes | zip | Yes |

- Flathub: `io.github.gopher64.gopher64`.
- BIOS: none.
- Fullscreen is `-f` (per ES-DE, **unverified** in source).
- simple64 (`simple64/simple64`) was **archived** on 2025-02-14 (last `v2024.12.1`, Windows only). Drop it.

### Flycast, Dreamcast, Naomi, Atomiswave (`flycast`), GPL-2.0

| OS | Kind | Repo | Regex | Verified asset (`v2.7`, 2026-08-19) | Format | Track |
|---|---|---|---|---|---|---|
| windows | gh-latest | `flyinghead/flycast` | `^flycast-win64-[\d.]+\.zip$` | `flycast-win64-2.7.zip` | zip | Yes |
| linux | gh-latest | same | `^flycast-x86_64-[\d.]+\.AppImage$` | `flycast-x86_64-2.7.AppImage` | AppImage | Yes |
| macos | gh-latest | same | `^flycast-macOS-[\d.]+\.zip$` | `flycast-macOS-2.7.zip` | zip → .app | Yes |

- Flathub: `org.flycast.Flycast`.
- BIOS:
  - Dreamcast: optional (`dc_boot.bin`, `dc_flash.bin`); the HLE BIOS ("reios") is built in.
  - Naomi/Atomiswave: **required** (`naomi.zip`, `awbios.zip`).
- Platforms: `sega_dreamcast`, `arcade` (Naomi/Atomiswave).
- **Fullscreen & first run:**
  - There is no fullscreen flag. Use `-config window:fullscreen=yes` (transient `-config section:key=value` in `cl.cpp`; key `window`/`fullscreen` in `sdl.cpp`, both verified).
  - The ROM is positional. No wizard.

### MAME (`mame`), GPL-2.0+

| OS | Kind | Source | Regex | Verified (`mame0289`, 2026-07-30) | Format | Track |
|---|---|---|---|---|---|---|
| windows | gh-latest | `mamedev/mame` | `^mame\d+b_x64\.exe$` | `mame0289b_x64.exe` | self-extracting 7z | Yes |
| linux | flathub | `org.mamedev.MAME` (0.289) | | yes | Flatpak | Via Flatpak |
| macos | page | `https://sdlmame.lngn.net/` → `stable/mame0289-arm64.zip` (a build by a MAME developer; x86 builds stop at 0286) | | yes | zip | Download only (version comes from the GitHub tag) |

- `mame0289lx.zip` is the `-listxml` output, **not** a Linux build.
- BIOS: system BIOS romsets (`neogeo.zip`, `pgm.zip`, …) live in the ROM path.
- Platforms: `arcade`.
- **Fullscreen & first run:**
  - **Fullscreen is the default.** `-window`/`-w` makes it windowed. `-skip_gameinfo` skips the info screen (both verified). Use `-rompath <dir> <set>`.
  - No wizard.

### DOSBox Staging (`dosbox-staging`), GPL-2.0+

| OS | Kind | Repo | Regex | Verified asset (`v0.83.0`, 2026-08-27) | Format | Track |
|---|---|---|---|---|---|---|
| windows | gh-latest | `dosbox-staging/dosbox-staging` | `^dosbox-staging-windows-x64-v[\d.]+\.zip$` | yes | zip | Yes |
| linux | gh-latest | same | `^dosbox-staging-linux-x86_64-v[\d.]+\.tar\.xz$` | yes | tar.xz | Yes |
| macos | gh-latest | same | `^dosbox-staging-macOS-v[\d.]+\.dmg$` | yes | dmg | Yes |

- Flathub: `io.github.dosbox-staging`.
- BIOS: none.
- Platforms: `pc_dos` *(new)*.
- **Fullscreen & first run:** `--fullscreen` (verified in `src/main.cpp`). Windowed is the default.

### ScummVM (`scummvm`), GPL-3.0

| OS | Kind | Source | File | Verified | Format | Track |
|---|---|---|---|---|---|---|
| all | version | GitHub `scummvm/scummvm` `releases/latest` (no assets) | | `v2026.4.0`, 2026-10-11 | | gives the version |
| windows | gh-tag+url | `https://downloads.scummvm.org/frs/scummvm/{v}/scummvm-{v}-win32-x86_64.zip` | listed on `/downloads/` | yes | zip | Yes |
| linux | flathub | `org.scummvm.ScummVM` (2026.4.0). Debs: `scummvm_{v}-1_ubuntu24_04_amd64.deb` | | yes | Flatpak | Via Flatpak |
| macos | gh-tag+url | `…/scummvm-{v}-macosx.dmg` (and `-macosx-x86_64.dmg`) | | yes (arch of `macosx.dmg` **unverified**) | dmg | Yes |

- BIOS: none.
- Platforms: `scummvm` *(new)*.
- **Fullscreen & first run:**
  - Flags (verified in `base/commandLine.cpp`): `-f/--fullscreen`, `-F/--no-fullscreen`, `-p/--path=<dir>`, `--auto-detect` (detect and start the first game in `--path`).
  - No wizard, but a game must either be added or be auto-detected.

### MesenCE, NES/SNES/GB/GBA/SMS/PCE (`mesen`), GPL-3.0

| OS | Kind | Repo | Regex | Verified asset (`2.2.1`, 2026-06-05) | Format | Track |
|---|---|---|---|---|---|---|
| windows | gh-latest | `nesdev-org/MesenCE` | `^Mesen_[\d.]+_Windows\.zip$` | `Mesen_2.2.1_Windows.zip` | zip | Yes |
| linux | gh-latest | same | `^Mesen_[\d.]+_Linux_x64\.zip$` | `Mesen_2.2.1_Linux_x64.zip` | zip | Yes |
| macos | gh-latest | same | `^Mesen_[\d.]+_macOS_ARM64_AppleSilicon\.zip$` / `_macOS_x64_Intel\.zip$` | yes | zip | Yes |

- `SourMesen/Mesen2` is **archived**. Its README says "archive of Mesen as it was around version 2.1.1" and points to MesenCE.
- Flathub: none.
- BIOS: FDS `disksys.rom` (required for FDS). GBA, SGB and PCE-CD BIOS are optional or per system.
- Platforms: `nintendo_nes`, `nintendo_snes`, `nintendo_gb`, `nintendo_gbc`, `nintendo_gba`, `sega_mastersystem`.
- **Fullscreen & first run:**
  - `--fullscreen` and `--doNotSaveSettings` (verified in `CommandLineHelper.cs`).
  - Portable when `settings.json` sits next to the exe (`ConfigManager.cs`, verified). Otherwise data goes to Documents/MesenCE.
  - The first-run location prompt is **unverified**. Pre-seeding `settings.json` next to the exe avoids it.

### Ymir, Saturn (`ymir`), GPL-3.0. Moved from `StrikerX3/Ymir`; GitHub redirects

| OS | Kind | Repo | Regex | Verified asset (`v0.4.1`, 2026-10-18) | Format | Track |
|---|---|---|---|---|---|---|
| windows | gh-latest | `ymir-emu/Ymir` | `^ymir-windows-x86_64-AVX2-v[\d.]+\.zip$` (SSE2 fallback) | `ymir-windows-x86_64-AVX2-v0.4.1.zip` | zip | Yes |
| linux | gh-latest | same | `^ymir-linux-x86_64-AVX2-v[\d.]+\.tar\.xz$` | yes | tar.xz | Yes |
| macos | gh-latest | same | `^ymir-macos-(arm64\|x64)-v[\d.]+\.zip$` | yes | zip | Yes |

- Flathub: `io.github.strikerx3.ymir`.
- BIOS: **required**, the Saturn IPL ROM (`sega_101.bin` JP, `mpr-17933.bin` US/EU). It goes in `<profile>/roms/ipl` and is auto-selected (README).
- Platforms: `sega_saturn`.
- **Fullscreen & first run:** flags (verified) are `-f/--fullscreen`, `-d/--disc <image>`, `-p/--profile <dir>`, `-u/--user`.

### Others, briefly

| Emulator | Repo | Latest (verified) | Assets | Notes |
|---|---|---|---|---|
| SameBoy (GB/GBC) | `LIJI32/SameBoy` | `v1.0.4`, 2026-10-09 | `sameboy_winsdl_v1.0.4.zip`, `sameboy_cocoa_v1.0.4.zip` | No Linux binary; Flathub `io.github.sameboy.SameBoy` (1.0.3). Boot ROMs are built in. `-f` per ES-DE. |
| Snes9x | `snes9xgit/snes9x` | `1.63`, 2024-07-09 | `snes9x-1.63-win32-x64.zip`, `Snes9x-1.63-x86_64.AppImage`, `Snes9x-1.63-Mac.zip` | Flathub `com.snes9x.Snes9x`. Infrequent releases. |
| Stella (Atari 2600) | `stella-emu/stella` | `7.0`, 2024-10-05 | `Stella-7.0c-windows.zip`, `stella_7.0_amd64.deb`, `Stella-7.0-macos.dmg` | Flathub `io.github.stella_emu.Stella`. Watch the `7.0c` suffix. |
| DeSmuME | `TASEmulators/desmume` | `release_0_9_13`, 2022 | win64 zip, macOS dmg | Stale; prefer melonDS. Flathub `org.desmume.DeSmuME`. |
| bsnes | `bsnes-emu/bsnes` | `v115`, 2020 | `bsnes_v115-windows.zip` | Stale; prefer MesenCE or ares. |
| Kronos (Saturn) | `FCare/Kronos` | `2.7.0_official_release`, 2025-01 | one Ubuntu asset | Stale; prefer Ymir. |
| Mednafen | mednafen.github.io/releases/ | 1.32.1 | `files/mednafen-1.32.1-win64.zip` | Page only, Windows only; download only. |

## 4. RetroArch cores

- **Per-core nightly zip:**
  - URL: `https://buildbot.libretro.com/nightly/{target}/latest/{core}_libretro.{ext}.zip`.
  - Targets: `windows/x86_64` (`dll`), `linux/x86_64` (`so`), `apple/osx/x86_64` and `apple/osx/arm64` (`dylib`).
  - `linux/armhf` is stale (2020).
- **Index with change detection:** `…/latest/.index-extended`. Each line is `YYYY-MM-DD <crc32> <file>`, e.g. `2026-10-28 7d0ae6af gambatte_libretro.so.zip`.
  - The CRC32 is of the **unzipped core binary**, not the zip. Verified: gambatte's zip CRC is `e2c3d282`, and the `.so` inside it is `7d0ae6af`.
  - Compare it with the CRC of the installed core to decide whether to update. This is what RetroArch's own core updater does.
  - `.index` is the plain file list. Entries per target: 253 (Linux), 251 (Windows), 242 (macOS x64), 237 (macOS arm64).
  - HTTP `Last-Modified` and `ETag` are also sent.
- **Stable bundle:** `https://buildbot.libretro.com/stable/{v}/{os}/{arch}/RetroArch_cores.7z` (Windows and Linux). It has no per-core index.
- **Core info, including firmware:** `https://buildbot.libretro.com/assets/frontend/info.zip` (200, 290 KB), or `libretro/libretro-core-info` (MIT). Example from `mednafen_saturn_libretro.info`:
  - `firmware0_path = "sega_101.bin"`, `firmware0_opt = "false"`.
  - `notes` carries the MD5s.
- **Availability:** all 13 cores `retroarch.toml` uses are on all 4 targets: `mesen`, `snes9x`, `mupen64plus_next`, `gambatte`, `mgba`, `desmume`, `dolphin`, `genesis_plus_gx`, `mednafen_saturn`, `flycast`, `mednafen_psx_hw`, `ppsspp`, `fbneo`. So are `mame`, `swanstation`, `melondsds`, `pcsx2`, `azahar`, `citra`, `sameboy`, `bsnes`, `picodrive`, `gearsystem`, `nestopia`, `fceumm`, `parallel_n64` and `gpsp`.
- **Core BIOS needs (libretro .info):**
  - `mednafen_psx_hw`: `scph5500.bin`, `scph5501.bin` or `scph5502.bin`. One per region.
  - `mednafen_saturn`: `sega_101.bin` and/or `mpr-17933.bin`.
  - `flycast`: `dc/dc_boot.bin` (optional).
  - `mgba`: `gba_bios.bin` (optional).
  - `mesen`: `disksys.rom` (FDS).
  - `genesis_plus_gx`: Sega CD `bios_CD_U.bin`, `bios_CD_E.bin`, `bios_CD_J.bin`.
  - `fbneo` and `mame`: `neogeo.zip` and others, in the ROM directory.

## 5. RomM firmware API

From `rommapp/romm` (AGPL-3.0), `backend/endpoints/firmware.py` and `endpoints/responses/firmware.py`. The same on master and release `5.4.1`. All paths are under `/api` (`main.py` mounts the routers with `prefix="/api"`).

| Method and path | Scope | Params | Returns |
|---|---|---|---|
| `GET /api/firmware` | `firmware.read` | `platform_id: int` (optional), `missing: bool` (optional; files missing from disk) | `FirmwareSchema[]` |
| `GET /api/firmware/identifiers` | `firmware.read` | | `int[]` (firmware ids). Present in master and 5.4.1 |
| `GET /api/firmware/{id}` | `firmware.read`, or none when `DISABLE_DOWNLOAD_ENDPOINT_AUTH` | | `FirmwareSchema` |
| `GET /api/firmware/{id}/content/{file_name}` | same | `file_name` is required in the path but ignored ("due to a bug in emulatorjs") | the file (`FileResponse`). 404 if `missing_from_fs` |
| `HEAD /api/firmware/{id}/content/{file_name}` | same | | headers, with `Content-Length` = `file_size_bytes` |
| `POST /api/firmware?platform_id=` | `firmware.write` | multipart `files[]` | `{uploaded, firmware[]}` |
| `POST /api/firmware/delete` | `firmware.write` | JSON `{firmware: int[], delete_from_fs: int[]}` | `{successful_items, failed_ids, errors}` |

- `FirmwareSchema` fields: `id`, `platform_id`, `file_name`, `file_name_no_tags`, `file_name_no_ext`, `file_extension`, `file_path`, `file_size_bytes`, `full_path`, `is_verified`, `crc_hash`, `md5_hash`, `sha1_hash`, `missing_from_fs`, `created_at`, `updated_at`.
- `PlatformSchema` (from `GET /api/platforms`) embeds `firmware: FirmwareSchema[]`. One platforms call is enough to see which firmware each platform has.
- `is_verified` means the file matched `known_bios_files.json` (`"<slug>:<file_name>"` → size, crc, md5, sha1).
- **Platform slugs** (`backend/utils/platform_slugs.py`, `UniversalPlatformSlug`):

| Caskade id | RomM slug | | Caskade id | RomM slug |
|---|---|---|---|---|
| nintendo_nes | `nes` | | sega_mastersystem | `sms` |
| nintendo_snes | `snes` | | sega_genesis | `genesis` (older IGDB slug `genesis-slash-megadrive`; `romm/sync.rs` already aliases it) |
| nintendo_n64 | `n64` | | sega_saturn | `saturn` |
| nintendo_gb / gbc / gba | `gb` / `gbc` / `gba` | | sega_dreamcast | `dc` |
| nintendo_ds | `nds` | | sony_playstation | `psx` (older `ps`) |
| nintendo_gamecube | `ngc` | | sony_playstation2 / 3 | `ps2` / `ps3` |
| nintendo_wii / wiiu | `wii` / `wiiu` | | sony_psp | `psp` |
| nintendo_switch | `switch` | | arcade | `arcade` |
| nintendo_3ds *(new)* | `3ds` | | sony_psvita *(new)* | `psvita` |
| sony_playstation4 *(new)* | `ps4` | | microsoft_xbox / xbox360 *(new)* | `xbox` / `xbox360` |
| pc_dos *(new)* | `dos` | | scummvm *(new)* | `scummvm` |

- `fs_slug` is the folder name on the server and can differ (e.g. `megadrive`). Match on `slug`, then `fs_slug`, as `romm/sync.rs` does.

## 6. Proposed new platforms

| id | Name | ES-DE | Playnite | RomM | Main emulator |
|---|---|---|---|---|---|
| `nintendo_3ds` | Nintendo 3DS | `n3ds` | `nintendo_3ds` | `3ds` | Azahar |
| `sony_psvita` | PlayStation Vita | `psvita` | `sony_vita` | `psvita` | Vita3K |
| `sony_playstation4` | PlayStation 4 | `ps4` | `sony_playstation4` | `ps4` | shadPS4 |
| `microsoft_xbox` | Xbox | `xbox` | `xbox` | `xbox` | xemu |
| `microsoft_xbox360` | Xbox 360 | `xbox360` | `xbox360` | `xbox360` | Xenia Canary |
| `pc_dos` | DOS | `dos` | `pc_dos` | `dos` | DOSBox Staging |
| `scummvm` | ScummVM | `scummvm` | none | `scummvm` | ScummVM |
| `sega_cd` | Sega CD | `segacd` | `sega_cd` | `segacd` | RetroArch Genesis Plus GX |
| `nec_pcengine` | PC Engine / TurboGrafx-16 | `pcengine` | `nec_turbografx_16` | `tg16` | MesenCE / RetroArch Beetle PCE |
| `atari_2600` | Atari 2600 | `atari2600` | `atari_2600` | `atari2600` | Stella |

## 7. Recommended default per platform

"Track" means updates can be tracked automatically: **GH** (GitHub releases), **API** (the project's update endpoint), **FJ** (Forgejo), **Flatpak** (updates come from Flathub), or "download only".

| Platform | Default standalone | Track | RetroArch core | BIOS / firmware needed for first Play |
|---|---|---|---|---|
| nintendo_nes | MesenCE | GH | mesen | none (FDS: `disksys.rom`) |
| nintendo_snes | MesenCE (Snes9x as an alternative) | GH | snes9x | none |
| nintendo_n64 | RMG (Win/Linux), gopher64 (macOS arm64) | GH | mupen64plus_next | none |
| nintendo_gb / nintendo_gbc | mGBA (SameBoy as an alternative) | GH | gambatte | none |
| nintendo_gba | mGBA | GH | mgba | none (BIOS optional) |
| nintendo_ds | melonDS | GH | melondsds / desmume | none (DSi: yes) |
| nintendo_gamecube / nintendo_wii | Dolphin | API | dolphin | none |
| nintendo_wiiu | Cemu | GH | none | none (`keys.txt` for encrypted dumps) |
| nintendo_switch | none by default (Eden or Ryubing, opt in) | FJ / unverified | none | **prod.keys + firmware (user)** |
| sega_mastersystem / sega_genesis | ares (Win/mac; Flatpak on Linux) | GH | genesis_plus_gx | none |
| sega_saturn | Ymir | GH | mednafen_saturn | **IPL ROM (user)** |
| sega_dreamcast | Flycast | GH | flycast | none (HLE) |
| sony_playstation | DuckStation | GH (rolling) | mednafen_psx_hw / swanstation | **PS1 BIOS (user)** |
| sony_playstation2 | PCSX2 | GH | none recommended | **PS2 BIOS (user)** |
| sony_playstation3 | RPCS3 | API / GH | none | PS3UPDAT.PUP (**downloadable from Sony**) |
| sony_psp | PPSSPP | GH | ppsspp | none |
| arcade | MAME (Windows; Flatpak on Linux; sdlmame on macOS) | GH / Flatpak / download only | fbneo, mame | BIOS romsets |
| nintendo_3ds *(new)* | Azahar | GH | azahar | none |
| sony_psvita *(new)* | Vita3K | GH (rolling) | none | PUP (**downloadable from Sony**) |
| sony_playstation4 *(new)* | shadPS4 | GH | none | some games need `sys_modules` (user) |
| microsoft_xbox *(new)* | xemu | GH | none | **MCPX + flash BIOS (user)**; HDD image downloadable |
| microsoft_xbox360 *(new)* | Xenia Canary (Win/Linux), XeniOS (macOS) | GH (rolling) | none | none |
| pc_dos *(new)* | DOSBox Staging | GH | dosbox_pure | none |
| scummvm *(new)* | ScummVM | GH tag + URL / Flatpak | scummvm | none |

### Takedowns, moves and other surprises

- **Nintendo's DMCA notice of 2026-02-12** ([github/dmca 2026-02-12-nintendo.md](https://github.com/github/dmca/blob/master/2026/02/2026-02-12-nintendo.md)):
  - It names "Citron, Eden, Kenji-NX, MeloNX, Pine, Pomelo, Ryubing, Ryujinx, Skyline, Sudachi, Sumi, Suyu, and Yuzu" as circumvention tools.
  - GitHub processed it against whole fork networks. `eden-emulator/Releases` has returned 451 since 2026-03-05.
  - Eden lives on at git.eden-emu.dev, and Ryubing at git.ryujinx.app (both off GitHub). `citron-neo/emulator` survives on GitHub but has been quiet since April.
  - GitHub search for Switch emulators returns mostly malware-bait repositories. Always pin exact hosts.
- **Repos that moved:** Mesen2 → `nesdev-org/MesenCE`; `StrikerX3/Ymir` → `ymir-emu/Ymir`; Lime3DS → Azahar; Ryujinx → Ryubing (off GitHub).
- **Archived:** `simple64/simple64`, `SourMesen/Mesen2`, `Lime3DS/lime3ds-archive`, `xemu-project/xemu-hdd-image` (its release is still downloadable).
- **Rolling or nightly-only releases:**
  - DuckStation (`latest` tag), Vita3K (`continuous`), and Xenia Canary (one tag per commit) re-publish under fixed or hash tags.
  - RPCS3 publishes per commit in separate binaries repos, and its main repo's releases have no assets.
  - PCSX2 nightlies are prereleases, but `releases/latest` is stable.
  - The shadPS4 Qt launcher has prereleases only.
- **Flathub DuckStation is EOL** after the license changed to CC-BY-NC-ND-4.0.
- **Gaps with no official binary:**
  - ares, SameBoy and MAME on Linux; RMG on macOS.
  - Dolphin stable on Linux comes only as a Flatpak bundle or from Flathub; the AppImage is dev-only.
  - Cemu's Mac build is Intel-only.
  - gopher64 on macOS is arm64-only.
- **Confusing asset names:**
  - `mame0289lx.zip` is listxml, not Linux.
  - `azahar.AppImage` has no version in its name.
  - The mGBA AppImage extension is lowercase (`.appimage`).
  - The shadPS4 tag has a `v.` prefix.
  - The Stella Windows build is `7.0c`.
