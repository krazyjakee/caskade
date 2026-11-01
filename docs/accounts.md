# Local accounts

Everyone using Caskade on a machine can have an account: a name, one of the
predefined avatars and an optional PIN. Accounts are local to the machine.
There is no online sign-in, sync or server.

## Roles and rules

- An **admin** changes everything, other accounts included. A **standard**
  account changes what its admin allowed: some Settings sections, and
  whether it may install, uninstall and move games. Admins also set the
  highest minimum age a standard account may play without an admin PIN.
- An existing library becomes one admin without a PIN, and starts as
  before, without an account picker. A single account without a PIN is
  always signed in at start.
- The last admin can't be deleted or made standard.
- A standard account can exist only while every admin has a PIN, so an
  admin can't remove their PIN while standard accounts exist.
- Only an admin changes an account, except that every account changes its
  own name, avatar and PIN. The core checks this, whatever the UI shows.
- Collections belong to the account that made them, and are deleted with
  it. The library, installs, play sessions, favourites, hidden games and app
  settings are shared.

## Settings → Accounts and My profile

Admins manage accounts in **Settings → Accounts**
(`apps/desktop/src/views/settings/accounts/`): the list with Add, Edit and
Delete, and the **rating board** (`core.accounts.ratingBoard`, PEGI by
default) that games show their rating in and age limits are picked from.
No Settings section owns `core.accounts`, so only admins change it.

- The editor sets an account's name, avatar, role and PIN, and for a
  standard account: **Install games**, one switch per Settings section, the
  **maximum age rating** (no limit, or one of the board's steps, stored as
  its minimum age) and **unrated games need a PIN**
  (`Account::unrated_needs_pin`, off by default; it applies only under an
  age limit).
- A change that breaks a rule above (the last admin, an admin without a
  PIN while standard accounts exist) shows the core's reason; nothing
  changes. Deleting asks first and says the account's collections go with
  it.
- **My profile** is for every account: its own name, avatar and PIN.
  Changing or removing your own PIN (here or in the editor) asks for the
  current one first, as a sign-in, so wrong tries count.
- A standard account never sees Accounts, and sees only the sections it
  may change, My profile, Statistics and About; a link to another section
  says to ask an admin. The tabs follow `account.changed`, so a change an
  admin makes applies at once. PINs are typed in the field or on the PIN
  pad, so all of it works with a controller.

## Permissions

The app checks every command against the signed-in account before it runs
(`apps/desktop/src-tauri/src/permissions.rs`), so the rules hold with the
webview bypassed. Each command is in one group, and a test fails when a
registered command has none:

- **Reads** are always allowed, signed in or not: the UI needs the values
  to render.
- **Shared changes** (play, favourites, hidden games, collections, updates
  of installed games) need someone signed in.
- **A Settings section** (`general`, `library-discovery`, `launchers`,
  `emulators` with RomM and Get emulators, `appearance`, `storage`,
  `installer`, `plugins`): its settings namespaces and the commands that
  change its state. Each plugin's settings fall under `plugins`; a
  namespace no section shows is for admins.
- **Install**: install requests (pastes, drops and deep links), installs,
  ROM scans, RomM syncs and downloads, games added by hand, uninstalls and
  moves.

With nobody signed in, only reads and signing in are allowed. A refusal
says why (`Error::Forbidden`); a refused deep link queues nothing and
reaches the UI as `installRefused`. The app's own background work (scans,
metadata, game and emulator updates) never goes through commands, so it
runs whoever is signed in.

## Age limits on play

A standard account's age limit is the highest minimum age it plays
without an admin PIN (`caskade_core::accounts::age`). A game's minimum age
is the highest of its ratings' (`caskade_core::igdb::age`); an unrated game
needs a PIN only where the admin chose that for the account. Admins and
accounts without a limit are never asked.

- The launch pipeline asks before anything else a launch does (RomM
  downloads, runners, emulators): `LaunchService::with_play_gate`, which the
  app sets to the accounts. Every launch command goes through it, and a
  launch with no UI to ask (a plugin's) fails with the reason.
- A refused `launch_game_with` answers `age_restricted` with the game's
  title and ratings. Play asks `game_age_check` first, so the PIN comes
  before an update before play; the game page and its quick look show
  "Admin PIN needed".
- Normal Library browsing and its filters omit games that need approval,
  including unrated games when the account requires it. Counts, platform
  lists, paging, the alphabet bar and Select all follow that same rule.
  Search suggestions and Library search results still include them, with
  a lock and **Admin PIN needed**. Searching or opening a game grants no
  permission to play. These rules never change the shared hidden setting.
- The admin PIN dialog names the game and signed-in account, and offers
  **Unlock this time** (the default) and **Always allow this game for this
  account**, using keys, mouse or controller. `game_age_approve` takes that
  account's id and rejects an approval if the signed-in account changed.
  A cancelled dialog or wrong PIN grants nothing; wrong tries count as in
  the admin PIN check below.
- **Unlock this time** keeps an approval in the core (the webview gets only
  the `PinCheck`): one start of that game spends it, any change to the
  accounts or who is signed in drops it, and it lapses after 30 minutes.
  It does not change browsing visibility or grant a persistent exception.
- **Always allow this game for this account** stores an exception in
  `account_game_approvals` (migration 0027), keyed by account and game.
  It survives restarts and account switches, makes the game appear in
  normal browsing, and lets future launches pass the core's age gate.
  Other games, other accounts and the account's overall age limit stay as
  they were. Deleting the account or game deletes its exceptions; backups
  include them with the rest of the account data.

## The PIN is a household lock

A PIN keeps other people at the keyboard out of an account: a child out of
an admin account, or one sibling out of another's collections. It does
**not** protect anything on disk. The library database, settings, caches and
game files are readable by anyone who can read the user's files, and
anyone who can edit the database can remove a PIN.

- A PIN is 4 to 8 digits. Only its argon2id hash, with a salt of its own,
  is stored. The PIN never reaches logs, events, error messages or the
  webview.
- After 5 wrong PINs an account waits 30 seconds before the next try; each
  further run of 5 doubles the wait, up to 15 minutes. The count is stored
  with the account, so restarting Caskade doesn't reset it. A right PIN
  does.
- The admin PIN check (used to play a game above an account's age limit)
  answers only whether some admin's PIN matched, never which. A wrong PIN
  there counts against every admin it was tried on.
