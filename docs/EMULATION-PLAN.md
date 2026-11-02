# Caskade — Emulation Plan

Status: core implemented; plugin definitions and ROM importers (E5) pending · Last updated: 2026-11-02

How Caskade finds emulators, configures them, turns ROM folders into library entries, and launches them. The model is Playnite's emulation support with its gaps fixed. This doc follows the conventions in `TECH-PLAN.md`, and it fills in the **Platform / Emulator / EmulatorProfile** row of TECH-PLAN §3 and the emulator side of the launch pipeline.

---

## 1. Goals and non-goals

**Goals**

- A user with emulators already installed gets a working setup without typing a path or an argument: detect emulators in standard locations, enable the built-in profiles that are actually present, and point a scanner at a ROM folder.
- Works the same on Linux, Windows and macOS: Flatpaks, AppImages, `$PATH` binaries, `.app` bundles and portable folders are all first-class installs.
- Anyone can still configure anything: custom profiles, per-game argument overrides, custom platforms.
- Every platform has a **default emulator and profile**. Games follow it unless they're pinned, so switching all SNES games to another core is one change.
- Multi-disc games, playlists (`.cue`, `.gdi`, `.m3u`) and archives import as one game, not one entry per file.
- BIOS and firmware status is visible before a launch fails.
- Emulator definitions are data, contributed by Caskade and by plugins in the same format.

**Non-goals (prototype)**

- Checksum identification against No-Intro/Redump/libretro DATs (out of scope in TECH-PLAN §8). The pipeline leaves a slot for it (§6.2, step 5).
- Configuring the emulators themselves (controllers, shaders, per-core options). Caskade launches emulators; it doesn't replace their settings UIs. The concept art's "Controller Profiles" and "Graphics & Shaders" tools are post-prototype plugin territory.
- Managed emulator installs, core downloads and BIOS retrieval are implemented separately by [`EmulatorSetup`](../crates/caskade-core/src/emulation/setup/mod.rs); see the [shipped catalogue guide](../crates/caskade-sources/emulation/README.md).
- Arcade set management (MAME merged/split sets). Arcade works through `{ImageNameNoExt}` profiles only.
- Cloud save sync (the "Cloud Saves" column in the concept art).

---

## 2. Concepts

| Concept                | Owner | What it is                                                                                                                                                       |
| ---------------------- | ----- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Platform**           | data  | A system that games run on: `nintendo_snes`, `sony_playstation2`. Name, short name, default extensions, BIOS list, external ids (IGDB, libretro DB, Playnite, ES-DE). |
| **Region**             | data  | Id, name, and the codes found in ROM names (`USA`, `U`, `Europe`, `E`, `Japan`, `J`, …).                                                                         |
| **EmulatorDefinition** | data  | One emulator as Caskade knows it: per-OS detection rules and a list of **definition profiles**.                                                                 |
| **Emulator**           | user  | One installed emulator: name, install, the definition it follows (if any), its profiles.                                                                         |
| **EmulatorProfile**    | user  | How to run one kind of ROM: either **built-in** (references a definition profile, may override arguments) or **custom** (everything user-entered).             |
| **PlatformSettings**   | user  | Per platform: default emulator + profile, ROM folders, BIOS folder override.                                                                                     |
| **RomScanner**         | user  | A saved scan: folder, platform, profile used to match files, options, exclusions. Playnite's "auto-scan configuration".                                          |
| **GameRom**            | user  | A ROM file of a game: path (stored relative to its scanner's folder), display name (`Disc 2`), size.                                                             |

"Data" means shipped with Caskade or contributed by a plugin, read-only to the user, versioned. "User" means stored in SQLite and edited in Settings → Emulation.

## 3. Definitions (data files)

Definitions live in `crates/caskade-sources/emulation/` as TOML (TECH-PLAN §3 already picked TOML) and are embedded in the binary. Plugins contribute more through `contributes.emulation` in `plugin.json` (§8). Later files with the same id replace earlier ones, so a plugin can fix a shipped definition.

We bootstrap the Windows half of the definitions from Playnite's `emulator.yaml` and `Platforms.yaml` (MIT, credited in the files), then add Linux and macOS rules by hand for the prototype set (§10).

### 3.1 Platform

```toml
[[platform]]
id = "nintendo_snes"
name = "Super Nintendo Entertainment System"
short = "SNES"
extensions = ["sfc", "smc", "fig", "bs", "st", "zip", "7z"] # identifies a stray ROM with no scanner (installer hand-off)
ids = { igdb = 19, libretro = "Nintendo - Super Nintendo Entertainment System", playnite = "nintendo_super_nes", esde = "snes" }

[[platform]]
id = "sony_playstation"
name = "Sony PlayStation"
short = "PS1"
extensions = ["cue", "chd", "pbp", "m3u", "iso", "bin"]
ids = { igdb = 7, libretro = "Sony - PlayStation", playnite = "sony_playstation", esde = "psx" }
bios = [
  { file = "scph5501.bin", md5 = "490f666e1afb15b7362b406ed1cea246", region = "usa", required = true },
  { file = "scph5502.bin", md5 = "32736f17079d0b2b7024407c39bd3050", region = "europe", required = false },
]
```

Caskade's ids are its own; `ids.playnite` and `ids.esde` exist so configs from those tools can be imported later.

`ids.hasheous` names the platform's [Hasheous data dump](https://hasheous.org/index.html?page=dumps) (the zip's name without `.zip`, e.g. `"Sega Mega Drive _ Genesis"`). `caskade_core::hasheous` downloads the dump of every platform a configured emulator runs, plus `Microsoft Windows` when the library has PC games and `Apple Mac` for native games on macOS (Hasheous has no Linux dump). The dumps are kept and indexed under `<cache>/hasheous/`, so ROM hashes and titles resolve offline. Platforms without a dump, such as the Switch, leave it out.

### 3.2 Emulator definition

```toml
id = "retroarch"
name = "RetroArch"
website = "https://www.retroarch.com/"

# Where to look and what to look for, per OS. Any rule that matches makes an install.
[detect.windows]
executable = '^retroarch\.exe$'
locations = ["{ProgramFiles}/RetroArch", "{AppData}/RetroArch", "{UserHome}/scoop/apps/retroarch/current"]

[detect.linux]
executable = '^retroarch$'
flatpak = "org.libretro.RetroArch"
path = ["retroarch"] # binary names looked up in $PATH
appimage = '^RetroArch.*\.AppImage$'

[detect.macos]
bundle = "com.libretro.RetroArch"

# Variables the profiles below can use. Per-OS values win over the shared ones.
[vars]
cores = "{EmulatorDir}/cores"
lib = ".dll"
[vars.linux]
cores = "{XdgConfig}/retroarch/cores"
lib = ".so"
[vars.linux.flatpak]
cores = "{UserHome}/.var/app/org.libretro.RetroArch/config/retroarch/cores"
[vars.macos]
cores = "{AppSupport}/RetroArch/cores"
lib = ".dylib"

[[profile]]
name = "Snes9x"
platforms = ["nintendo_snes"]
extensions = ["sfc", "smc", "fig", "swc", "bs", "zip", "7z"]
args = ["-L", "{cores}/snes9x_libretro{lib}", "{RomPath}"]
requires = ["{cores}/snes9x_libretro{lib}"] # profile is "available" only if these exist
bios = []                                   # extra BIOS beyond the platform's list

[[profile]]
name = "Beetle PSX HW"
platforms = ["sony_playstation"]
extensions = ["cue", "chd", "pbp", "m3u", "toc", "exe"]
args = ["-L", "{cores}/mednafen_psx_hw_libretro{lib}", "{RomPath}"]
requires = ["{cores}/mednafen_psx_hw_libretro{lib}"]
```

Differences from Playnite's `emulator.yaml`, and why:

- **Per-OS detection with install kinds.** `executable` regex in a folder (Playnite's rule), plus `flatpak` app id, `$PATH` names, AppImage name, and macOS bundle id. The install kind decides how Caskade launches it (§5.1).
- **`vars` instead of hard-coded relative paths.** RetroArch cores live next to the exe on Windows but in `~/.config/retroarch/cores` on Linux and in a Flatpak's sandbox when installed as a Flatpak. Profiles are written once.
- **`args` is a list of argv tokens, not one string.** Variables are expanded inside each token, so `{RomPath}` with spaces stays one argument and nobody has to quote anything. A custom profile can still be written as one string; the UI splits it with shell-word rules when it's saved and shows the result (§7.2).
- **`requires` replaces `ProfileFiles`**, with variables, so it works off-folder.
- **BIOS lists** on platforms and profiles. Playnite has none.
- **`import`** names an importer for emulators whose games aren't plain files (Playnite's `ScriptGameImport`): `import = "rpcs3"` uses a built-in Rust importer, and plugins can provide others (§8). No PowerShell.

### 3.3 Variables

Available in `args`, `requires`, `vars`, working directory and tracking path. Names follow Playnite where one exists, so its community docs still apply.

| Variable                                          | Value                                                                    |
| ------------------------------------------------- | ------------------------------------------------------------------------ |
| `{RomPath}` (alias `{ImagePath}`)                 | Full path of the ROM being launched.                                     |
| `{RomName}` (alias `{ImageName}`)                 | File name with extension.                                                |
| `{RomNameNoExt}` (alias `{ImageNameNoExt}`)       | File name without extension (MAME/FBNeo set names).                     |
| `{RomDir}` (alias `{ImageDir}`)                   | Folder of the ROM.                                                       |
| `{EmulatorDir}`                                   | Emulator install folder (for Flatpak: the app's data dir).               |
| `{Name}`, `{Platform}`, `{GameId}`                | Game title, platform id, Caskade game id.                                |
| `{CaskadeDir}`, `{StorageRoot}`                   | Caskade's data dir and the storage root from Settings → Storage.         |
| `{UserHome}`, `{XdgConfig}`, `{AppData}`, `{ProgramFiles}`, `{AppSupport}` | OS folders; empty on OSes that don't have them. |
| any key from the definition's `vars`              |                                                                          |

Unknown variables are a validation error at save time, not a silent empty string at launch.

### 3.4 Save locations

Emulator definitions may declare save files and save states separately:

```toml
[saves]
files = ["{save_data}/saves/**/{RomNameNoExt}.srm"]
states = ["{save_data}/states/**/{RomNameNoExt}.state*"]
```

These use the same built-in and definition variables as profiles. Paths are
absolute glob templates; matching folders are scanned recursively. Runtime
ROM and device values are escaped as literal glob components, so a ROM named
`Game[1]` does not match another game's saves. Missing variables fail rather
than becoming empty strings. Catalogue validation checks variables, cycles,
absolute paths and glob syntax for every OS and install kind. Old definitions
without `[saves]` remain valid and have no automatic locations.

`caskade_core::saves::Saves::with_catalog` resolves an emulated release using
its pinned emulator/profile or platform default, with each disc's ROM path.
An ask-at-launch target has no known emulator destination and stays unknown.
The result tags save files `save` and save states `state`; per-release manual
locations take precedence. Global memory cards and emulator save folders stay
global where the emulator does not name them after the ROM. The shipped
defaults, source references and custom-directory limitations are documented
in the [emulation README](../crates/caskade-sources/emulation/README.md).

## 4. User configuration

Stored in SQLite (TECH-PLAN §3). Ids are UUIDv7.

```rust
pub struct Emulator {
    pub id: EmulatorId,
    pub name: String,
    pub definition: Option<DefinitionId>,     // "Emulator specification"; None = fully custom
    pub install: EmulatorInstall,
    pub profiles: Vec<EmulatorProfile>,
}

pub enum EmulatorInstall {
    Folder { dir: PathBuf, executable: PathBuf }, // portable or installed folder
    Path { binary: String },                      // found on $PATH
    Flatpak { app_id: String },                   // launched with `flatpak run`
    AppImage { file: PathBuf },
    AppBundle { bundle: PathBuf },                // macOS .app, launched with its inner executable
}

pub enum EmulatorProfile {
    BuiltIn {
        id: ProfileId,
        definition_profile: String,
        args: ArgsOverride,                       // Default | Replace(Vec<String>) | Append(Vec<String>)
        hooks: ProfileHooks,
    },
    Custom {
        id: ProfileId,
        name: String,
        executable: Option<PathBuf>,              // None = the emulator's own executable
        args: Vec<String>,
        working_dir: Option<String>,
        platforms: Vec<PlatformId>,
        extensions: Vec<String>,                  // "<none>" for files without an extension
        env: Vec<(String, String)>,
        tracking: Tracking,
        hooks: ProfileHooks,
    },
}

pub struct ProfileHooks {                         // replaces Playnite's PowerShell pre/post/exit scripts
    pub before: Option<Vec<String>>,              // argv run by core before launch; non-zero exit cancels
    pub after_exit: Option<Vec<String>>,
}

pub enum Tracking { Default, ProcessTree, OriginalProcess, Folder(String), ProcessName(String) }
```

A built-in profile's availability is computed, not stored: it's **available** when the emulator is installed and every `requires` path exists, **missing files** otherwise. Missing profiles stay configured (a core may be reinstalled) but games using them show a warning instead of failing at launch.

```rust
pub struct PlatformSettings {
    pub platform: PlatformId,
    pub default: Option<(EmulatorId, ProfileId)>,
    pub bios_dir: Option<PathBuf>,                // overrides where BIOS files are looked for
}

pub struct RomScanner {
    pub id: ScannerId,
    pub name: String,
    pub folder: PathBuf,
    pub platform: Option<PlatformId>,             // None = take the profile's platform(s)
    pub profile: (EmulatorId, ProfileId),         // its extensions decide what files match
    pub launch_with: LaunchTarget,                // what imported games launch with
    pub recursive: bool,
    pub scan_archives: bool,
    pub skip_cloud_placeholders: bool,
    pub exclusions: Vec<Exclusion>,               // Playnite's four path modes, see §6.3
    pub in_library_update: bool,
}

pub enum LaunchTarget {
    PlatformDefault,                              // default for new scanners
    Profile(EmulatorId, ProfileId),               // pinned
    AskProfile(EmulatorId),                       // choose a profile of this emulator at launch
    AskEmulator,                                  // choose any compatible emulator at launch
}
```

`LaunchSpec::Emulator` from TECH-PLAN §3 becomes:

```rust
LaunchSpec::Emulator {
    target: LaunchTarget,
    roms: Vec<GameRom>,                           // several for multi-disc
    args: ArgsOverride,                           // per-game override, Playnite's "Additional arguments"
}
```

**Paths are relative by default.** A `GameRom` path is stored relative to its scanner's folder (`scanner:<id>/Final Fantasy VII (Disc 1).chd`). Moving a ROM collection or remounting a drive means editing one scanner folder, not every game. ROMs added outside a scanner store an absolute path.

## 5. Emulator detection

Detection runs as part of library discovery (Settings → Library Discovery, so "Emulators configured" there is real) and on demand from **Settings → Emulation → Detect emulators**.

1. **Standard locations, no user input.** For the current OS, every definition's `detect` rules are tried against: `$PATH`, `flatpak list --app`, AppImages in `~/Applications` and `~/.local/bin`, `/Applications` and `~/Applications` bundles (by bundle id), Program Files, `%AppData%`, Scoop and Chocolatey, and EmuDeck/RetroDECK layouts.
2. **A folder from the user** (Playnite's import wizard): walk it recursively, match `executable` regexes, group matches by definition + directory.
3. For each install found, evaluate every definition profile's `requires`. The ones that pass are offered as enabled built-in profiles.
4. Show a review list: emulator, install kind and path, version if known, available profiles. Already-configured installs are marked and unticked. **Import** creates the emulators.
5. After import, any platform without a default gets the first available profile for it, in definition order (definitions list profiles in recommended order).

Detection never runs emulators to probe them.

### 5.1 Launching by install kind

| Kind        | argv                                                                                        | Tracking default                                    |
| ----------- | ------------------------------------------------------------------------------------------- | --------------------------------------------------- |
| `Folder`    | `<executable> <args…>`, cwd = install dir                                                   | process tree                                        |
| `Path`      | resolved binary + args                                                                      | process tree                                        |
| `Flatpak`   | `flatpak run --filesystem=<rom dir>:ro <app_id> <args…>`, adding the ROM dir only when the app can't already see it | process name (the sandboxed child isn't our child) |
| `AppImage`  | the AppImage + args                                                                         | process tree                                        |
| `AppBundle` | `<bundle>/Contents/MacOS/<exe> <args…>` (not `open -a`, so we get the process)              | original process                                    |

## 6. ROM scanning

ROM scanners replace the M1 line item "ROM-folder source with RetroArch profiles". The `roms` library source in `caskade-sources` runs every scanner with `in_library_update` during a library update, and one scanner on demand.

### 6.1 Manual vs automatic

- **Automatic** (library update, "Scan all"): results are imported straight away, like Playnite's "Update emulated folders".
- **Manual** ("Scan" on one scanner, or a new scanner's first run): results open a review list where the user can untick files, fix title/platform/region in bulk, and split or merge multi-disc groups. The choices are remembered as exclusions and overrides so the next automatic scan agrees.

### 6.2 Pipeline

1. **Enumerate** files under the folder (recursive if set). Skip exclusions, hidden files, and cloud placeholders if set (Windows `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS`, macOS dataless files).
2. **Match extensions** case-insensitively against the profile's list. With `scan_archives`, look inside zip/7z for a supported file when the profile doesn't accept the archive itself.
3. **Absorb playlist parts.** `.m3u`, `.cue` and `.gdi` claim the files they reference, so `.bin` tracks never become games. An `.m3u` beats the `.cue` files it lists.
4. **Parse names** (No-Intro/Redump conventions): `(…)` and `[…]` groups are properties. Properties starting with `Disc`, `Disk` or `Side` mark parts. Region codes come from the region data. The title is the name with the groups removed, `_` replaced by spaces, trademark symbols dropped, and trailing `, The` moved to the front.
5. **Identify** _(post-prototype)_: CRC32 or serial lookup in the platform's DAT, which can replace the parsed title and region. Formats without DAT coverage (`chd`) skip this step.
6. **Group** files with the same title, platform and folder into one game with several `GameRom`s, ordered by disc number.
7. **Reconcile** with the library: already-imported ROM paths update in place, missing ones mark the game's entry as not installed (they don't delete it), new ones go through the `library.import` interceptor chain (TECH-PLAN §4.3) and are saved.

Every step is a pure function over a file list except enumeration and reconciliation, so they're snapshot-tested with `insta` on fixture trees.

### 6.3 Exclusions

Per scanner and global, with Playnite's syntax so it's familiar:

| Pattern       | Matches                                         |
| ------------- | ----------------------------------------------- |
| `bios/`       | that exact path under the scan folder           |
| `>*.txt`      | a name or relative path at any depth (glob)     |
| `?^beta/.*`   | regex anchored at the scan folder               |
| `>?\(Beta\)`  | regex at any depth                              |

## 7. Settings → Emulation (UI)

One settings tab with three sections, selected with `?section=` in the URL (`/settings?tab=emulators&section=scanners`). The UI lives in `apps/desktop/src/views/settings/emulators/` and talks to the core through `api/emulation.ts` (the view contract) and `api/emulationNative.ts`.

### 7.1 Header

Stats (emulators, platforms with a working default, ROMs found, BIOS issues) and actions: **Detect emulators**, **Add emulator**, **Scan all**.

The detect action opens the review list from §5 inline: folder field (optional) with Browse, then results with checkboxes and an Import button.

### 7.2 Emulators section

Master–detail. The list on the left shows each emulator with its install kind, platforms, and a status dot (ready / some profiles missing / not found).

The editor on the right has:

- **Name**, **Installation** (kind + path, Browse), **Emulator specification** (definition or "None, custom").
- **Profiles**: each built-in profile shows its platforms, extensions, availability, BIOS status, and the resolved command line for a sample ROM. **Arguments** is a segmented control: Default / Replace / Append, with a field when not Default. The resolved command line is always shown, with each argv token as a chip, so quoting mistakes are visible.
- **Add built-in profile** lists definition profiles not yet added (available ones first). **Add custom profile** adds the full form: executable, arguments, working directory, platforms, extensions, tracking mode (+ path/name), environment, before/after commands.
- A variables reference next to the argument fields. Clicking a variable inserts it.

### 7.3 Platforms section

One row per platform that has a configured profile or ROMs: name, ROM count, **default emulator/profile** select (only compatible profiles), BIOS status (`2/3 found`), extensions. This is the "which platforms actually work" overview Playnite lacks.

### 7.4 ROM scanners section

Table of scanners: name, folder, platform, profile, "launch with", games found, last scan, "include in library update" switch, Scan button. Selecting one opens its editor with every `RomScanner` field and its exclusions list.

### 7.5 Emulation Hub view (later)

The `emulation.png` concept is a browsing view (platform shelves, configured emulators summary, recently played, tools). It reuses this data but is a separate route, built after the settings tab works against the real backend.

## 8. Plugins

Extension points (names final in M4):

- **`contributes.emulation`** in `plugin.json`: paths to platform/region/definition TOML files. Data only, no code, so no permissions are needed. This is how the community adds emulators.
- **`rom-importer` provider** (WIT): for emulators whose games aren't plain files (RPCS3's installed titles, ScummVM, Vita3K). Gets the emulator's install, returns `GameRom`-like entries with serials/titles. Needs `fs` permission for the emulator's folders.
- **`launch.before` / `launch.after` interceptors** already receive the `LaunchSpec`. For emulator launches the payload adds the resolved emulator, profile and argv, so a plugin can rewrite them (e.g. pick a RetroArch config per game).
- **`emulation.scan.completed`** event with counts per scanner.

Profile `before` / `after_exit` commands (§4) are core features for simple cases. Anything smarter is a plugin.

## 9. Installer hand-off

`caskade-installer` returns `HandOff::Rom { platform, files }` for ROMs and disc images. Emulation import then:

1. Finds the scanner for that platform. If there is one, offers to move/copy the files into its folder (default: move when on the same filesystem), then runs that scanner.
2. Otherwise imports the files in place as a game with absolute `GameRom` paths and `LaunchTarget::PlatformDefault`, and suggests creating a scanner.
3. If the platform has no emulator, the game is still added and shows "No emulator for SNES — configure" instead of a Play button.

## 10. Milestones

Lettered E0–E4 and slotted into TECH-PLAN's milestones.

| #      | Lands in | Contents                                                                                                                                                                                                                      | Exit criteria                                                                                     |
| ------ | -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| **E0** | now      | Settings → Emulators prototype on mock data (§7), research notes, this plan.                                                                                                                                                 | Every §7 interaction can be clicked through in the browser.                                       |
| **E1** | M1       | Definition format + loader + validation, variables, shipped data for ~20 platforms and RetroArch, PCSX2, DuckStation, Dolphin, PPSSPP, melonDS, mGBA, Cemu/Ryujinx-class, RPCS3 (no importer yet). SQLite schema for §4. | Definitions load on all 3 OSes; insta snapshots of resolved argv per OS.                          |
| **E2** | M1       | Detection (§5) for all install kinds, emulator/profile CRUD commands, settings tab wired to the backend.                                                                                                                       | On a Linux box with Flatpak RetroArch + AppImage PCSX2, detection configures both with no typing. |
| **E3** | M1       | ROM scanners (§6) without DAT identification, review list, exclusions, relative paths, platform defaults, installer hand-off.                                                                                               | 1,000-file fixture tree imports in < 2 s with correct multi-disc grouping.                        |
| **E4** | M2       | Emulator launches through the pipeline: install kinds (§5.1), tracking modes, ask-at-launch, disc picker, per-game overrides, before/after commands, BIOS status.                                                        | Launch a PS1 multi-disc game via Flatpak RetroArch with playtime recorded.                        |
| E5     | M4       | `contributes.emulation`, `rom-importer` provider + RPCS3 importer, emulator payload in launch interceptors.                                                                                                                  | A plugin adds an emulator definition that detection then finds.                                   |

Post-prototype: DAT/CRC identification, Playnite and ES-DE config import, Emulation Hub view, BIOS manager with hash verification UI, RetroAchievements hashing, per-game emulator config files.

### Testing

- **Definitions:** every shipped file parses and validates in CI. Regexes compile. Every profile platform exists. Every `requires` and `args` variable resolves on every OS.
- **Name parsing and grouping:** table tests and `insta` snapshots over real-world No-Intro/Redump names (regions, revisions, `(Disc 1 of 2)`, `[!]`, `, The`).
- **Detection and scanning:** fixture trees generated in temp dirs (empty files with the right names). Flatpak detection is tested by feeding recorded `flatpak list` output.
- **Launch:** argv resolution is pure and snapshot-tested. Real launches are covered by a fake "emulator" binary that records its argv and exits.

## 11. Risks and open questions

| Risk / question                                                                                                  | Plan                                                                                                                                        |
| ---------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| Definition upkeep: 95 emulators × 3 OSes is a lot of data to keep right.                                         | Ship a small, tested set. Make contributions data-only so the community can PR or plugin them. CI checks validity, not correctness.         |
| Flatpak sandboxes can't see ROMs outside the user's home or granted dirs.                                        | Add `--filesystem=<rom dir>:ro` per launch when needed (§5.1). Show a hint when a scanner folder is outside what the app can reach.         |
| Process tracking for emulators that relaunch themselves (PCSX2 updater, Yuzu-forks) or run under `flatpak run`. | Per-definition default tracking mode; `ProcessName` fallback.                                                                               |
| Should platform defaults be per-device? A Steam Deck and a desktop may want different cores.                     | Open. Leaning: platform defaults are device-local settings, scanners and games are synced (relevant once sync exists, TECH-PLAN §10).      |
| Arg override "Replace" loses future definition fixes.                                                            | The editor shows the definition's current args beside the override and flags when they changed since the override was made.               |
