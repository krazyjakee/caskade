# Ports of games

A game's page lists fan **decompilations, static recompilations and source
ports** of it: projects that run the game natively on a PC from the user's own
copy (Zelda64Recomp, Ship of Harkinian, sm64ex, …). One of them installs in a
click as another release of the same library game.

Code: `caskade_core::ports` (catalogue, matching, install plan), the install
pipeline's `install::AttachTo`, commands `game_ports` / `game_port_install`
(`commands/ports.rs`), and `views/game/PortsPanel.tsx` over `api/ports.ts`.

## Where the list comes from

[Decomp Buddy](https://github.com/TanookiStudios/decomp-buddy) publishes an
install plan per project at `https://tanookistudios.com/decomp-buddy/plans.json`
(about 1,900 projects, 10 MB, served gzipped with an ETag). Each plan names, per
system (`windows`, `linux`, `macos-arm64`, …), how the project installs
(`release`, `source`, `unsupported`), the release file and tag, the program to
start and the SHA-256 of the release file the catalogue scanned, and the files
the user supplies: which game file, its region, and whether the port asks for it
itself (`in_app_picker`), wants it in a folder (`folder`, `drop_into`,
sometimes `expected_filename`) or takes it on its command line.

- The plans are written by an AI from each project's README and releases.
  Some are wrong; the install dialog shows the plan's notes and what its author
  wasn't sure of.
- Nothing ships with Caskade. `Ports::watch_settings` fetches the file under
  the online supervisor (first-run setup done, `core.metadata.online` on),
  again a day after the last check that worked, with `If-None-Match`; a 304
  costs one small request. Failures retry after 1, 2, 4, … minutes (up to 15)
  and keep the last copy, which is what offline starts use.
- `<cache>/ports/plans.json` and `state.json` (URL, ETag, time of the last
  check) are written atomically. A project that doesn't parse is left out, never
  the catalogue; a file that isn't the catalogue at all keeps the old one.
- `core.ports` (Settings → General, device-local): `enabled` (off: nothing is
  fetched or listed) and `catalogueUrl`.
- Decomp Buddy's owner pays for generating the plans. The panel credits them
  with a link.

## Matching a library game

A plan's free-text system ("Nintendo 64", "PlayStation 1", "PC (MS-DOS /
Windows)") becomes Caskade platform ids (`ports::consoles::families`); every PC
system is one family, `pc`. A library game's families come from its entries:
an emulated entry's `platform_id`, `pc` for any other installed, owned or
launcher entry (web games excluded), plus `Game.platforms` read the same way.
A port shows only on a game it shares a family with.

Within the family:

1. **IGDB** (`PortMatch::Igdb`): when the game has an IGDB match, the plan's
   title is looked up by name in the Hasheous dumps on the device for the
   port's system (`Hasheous::search`, trying "X: Y", "X - Y" and No-Intro's
   "X, The - Y"). The IGDB ids Hasheous gives decide: the same id lists the
   port, another id rules it out even when the titles agree (Doom and Doom).
   Results are cached until the catalogue changes.
2. **Title** (`PortMatch::Title`), only when Hasheous doesn't know the plan's
   game or the library game has no IGDB match: normalized titles (`ports::title`:
   accents, case, punctuation, "The", roman numerals) of the plan's title, its
   " / " parts and its game files' labels ("Majora's Mask US ROM") against the
   game's title, sort title, aliases and the halves of "Series: Subtitle".
   The halves of the plan's title only match a whole library title.

A port counts as installed when one of the game's entries is followed for
updates from the same GitHub project (`game_updates`).

## Installing

`Ports::install` takes the plan for this system (`ports::this_system`: Windows,
Linux on x86-64, macOS on Apple silicon) when its method is `release` and the
project is on GitHub:

- The input is `https://github.com/<owner>/<repo>/releases/download/<tag>/<asset>`
  with `#sha256=<hex>` when the plan has the scanned digest; the installer
  refuses a file that doesn't match.
- `InstallService::install_attached` runs the usual install pipeline (download
  queue, storage root, Activity, retries) into a folder named after the repo,
  then `finish` adds the entry to the existing game (`AttachTo.game_id`; a game
  removed meanwhile gets an install of its own). The launch target is the plan's
  `executable`, passed to discovery as a launch hint (a bare name finds the
  shallowest file of that name, so a release's own tools don't make it ask)
  and looked for again after the install; without a match the installer's
  choice stays and the job warns. Releases zipped around one tarball (the
  N64Recomp projects do this to keep the executable bit) unpack the tarball in
  its place.
- The game file: for `folder`, the game's ROMs for the port's system are copied
  (hard-linked when the drive allows) into `drop_into`, renamed to
  `expected_filename` when there is one ROM; for `command_line`, the first
  ROM's path is added to the launch arguments; for `in_app_picker` nothing is
  copied and the port asks on first start. A ROM the library doesn't have is
  for the user to give the port.
- The download URL makes the install followed by game updates
  (`updates::GitHub`), so newer releases are offered like any GitHub install.
  Files the user added (the copied ROM, saves) are carried over by updates.

Not yet: ports that must be built from source (listed with a link), GitLab
projects, Windows-only ports on Linux through Proton, and a "works with your
copy" badge from the plan's hashes (and Hasheous for plans without them).

## Tests

`cargo test -p caskade-core --lib ports::` covers parsing, families, matching,
the ETag refresh and install plans against a small catalogue in the published
shape; `install::tests::an_attached_install_*` the attached install. Two
ignored tests use the real services: `ports::tests::live_catalogue_parses`
(the published plans) and `ports::tests::live_a_port_installs_into_the_game`
(Zelda64Recomp's Linux release, with a wrong checksum refused first):
`cargo test -p caskade-core --lib ports::tests::live_ -- --ignored`.
