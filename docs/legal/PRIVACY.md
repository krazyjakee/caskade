# Privacy policy

Effective: 2026-10-28

> **Draft.** This describes what Caskade does with your data as of this date. It is not yet complete as a policy: **missing** are the name of the person or organisation responsible for Caskade and its metadata server, and a contact address for privacy requests. Until they are added, ask questions through the project's [issue tracker](https://github.com/krazyjakee/caskade/issues).

## In short

Caskade runs on your computer. It has no online accounts, no advertising and no telemetry: it does not report how you use it, what you play or what crashed. Your library stays on your device. Caskade connects to the services below to fetch game information, updates and downloads, and those services see your IP address and the requests Caskade makes.

## What Caskade keeps on your device

- **Your library**: the games Caskade found or installed, their details and artwork, your collections, favourites, hidden games, launch options and play sessions (when and how long you played), in a SQLite database (`library.sqlite`) in Caskade's data folder.
- **Settings**: your preferences, in Caskade's configuration and data folders.
- **Local accounts**, if you add them: each account's name, avatar, role and age limit, and a salted hash of its PIN (never the PIN itself), in the same database. They never leave the device.
- **Caches**: downloaded artwork, the IGDB answers Caskade received and the Hasheous data dumps, in Caskade's cache folder. They can be deleted at any time; Caskade fetches them again when needed.
- **Passwords and tokens** for RomM servers: in your system's keychain (Secret Service on Linux, Keychain on macOS, Credential Manager on Windows), never in the database or settings. Removing a server removes them.
- **Logs**: Caskade writes its log to standard error, where only a terminal that started it sees it. It does not save or send them.

The folders are your system's standard application folders for `dev.Caskade.Caskade` (for example `~/.local/share/caskade` on Linux, `%APPDATA%\Caskade\Caskade` on Windows and `~/Library/Application Support/dev.Caskade.Caskade` on macOS). Uninstalling Caskade and deleting those folders removes everything it stored.

## What Caskade reads on your device

To find your games, Caskade reads the files and settings of game launchers and stores installed on your computer (Steam, Heroic, Lutris, GOG, Epic, Xbox and others), your Start Menu, `.desktop` files and app folders, and the folders you choose for games and ROMs. It reads these only on your device.

## Who Caskade connects to

**Caskade's metadata server** (`caskade-server`, hosted on Railway) answers questions about games using IGDB. Caskade sends it:

- IGDB ids of games in your library, to fetch their details;
- store ids of games in your library (for example a Steam app id or GOG product id), to find their IGDB entry;
- the text you search for when you change a game's IGDB match.

It does not send your file paths, playtime or account names. These lookups run in the background while **Settings → General → Fetch artwork and game info online** is on (the default); searching happens only when you search.

The metadata server keeps a cache of IGDB's answers (game details, keyed by IGDB id) and, in memory for at most a day, recent catalog pages; none of it is tied to who asked. It does not log individual requests, only its own errors. Railway, which hosts it, keeps its standard HTTP request logs (such as the address a request came from, the path and the time) under [Railway's own retention](https://railway.com/legal/privacy).

**Image hosts**, chiefly IGDB's (`images.igdb.com`): Caskade downloads covers, icons, heroes and screenshots from the addresses the metadata server or your RomM server gives.

**Hasheous** (`hasheous.org`): Caskade downloads Hasheous data dumps for the platforms your games and emulators need (the dump index first), and looks games up in them on your device. Beyond which platforms' dumps it asks for, it sends Hasheous nothing about your games.

**GitHub** (`github.com` and its download hosts):

- Caskade's own updates: Caskade checks the latest release's `latest.json` when it starts and every six hours, while **Check automatically** is on (Settings → General → Caskade updates), and downloads an update only when you ask.
- Updates to games and emulators installed from GitHub releases: Caskade checks their release pages every six hours by default (Settings → Installer), and before you play a game whose last check is more than an hour old.
- Emulators, umu-launcher (on Linux, to run Windows games) and games you install from GitHub.

**Emulator sources**, when you install or update an emulator: libretro's buildbot (`buildbot.libretro.com`) for RetroArch and its cores, Dolphin's update service (`dolphin-emu.org`), Flathub through Flatpak, and the download sites named in Caskade's emulator definitions.

**RomM servers you add**: Caskade signs in with the credentials you give and syncs your RomM library, downloads ROMs, firmware and artwork from it.

**Addresses you give it**: when you install from a link (a web page, Google Drive, Dropbox, GitHub or another host), Caskade downloads from that address. Plugins you enable may connect to the hosts their manifest declares and you grant.

**Your browser**: links to IGDB, stores and project pages open in your browser, under that site's own policies.

Each of these services handles your IP address and requests under its own privacy policy. Requests from Caskade identify themselves as `Caskade/<version>`.

## Changes

Changes to this policy are recorded in Caskade's repository (`docs/legal/PRIVACY.md`) with a new effective date, and ship with the next version of Caskade.
