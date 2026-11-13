# Product spec

What Caskade does today, and what is planned. Last reviewed 2026-11-08, at version 0.1.3.

Why it exists is in the [mission](MISSION.md). How it is built is in the [tech spec](TECH-SPEC.md). How it looks and reads is in the [design guidelines](DESIGN-GUIDELINES.md).

## 1. Status

- **Released**: 0.1.x builds are published as GitHub releases (deb, AppImage, NSIS, MSI and a macOS app bundle), and installed copies update themselves from them.
- **Linux** (X11, including Cinnamon, GNOME and gamescope): the reference platform. Every feature runs and is tested here, with live checks under Xvfb.
- **Windows**: the app builds and passes lint. CI runs the launch channel and the administrator steps natively, and streaming, sync and launches have been tried in Windows VMs. Some real-hardware checks are still open (§5).
- **macOS**: the code paths exist, but the app has not been compiled or run on a Mac yet. The standalone installer and discovery libraries are tested on macOS in CI.
- **Languages**: English and Swedish. A translation ships as soon as its PO files are added.

## 2. Who it's for

- **The collector**: games across Steam, GOG, Epic, Xbox, itch, emulators and ROM folders, who wants one library with real artwork and one Play button.
- **The retro player**: wants emulators set up, BIOS files found, ROMs scanned and named correctly, and saves kept, without learning each emulator.
- **The household**: several people sharing PCs and several PCs in a home. Children with age limits, parents with a PIN, the game on the office PC played on the living-room TV, saves that follow the player.
- **The couch player**: a TV, a handheld or a Steam Deck, driven entirely with a pad.
- **The tinkerer**: themes, custom CSS, plugins, launch scripts and per-game actions.

## 3. Surfaces

Caskade has two shells over the same views:

- **Desktop mode**: a window with a sidebar, a topbar that is also the title bar, and a status bar. Mouse, keyboard and drag and drop.
- **Console mode**: fullscreen, larger type and cards, a hint bar showing what each pad button does, and an on-screen keyboard. It starts by itself under gamescope, on a handheld or TV (detected), with `--console`, or from the setting. The guide button brings Caskade back over a running game.

Views:

| View | What it holds |
| --- | --- |
| **Home** | A hero for the game to continue, then Recently Played, Recently Added and Recently Released shelves. A game last played on another device says which ("Last played on Den", or "another device" when this one isn't paired with it). |
| **Library** | Every game, filtered by Recently Played, Installed, Ready to Install (owned but not installed), Emulated, Web and All Games, with counts. Grid or list layout, sorts (recently played, title, popular, playtime, recently added, release date), a platform view, search, selection mode and Save as collection. The Web filter shows web games as a mosaic. |
| **Game page** | Hero art, Play (or Stream from a device), playtime and its sources, details, screenshots and videos, achievements, saves, the game's releases (one per platform or store), owned releases still to install, other devices that have it, fan ports, the IGDB match, and plugin tabs. |
| **Store** | A catalog of games from IGDB: search with filters, new and upcoming releases, popular sorts, and pages for games you don't own, linking to stores. It creates no library entries. |
| **Collections** | Manual lists and smart collections (a saved filter, platform, search and sort), pinned or not, each with a page that can play a random game. |
| **Activity** | Jobs (downloads, installs, updates, copies, questions waiting for an answer) and the notification feed, opened from the topbar's bell. |
| **Settings** | General, Appearance, Plugins · Library Discovery, Launchers, Store accounts, Storage, Installer · Emulation, Scripts · Devices, Streaming · Profile, Accounts · Backup, Statistics, About. Searchable. |

Choosing a game opens its quick look: a side panel in desktop mode, the page itself in console mode. Double-clicking a game in the library plays it, as its menu's Play does; its card and quick look say Starting… at once, until the game runs. Every game card, hero, search result and release row opens the same game menu: Play or Stop, Play with…, tools, open the page, favourite, Add to collection…, Edit…, Share with someone…, open or copy its folder, move, uninstall, hide, and plugins' actions.

## 4. Features in place

### 4.1 Finding games

- **Launchers and stores on the device**: Steam, Heroic, Legendary, Lutris, Epic, GOG, EA, Ubisoft, Battle.net, Xbox, itch, Amazon, Riot, Rockstar, Bottles, Faugus, Minigalaxy, Flatpak and Snap.
- **The OS**: Start Menu shortcuts, uninstall entries, Windows Game Explorer, `.desktop` files, macOS app bundles and Wine prefixes, with a classifier that keeps out things that aren't games.
- **Merging**: one game found by several sources becomes one game with several releases. Games that are the same game (the same IGDB id, or the same title released within five years) merge into one game with a release and a Play button per platform. Separate keeps a release apart for good.
- **Watching**: folders and launchers are rescanned when they change. Each launcher can be switched off in Settings → Launchers.
- **Adding by hand**: Add a game on this computer… takes a program, a shortcut (with its arguments, folder and environment) or a URI. Add by title… adds a record with no files. Likely duplicates ask whether to join or keep apart.
- **New games** found in the background arrive as one batched notification an hour.

### 4.2 Game information

- **IGDB metadata** through Caskade's metadata server: each game is matched by its RomM ROM, a Hasheous match by a hash supplied by RomM or by title, or its store ids (Steam, GOG, Epic and others); a game from another device that nothing here can match takes that device's match. It's then given a description, genres, tags, release date, alternative names, age ratings, cover, hero, icon, screenshots and videos. Answers are cached, so a matched library stays complete offline.
- **Hasheous** data dumps identify ROMs and games by hash and title, offline.
- **Games from your other devices** are matched and filled in on each device, with its own downloads, as soon as they arrive, while online lookups are on there. A match you chose by hand travels with the game.
- **The IGDB panel** on a game page shows the match and changes it: search by title, type an id, mark "not on IGDB", rematch or refresh. While online lookups are off on this device it says so, and turns them on.
- **Artwork**: covers, backgrounds and icons from IGDB, RomM and stores, each replaceable by the user's own image (a file or a drop), hidden or reset. Screenshots and the hero download only when the page first opens.
- **Videos** play in a dedicated video window over Caskade.
- **Age ratings** from every rating board give each game a minimum age, which account age limits use.
- **Popularity**: daily IGDB scores sort the Library, smart collections and the Store by Popular, from the last scores when offline.

### 4.3 Organising

- Favourites, hidden games, search (the topbar search, or simply typing anywhere), filters, sorts and platform views.
- **Collections**, manual and smart, belonging to the local account that made them.
- **Editing**: the game editor changes title, sort title, description, dates, genres, tags, platforms, artwork, actions, scripts and save locations. A field you set stays set: background refreshes never undo it, including an intentional clear, and Reset to automatic gives it back.
- **Selection mode** picks games by click, Shift+click or pad, or all of them, then bulk-edits them with a preview, or refreshes their details from IGDB as a job you can follow, cancel and retry.

### 4.4 Playing

- **Play** starts the game through its launcher URI, executable, emulator, web window or stream, then finds the real game process (even when a launcher starts it as a child) to count playtime and to offer Stop.
- **Playtime** combines Caskade's own sessions with the totals stores report, without counting the same time twice. The page shows where the time came from.
- **Before play**, Caskade can offer a game update, restore a newer save from another device (or ask which save to use when both devices played), warn that the game is running on another device, and refuse a game above the account's age limit.
- **Launch actions**: a release can have named play actions (a mod loader, a different executable, other arguments) and utility actions (a config tool, a mod manager), with a default or a question at each launch, and a choice of how the game's process is tracked. On Linux, an action that runs a Windows program from a Caskade install picks its Proton build: Automatic (the best one installed when it starts) or one of the builds on this device.
- **Lifecycle scripts**: commands or shell scripts before launch, after the game starts and after it exits, globally, per emulator profile and per game, with a time limit, output capture, Test, and cleanup that always runs.
- **Focus**: in desktop mode Caskade minimises, hides or stays when a game starts (Settings → General). In console mode a curtain covers Caskade while the game runs and lifts when it exits or Caskade comes back.
- **`caskade --launch <game>`**: other programs (shortcuts, Sunshine, other launchers) start a library game through the same pipeline and live as long as the game.

### 4.5 Installing and storage

- **Install from** a file, a folder, a link (web page, Google Drive, Dropbox, GitHub releases and other hosts), a magnet link or a `.torrent` file. Windows installers, Linux and macOS packages, disc images, archives and portable games are all understood. On Linux, Windows installers and games run under Proton through umu-launcher, with the best build installed when they start: UMU-Proton (which umu keeps current), else the newest GE-Proton, else Steam's newest numbered Proton, then Experimental; Hotfix builds are never chosen on their own. With none installed, umu downloads UMU-Proton.
- **Nothing installs unasked**: links (`caskade://install?…`), drops on the library, pasted text and torrent links all land in a confirmation first. Caskade can become the system's handler for magnet links and torrent files (Linux and Windows).
- **Storage locations**: Caskade-managed install folders, with a default. A drive that comes back at another path is recognised. Games move between locations as a job, and a game moved outside Caskade is found again.
- **Game updates**: games installed from GitHub releases are checked every six hours (and before play when that check is overdue). An update keeps the user's own files and changes, backs up the ones upstream changed too, and rolls back on failure.
- **Uninstall** removes a game's files; removing a game Caskade didn't install never touches its files.

### 4.6 Emulation

- **21 emulators** are defined: RetroArch (with cores), ares, Azahar, Cemu, Dolphin, DuckStation, Flycast, gopher64, MAME, melonDS, Mesen, mGBA, PCSX2, PPSSPP, RMG, RPCS3, Ryujinx, shadPS4, xemu, Xenia and Ymir, across the platforms in `platforms.toml`.
- **One-click setup**: Settings → Emulation lists the platforms in your library with no working emulator and installs the recommended one (a download before a Flatpak), skips its first-run wizard, points it at the BIOS folder, adds its profiles and makes it the platform's default. Installed emulators update like games.
- **ROM scanners** turn ROM folders into library games named from their filenames, with metadata matched by title, per-platform defaults and per-game overrides. RomM games can also match by their server-supplied ROM hashes.
- **Profiles** set each emulator's arguments, and launches add fullscreen flags in console mode.
- **BIOS** files missing for a platform come from your RomM server's firmware.
- **RomM servers** are a library source: their ROMs, artwork and metadata sync into the library, and ROMs download on demand. Servers can be the same on your other devices, but each device signs in on its own: a server without a sign-in here, or whose sign-in the server refused, says so and offers Sign in, and waits for it instead of retrying in the background.
- **Emulator saves** are found from each definition's save rules.

### 4.7 Web games

- The Library's **Web** tab holds games that run in a web page, shown as a mosaic. **Add web game** takes a page address and nothing else.
- Web games play in Caskade's own game window, so playtime counts, console mode is fullscreen and the guide button works.
- **Play offline** (Linux): a guided capture saves the game and its files into a storage location, without the site around it, its ads or its trackers, and checks that it plays with no network.

### 4.8 Fan ports

Game pages show fan decompilations, recompilations and source ports of that game (from Decomp Buddy's plans), matched by platform and IGDB id or title. Installing one adds it to the same game as another release, using your ROM where the port needs one.

### 4.9 Store accounts and the Store

- **Steam, GOG and Epic accounts** import the complete owned library, including uninstalled games, which appear in Ready to Install. Install opens the store's launcher, and Caskade picks up the copy when it lands.
- Play time the stores report is imported (never, for new games only, or always).
- Sign-ins live in the OS keychain. Disconnect signs out and keeps the library; removing an account previews what goes and keeps anything installed or owned another way.
- **The Store** browses IGDB's catalog and links to stores. It sells nothing.

### 4.10 Achievements

Read-only achievement progress for Steam, GOG, Epic and RetroAchievements (emulated games, linked by a verified ROM hash, hardcore and softcore apart). The game page lists them with rarity, hides spoilers, and announces new unlocks after a session. Each store account's achievements stay its own.

### 4.11 Devices and sync

A household's devices pair once and then keep in step directly, with no server and no internet needed on a local network.

- **Pairing** by a short code or a single-use link, accepted by an admin. Only admins pair.
- **Library sync**: games, edits, collections, favourites, play sessions (with the device each was played on), accounts and settings, merged field by field. Paths, folders, emulators, store sign-ins and caches never leave a device. Each settings section can be kept to this device. New handhelds and TVs start in console mode.
- **Saves** follow the player: after each session a game's saves are kept as versions (from the Ludusavi manifest or emulator rules, mapped into a Wine or Proton prefix where needed), sent to the other devices, restored before play, and browsable on the game page.
- **Playing elsewhere**: a game running on another device shows as such, and Play warns before starting it twice.
- **Copies**: a game installed on one device can be copied to another over the local network, as an independent install. A web game's offline copy arrives as that game's offline copy; one that plays from its page has nothing to copy. Downloads a nearby device already has come from it instead of the internet.
- **Other devices' games** show on the game page, with copy and stream offers.
- **Backups kept on a paired device**, restorable on a fresh install.
- **Set up from another device**: first-run setup can pair with an existing device, take its library, and offer its games to copy.
- **Emulators across devices**: a preferred emulator missing here is offered for install, and emulator configuration can follow.
- Relays are used only when allowed and nothing direct works, and never for game files. Never use relays turns them off.

### 4.12 Sharing a game once

Share with someone… gives a game to a person outside the household through a link (and a QR code in console mode) that expires and can be stopped at any time. The recipient installs it from the sharer and the share's other recipients at once.

### 4.13 Streaming

Play a game installed on another device in the house, streamed over the local network.

- **Hosting** is one switch per device (and one administrator prompt). Caskade installs, configures, updates and repairs the streaming host itself, publishes the device's games to it with their covers, and runs it only while Caskade runs.
- **Clients** install the streaming client on the first stream, without asking.
- **Pairing** with a code entered in Caskade on the host, or automatically between devices already paired through sync.
- **Stream from <device>** on the game page and in the game menu. A sleeping host is woken (Wake-on-LAN), a host busy with another game asks before stopping it, and End stream is in the guide menu.
- Stream quality is a per-device setting. Playtime counts on the host.
- The user never sees the underlying programs (Sunshine and Moonlight), their windows or their settings.

### 4.14 People

- **Local accounts**: admin or standard, permissions, an age limit, an optional 4–8 digit PIN (a household lock, not data protection), and a switcher. A library from before accounts starts with one admin and no PIN.
- Collections and some preferences belong to each account. Settings that change the device are admins'.

### 4.15 Activity and notifications

- Jobs show progress, wait for answers, recover after a failure (try again, download again, locate the installer, choose another location) and survive restarts. A download can pause and resume where it stopped, even after a restart (#131); a store's own launcher pauses its downloads itself, and the game page says so. A failed or cancelled job can be removed from Activity; what it downloaded or installed stays.
- The notification feed keeps finished work, failures, updates found, new games and achievement unlocks, with read state that survives restarts.

### 4.16 Backups

Settings → Backup (admins): back up now, daily or weekly automatic backups with retention, and inspect or restore any backup, applied at the next start. Each backup is verified before it counts. A library that won't open offers Restore from backup…, and `caskade --restore` restores from the command line. An older Caskade opens a library a newer one changed when the changes don't affect it; when they do, it offers Update Caskade, which installs the newest release and restarts.

### 4.17 Making it yours

- **Themes**: four built in: Nexus, the default (dark, sci-fi accents), and three in the spirit of today's console home screens with names of their own: Cobalt (deep cobalt and white), Emerald (near-black and emerald) and Engage (light, white with red and teal); Paper, Ember and more from the add-on catalogue, and any theme folder you add. An accent colour, density, reduced effects and motion, a UI scale, and custom CSS on top.
- **Plugins**: WebAssembly backends with the permissions you grant, and optional UI parts (views, game page tabs, menu actions, settings). Settings → Plugins installs, updates and removes plugins and themes from a signed catalogue, rolling back a failed update and asking again for any new permission.
- **Languages**: the UI, the core's messages and plugins are translatable.

### 4.18 The app itself

- **Caskade updates** are checked every six hours, downloaded and installed only when you press the button, and run after Restart now.
- **Tray icon** with Show and Quit. Closing the window hides it to the tray by default. A second start shows the running window.
- **Startup** (Settings → General): Launch on system startup starts Caskade when you log in, Start minimized keeps that start in the tray with no window, and Open to picks the first view: Home (the default), Library, Collections or the view open when Caskade last quit. Console mode keeps its own start.
- **Settings → Statistics**: CPU, GPU, memory, network and storage use, sampled only while you watch.
- **Self-healing**: lost webviews reload and the app restarts itself with a cap, interrupted installs and updates finish or roll back at start, and failures say what happened and what to do.
- **First-run setup**: language, where games and ROMs live, the first account, or setting up from another device.
- **About**: version, terms, privacy policy and third-party licences.

## 5. Planned

### 5.1 Open issues on GitHub

| Issue | What it needs |
| --- | --- |
| [#71](https://github.com/krazyjakee/caskade/issues/71) | Idle memory with a large library (8,000+ games). The allocator caps, trimming and several fixes have landed. Still to do: a memory budget for an idle app, checked so it can't creep back up. |
| [#8](https://github.com/krazyjakee/caskade/issues/8) Playnite milestone | One acceptance check left: from a clean profile, connect Steam, GOG and Epic, install an owned game through its launcher, and launch it with accurate playtime, on real accounts. |
| [#20](https://github.com/krazyjakee/caskade/issues/20), [#19](https://github.com/krazyjakee/caskade/issues/19) Devices, sync and sharing | Sync is built and tested in CI with several cores and a local relay. Still open: the manual checklist on real machines on all three platforms. |
| [#33](https://github.com/krazyjakee/caskade/issues/33), [#27](https://github.com/krazyjakee/caskade/issues/27), [#28](https://github.com/krazyjakee/caskade/issues/28), [#29](https://github.com/krazyjakee/caskade/issues/29) Game streaming | Streaming is built. Still open: real-machine checks of the first-stream install, pairing and streaming, across operating systems. |

The procedures for these checks, grouped by the machines they need, are in [manual checks](manual-checks.md).

### 5.2 Next, not yet filed as issues

Carried over from the retired design plans. Each needs an issue before work starts.

- **macOS**: compile, run and verify the app on a Mac. Streaming clients are in scope there. Hosting waits for Sunshine's own macOS support.
- **Web games**: simple routes for direct files, itch.io and Newgrounds, Ruffle for Flash, stand-ins for ad SDKs, offline bundles on Windows and macOS, RomM's browser games, a pad-to-key mapping and Gamepad API support fed by Caskade's own pad input, and per-game tile sizes in the mosaic.
- **Emulation from plugins**: plugin-supplied emulator definitions and ROM importers (an RPCS3 importer first), with the emulator's details passed to launch interceptors.
- **Plugin metadata providers**: the hook is reserved in the plugin contract but doesn't run.
- **JavaScript plugin backends**: backends are Rust-only for API 0.1. An embedded JS runner behind the same contract is an open question.
- **Untrusted UI plugins**: whether signing and review are enough, or plugin UIs need an isolated tier.
- **Windows hosts' gamepads** depend on ViGEmBus, which is archived. If Windows stops loading it, a signed replacement is needed.

### 5.3 Decided against

- Backlog and completion tracking, advanced library filters, and custom metadata sources or provider selection.
- Store account adapters beyond Steam, GOG and Epic for now, wishlists, social features, and importing a Playnite database or Playnite plugins.
- A Caskade cloud or account. Sync is device to device.
- Streaming through relays or over the internet.
- Linking or patching GPL programs (Sunshine, Moonlight, emulators). They stay separate programs.
- Telemetry and advertising.
