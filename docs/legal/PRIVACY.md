# Privacy policy

Effective: 2026-11-25

## In short

Caskade is maintained by **Jake Cattrall** and runs locally on your computer. It has no Caskade online accounts, no advertising and no telemetry: it does not report how you use it, what you play or what crashed. Your library stays on your device unless you pair your own devices to sync or share a game directly with someone. Caskade connects to the services below to fetch game information, updates, accounts you connect and downloads you request, and those services see your IP address and the requests Caskade makes.

## What Caskade keeps on your device

- **Your library**: the games Caskade found or installed, their details and artwork, your collections, favourites, hidden games, launch options, play sessions (when and how long you played), store entitlements, cached achievements, paired device records, save versions and shares, in a SQLite database (`library.sqlite`) in Caskade's data folder, alongside managed emulators and tools, versioned save blobs, themes, plugins and backups.
- **Settings**: your preferences, in Caskade's configuration and data folders.
- **Local accounts**, if you add them: each account's name, avatar, role, permissions, age limit, and a salted argon2id hash of its PIN (never the PIN itself), in the same database. When device sync is off, local accounts stay on your device; when you pair devices and enable device sync, local accounts replicate across your paired devices.
- **Caches**: downloaded artwork and thumbnails, IGDB answers (`igdb/games.sqlite`), Hasheous data dumps and indexes, the Ludusavi save-location manifest, Decomp Buddy port plans, the add-on catalogue, compiled plugins and peer download caches, in Caskade's cache folder. They can be deleted at any time; Caskade fetches or rebuilds them when needed.
- **Passwords, tokens and keys**: stored only in your system's keychain (Secret Service on Linux, Keychain on macOS, Credential Manager on Windows), never in the database, settings, logs, events, backups or UI. This covers RomM server passwords and API tokens (removed when you remove a server), store and achievement sign-in tokens and API keys for Steam, GOG, Epic Games and RetroAchievements (removed when you disconnect or remove an account), this device's ed25519 identity key for device sync, and web credentials for Caskade's managed Sunshine host or a Sunshine host of your own.
- **Logs**: Caskade writes its log to standard error and to daily rotating files (`caskade.YYYY-MM-DD.log`, keeping the last 7 days) in Caskade's logs folder (`logs` inside your system's local application data folder). Logs stay on your device and are never sent automatically; recorded error details redact passwords, tokens and URL credentials.

The folders are your system's standard application folders for `dev.Caskade.Caskade` (for example `~/.local/share/caskade`, `~/.config/caskade` and `~/.cache/caskade` on Linux, `%APPDATA%\Caskade\Caskade` and `%LOCALAPPDATA%\Caskade\Caskade` on Windows, and `~/Library/Application Support/dev.Caskade.Caskade` and `~/Library/Caches/dev.Caskade.Caskade` on macOS). Uninstalling Caskade, deleting those folders and clearing its keychain entries removes everything it stored.

## What Caskade reads on your device

To find your games and version saves, Caskade reads the files and settings of game launchers and stores installed on your computer (Steam, Heroic, Lutris, GOG, Epic, Xbox and others), your Start Menu, `.desktop` files and app folders, the folders you choose for games and ROMs, and save-file locations for games you play. It reads these only on your device.

## Who Caskade connects to

**Caskade's metadata server** (`caskade-server`, operated by Jake Cattrall and hosted on Railway) answers questions about games using IGDB. Caskade sends it:

- IGDB ids of games in your library, to fetch their details;
- store ids of games in your library (for example a Steam app id or GOG product id), to find their IGDB entry;
- the text you search for when browsing the catalogue, matching a web game by title, or changing a game's IGDB match.

It does not send your file paths, playtime or account names. Background lookups run while **Settings → General → Fetch artwork and game info online** is on (the default); browsing and manual searches happen only when you open them.

The metadata server keeps a cache of IGDB's answers (game details, keyed by IGDB id) and, in memory for at most a day, recent catalogue pages; none of it is tied to who asked. It does not log individual requests, only its own errors. Railway, which hosts it, keeps its standard HTTP request logs (such as the address a request came from, the path and the time) under [Railway's own retention](https://railway.com/legal/privacy).

**Image hosts**, chiefly IGDB's (`images.igdb.com`), YouTube thumbnails, RetroAchievements (`media.retroachievements.org`), store artwork hosts and your RomM server: Caskade downloads covers, icons, heroes, screenshots and achievement icons from the addresses those services provide.

**Hasheous** (`hasheous.org`): Caskade downloads Hasheous data dumps for the platforms your games and emulators need (the dump index first), and looks games up in them on your device. Beyond which platforms' dumps it asks for, it sends Hasheous nothing about your games.

**Store accounts and achievements**, only when you connect an account in **Settings → Store accounts**:

- **Steam** (`api.steampowered.com`, `steamcommunity.com`): connects with your API key or sign-in to fetch owned games, playtime and achievements.
- **GOG** (`auth.gog.com`, `embed.gog.com`, `content-system.gog.com`, `gameplay.gog.com`): connects with your GOG account to fetch owned games, playtime and achievements.
- **Epic Games** (`epicgames.com` and Epic's account, library, catalogue and GraphQL endpoints): connects with your Epic account to fetch owned games and achievements.
- **RetroAchievements** (`retroachievements.org`, `media.retroachievements.org`): connects with your username and Web API key to fetch achievements and icons using ROM hashes computed locally on your device.

**Device sync and one-time game shares** (iroh), only when you pair devices or create or accept a share link:

- **Device sync**: discovers nearby Caskade devices on your local network via mDNS (service `caskade`), and uses iroh's n0 address lookup and encrypted relay servers to reach your paired devices across networks unless **Never use relays** is turned on in **Settings → Devices**. Syncs library metadata, collections, play sessions, local accounts, RomM server addresses (never passwords or tokens), settings you choose to sync, and game saves between your paired devices, authenticated by ed25519 device keys.
- **Game copies and one-time shares**: copying an installed game between paired devices or transferring a game through a one-time `caskade://receive` share link connects directly between devices (and other recipients in a share swarm) without passing game files through a relay server.

**Game streaming (Sunshine and Moonlight)**, only when you enable streaming in **Settings → Streaming**: discovers streaming hosts on your local network over mDNS (`_nvstream._tcp`), wakes hosts with Wake-on-LAN, pairs directly over the GameStream protocol and streams directly between your devices.

**Decomp Buddy** (`tanookistudios.com/decomp-buddy/plans.json`, or a port catalogue URL you configure): while **Show ports of games** is enabled, Caskade checks `plans.json` daily (using an ETag) to list native fan ports for games in your library, matching them locally. Nothing about your library is sent to Decomp Buddy; a port's release is downloaded from GitHub only when you ask to install it.

**Add-on catalogue** (`github.com/krazyjakee/caskade/releases/download/catalogue/catalogue.json`, or a catalogue URL you configure): fetches the signed catalogue of plugins and themes, and downloads an add-on package only when you ask to install or update it.

**GitHub** (`github.com` and its download hosts):

- Caskade's own updates: Caskade checks the latest release's `latest.json` when it starts and every six hours, while **Check automatically** is on (**Settings → General → Caskade updates**), and downloads an update only when you ask.
- Updates to games, fan ports, managed tools and emulators installed from GitHub releases: Caskade checks their release pages every six hours by default (**Settings → Installer**), and before you play a game whose last check is more than an hour old.
- The Ludusavi save-location manifest (`github.com/mtkennerly/ludusavi-manifest`), cached locally to locate game saves.
- Emulators, umu-launcher (on Linux, to run Windows games), Sunshine, Moonlight, ViGEmBus (on Windows, for streaming) and games you install from GitHub.

**Emulator sources**, when you install or update an emulator: libretro's buildbot (`buildbot.libretro.com`) for RetroArch and its cores, Dolphin's update service (`dolphin-emu.org`), Flathub through Flatpak, and the download sites named in Caskade's emulator definitions.

**RomM servers you add**: Caskade signs in with the credentials or API token you give and syncs your RomM library, downloads ROMs, firmware and artwork from it.

**Web games and videos**: web games you open run in a standalone webview window (`caskade-player`) and connect directly to the game's site (and `cdn.jsdelivr.net` for Ruffle when playing Flash games). Trailers open YouTube embeds in a standalone video window.

**Addresses you give it and plugins**: when you install from a link (a web page, Google Drive, Dropbox, GitHub or another host), magnet link or `.torrent` file, Caskade downloads from that address or peer swarm (torrent uploading is disabled). Backend plugins you enable may connect only to the hosts their manifest declares and you grant.

**Your browser**: links to IGDB, stores and project pages open in your browser, under that site's own policies.

Each of these services handles your IP address and requests under its own privacy policy. HTTP requests from Caskade identify themselves as `Caskade/<version>`.

## Contact

Caskade and its metadata server (`caskade-server`) are maintained by **Jake Cattrall**. For privacy questions or requests, email `krazyjakee@gmail.com`.

## Changes

Changes to this policy are recorded in Caskade's repository (`docs/legal/PRIVACY.md`) with a new effective date, and ship with the next version of Caskade.
