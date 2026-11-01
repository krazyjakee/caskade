# Game editing

The user's own values for a game's details, kept apart from what automatic
sources (discovery, installs, IGDB, RomM, ROM scanners) write.

## Manual fields (#171)

`caskade_core::model::GameEdit` is a patch of chosen fields:

- title and sort title
- description
- aliases, genres, tags and platforms
- release date
- personal rating and notes

`LibraryStore::edit_game` applies it all or nothing. Fields left out of the
patch are never written, so a refresh that runs while the editor is open
keeps what it wrote to them. Unknown keys fail the whole patch at
deserialization.

Each change is one of:

- **Set**: the user's value. An empty value is an intentional clear: no
  description, no genres, no release date. The title can't be empty.
  Titles are trimmed. List values are trimmed, and empty ones and repeats
  are dropped.
- **Add / Remove** (lists, for bulk edits): values join or leave the list,
  whatever else it holds.
- **Reset**: back to the automatic value, and automatic sources control the
  field again.

Rating (0–100) and notes are the user's alone. They are set or cleared,
never reset.

A rejected patch saves nothing. `GameEditOutcome::Rejected` lists a
`FieldError` per field, and its `problem` is one of two:

- **invalid**: the value isn't allowed, with the core's message.
- **conflict**: the field changed since the editor loaded it. The editor
  sends that game as `base`, and a set field whose stored value is no longer
  `base`'s (and isn't already the new value) is a conflict, not an
  overwrite. Adds, removes and resets never conflict.

The rejection carries the game as it is now, so the editor can reload.

### Storage

`games.record_json` and its columns (title, sort title, aliases, tags,
search index) hold what the library shows, the user's values included.
Summaries, search and collections therefore read edits like any other
value.

`games.overrides_json` (migration 0020) maps each field the user set to the
value automatic sources would give it, using the camelCase `Game` keys.
`GameEditState` gives the editor:

- `game`: what the library shows
- `automatic`: the layer beneath, which is what Reset restores
- `manual`: which fields the user controls

Automatic writers write beneath the user's values:

- **`update_automatic_game(id, |game| …)`**: for read-modify-write sources
  (IGDB's apply, a match by hand's rename, RomM's sync, ROM scanners). The
  closure sees the automatic values and changes them, so their own
  "only what the last sync wrote" rules keep working. A field the user set
  keeps its value, and only what Reset restores changes.
- **`upsert_game`** and the import paths: for writers that save a whole
  game they built or read. Where the game differs from a field the user set,
  the difference becomes the new automatic value and the user's value stays.
  Where it equals the shown value, the writer left the field alone.
  Discovery imports keep an existing game's details anyway; they write a
  game only when they create it.

Hiding, favourites and the default entry have their own commands and are
never manual fields.

### Merges and separation

A merge (`merge_games`, discovery joining games, IGDB's same-game groups)
merges what the library shows as before:

- Lists are joined.
- The kept game's title, description and release date come first.
- Favourites and hiding are kept.
- Notes are appended under the donor's title, never dropped.
- The donor's title, and its automatic title when the user renamed it,
  become aliases.

The automatic layers merge the same way beneath. A field the user set on
either game stays the user's. A description or release date the user set
on the donor beats an automatic one on the kept game. The donor's title and
sort title never become the user's: the kept game's name stands.

Separating an entry (`separate_entry`) copies the fields the user set to the
new game. The platforms the user picked for each game become both their
shown and their automatic platforms.

### Commands

`game_edit_state(gameId)` returns the state, or `null` when the game is
gone. `game_edit(gameId, edit, base)` returns a `GameEditOutcome`, and emits
`LibraryGameUpdated` when something changed (`commands/game_edit.rs`).
Reading needs nothing; editing needs someone signed in, like the IGDB match.

## The editor (#172)

`components/gameEditor/GameEditor.tsx` opens from these places:

- a game's menu: **Edit…**, from any `GameMenuHost` entry point (card, hero, search suggestion, quick look)
- the game page's **Edit details** button
- `useEditGame()`

It lives in the host, so a virtualized card can unmount. In desktop mode it is a dialog; in console mode it fills the window.

### Details tab

**Fields:** title and sort title, description, genres, tags, platforms, other names (aliases), release date, your rating and notes. Lists are chips with their own remove buttons and an add field. Genres and tags accept several values separated by commas.

**Draft:** the tab keeps a draft per field (`draft.ts`), and Save sends only the changed fields as one `GameEdit`, with the game as the editor opened it as `base`. While the sort title matches the title, it follows the title.

**Manual fields:** a field the user set shows **Yours** and a **Reset** button. Reset previews the automatic value until Save. Rating and notes have no Reset.

**Rejected saves:** each `FieldError` shows beside its field, and the footer says nothing was saved. A conflict shows the value saved meanwhile and offers two choices:

- **Use theirs**: drops the draft for that field.
- **Keep mine**: the next Save overwrites it.

**Failed calls:** a call that fails keeps the draft, and Save becomes **Try again**.

### Other tabs

**Actions** and **Scripts** belong to the launch actions work (#178, #180). Each is a component with `GameEditorTabProps` (`types.ts`), loads and saves on its own, and reports unsaved work through `onDirtyChange`.

Every tab stays mounted, so switching tabs keeps unsaved work. Closing with unsaved work in any tab asks first: Keep editing, or Discard.

### Gamepad

- **LB/RB:** switch tabs
- **Start:** Save
- **B:** close (with the same question when something is unsaved)
- **A on a text field:** opens the on-screen keyboard

### API

`LibraryApi.edits` (`api/gameEdit.ts`, fixtures in `gameEditFixtures.ts`). A save that changed something invalidates the whole library, so cards, Home, collections and the page update at once.

## Artwork (#175)

The editor's **Artwork** tab (`components/gameEditor/ArtworkTab.tsx`) controls a game's cover, background (hero) and icon: the kinds the app shows one of. Screenshots show as a strip and logos show nowhere, so they get no controls.

Each kind offers:

- **Choose** one of the images automatic sources supplied (IGDB, RomM, discovery, installers, web pages).
- **Import image…**: a file through the path picker, which is the native dialog in desktop mode and the in-app browser by pad in console mode. Dropping a file on the kind's area does the same.
- **Show none**.
- **Reset**: back to automatic precedence.

Changes apply at once, with no Save. A failed import leaves the game as it was.

### Storage

`artwork_choices` (migration 0021) lists the kinds the user controls. Under a choice the game shows only its `user`-source artwork of that kind, or none when the user removed it (`USER_ARTWORK_SOURCE`).

**Choosing an automatic image** copies its record as the user's. The cache bytes are shared, so an IGDB or RomM refresh that replaces or deletes the automatic record leaves the choice alone.

**Importing** goes through `ArtworkCache::import_bytes`:

- The file is checked against the 16 MiB limit before it is read.
- PNG, JPEG and WebP only, with the cache's decoding limits.
- Content-addressed bytes, with the 96, 320 and 960 px thumbnails.

Automatic sources keep writing their own images beneath a choice:

- IGDB still downloads its cover when the user chose one.
- Executable icons still come when the user chose an icon.

Reset deletes the `user` records of that kind and the choice. It never deletes cache bytes, which other games or records may share.

### What reads it

`LibraryStore::shown_artwork` (the game page) and the library's cover and icon queries (cards, rows, Home, collections) apply the choices.

### Merges and separation

- **Merge:** a kind both games chose keeps the kept game's image, and a kind only the donor chose stays chosen.
- **Separation:** copies the choices.

### Commands

- `game_artwork(gameId)` returns `ArtworkSlot`s: `choice` (`automatic`, `chosen` or `removed`), `shown`, and the `automatic` images in precedence order.
- `game_artwork_set(gameId, kind, change)` takes `choose {artworkId}`, `import {path}`, `remove` or `reset`, emits `LibraryGameUpdated`, and returns the slots.

## Selection and bulk edits (#173)

The Library and collection pages have a **Select** button. In selection mode:

- A click, Enter or a pad's A on a card or row chooses it rather than opening it.
- Shift+click chooses the loaded games between the last one chosen and this one.
- B or **Done** leaves selection mode.

The selection is a set of game ids (`components/gameSelection/selection.tsx`), so it survives the virtualized grid's paging and scrolling. Cards and rows re-render only when their own state changes.

### The selection bar

`SelectionBar.tsx` shows how many games are chosen, and offers:

- **Select loaded**: the games paged in so far.
- **Select all N games**: every game the view finds. In the Library that comes from `library_game_ids(query)`; on a manual collection, its members; on a smart collection, its query.
- **Clear**.
- **Edit selected…**: also a pad's Start on a chosen card.
- **Done**.

### The bulk editor

The bulk editor (`BulkEditDialog.tsx`) changes only ticked fields, so values that differ between the games stay as each game has them:

- tags, genres, platforms and other names: add, remove, replace or reset to automatic
- description: replace or reset
- release date: set (empty clears it) or reset
- rating
- notes

**Preview** runs `games_bulk_edit(ids, edit, apply: false)`, which saves nothing. It lists:

- how many games change
- how many already have the result
- how many can't take it, with each game's field errors
- how many are gone
- each changing game with its before → after values (the first 200; the count covers the rest)

**Apply** runs the same call with `apply: true`:

- Each game saves in its own transaction (`LibraryStore::bulk_edit_games`), and the work yields every 50 games.
- The result says which games saved, which already had it, and which failed.
- The adapter emits `LibraryGameUpdated` per changed game, and its 200 ms coalescer merges them.

Adds, removes and sets are idempotent, so **Try again** after a failure redoes nothing that saved. An add or remove that leaves a list unchanged doesn't take the list over from automatic sources. Cancel or Back before Apply saves nothing. One bulk edit takes at most 10,000 games.

## Games added by hand (#174)

**Add game** has two more choices besides installing (`components/manualGame/ManualGameDialog.tsx`).

### Add a game on this computer…

Takes a program, a shortcut or a URI. You can type it, Browse for it (the path picker, by pad in console mode) or drop it on the dialog. Caskade starts it where it is: nothing is copied, moved, installed or run.

`caskade_core::manual::resolve_target` works out what it starts, and the dialog shows that before adding:

- **A `.desktop`, `.lnk` or `.url` shortcut:** its target, with the shortcut's arguments, working folder and environment, through the same parsers as a drop (`install::dropped::shortcut_target`). A Wine menu entry gives its game in its `WINEPREFIX`. Launchers' and Flatpaks' shortcuts are refused, as for drops: discovery finds those games.
- **A program:** started directly with no arguments, never through a shell. A Windows program (`.exe`, `.bat`) gets a Windows runtime, so it runs through a compatibility tool off Windows.
- **A URI** (`steam://…`, `https://…`): opened as it is.

Folders, relative paths and missing files are refused with the reason. Targets go through the launch actions' `validate_target`.

The game gets a `manual` entry (`MANUAL_SOURCE`): installed, located in the program's folder, with the target as its built-in launch. Named actions (#177) attach to it, and its built-in target stays editable in Edit details → Actions.

### Add a game by title…

A title-only record, for a cartridge or a disc on a shelf. It has no entry. Its page says Caskade can't start it and points to Edit details → Actions, where adding an action creates its `manual` entry. It keeps notes, artwork, collections and IGDB matching like any game.

### Duplicates

Before adding, `LibraryStore::manual_duplicates` lists games that may be the same: one that starts the same program or URI, or whose title or another name has the same words. The dialog offers:

- **Add to this game**: the target becomes a `manual` release of that game.
- **Keep separate**: a new game in `kept_apart`, so IGDB's same-game merging leaves it alone.

A game a launcher finds later is a game of its own until a merge, which keeps both releases and the notes.

### Removing

**Remove from library…** (`InstallService::remove_game`) accepts `manual` releases and title-only records. It deletes the records only, never the game's files.

### Commands

`manual_game_target`, `manual_game_duplicates` and `manual_game_add`, in `commands/manual_games.rs`.

## Refreshing chosen games (#176)

The selection bar's **Refresh details…** (`RefreshDialog.tsx`) refreshes the chosen games' details and artwork from the built-in sources. Matching uses RomM's IGDB ids, Hasheous names and store ids; the details come from IGDB through Caskade's metadata server. There are two modes:

- **Missing details only:** unmatched games are matched now. Matched games apply again from what the IGDB cache has, which fills in what they lack without asking the server for games it already answered.
- **Refresh automatic details:** also fetches matched games again (`Igdb::refresh`, which refetches answers older than ten minutes) and replaces their automatic details and artwork.

Either way the user's fields (#171) and artwork choices (#175) stay, and so do games set to "not on IGDB" (counted as skipped).

`Metadata::refresh_games` goes through the background runs' own code (`run_games`, `apply_rows`):

- Games go in batches of 50, with one request per batch, so the cache, the server's cooldown and `Retry-After` stay authoritative.
- When the server or an image host can't be reached, that batch and the rest are listed as failed, and keep what they had.
- Cancelling stops after the current batch and lists the games left.

One refresh runs at a time (`start_refresh`, `cancel_refresh`). Its `RefreshStatus` goes to the webview as `GamesRefreshChanged`:

- the mode, done of total
- updated, unchanged, not found on IGDB, skipped and failed games
- whether it was cancelled, and the games left

Activity's Jobs tab shows it (`RefreshCard.tsx`) with **Cancel**, **Retry the failed games** and **Continue with the games left**. Updated games emit `LibraryGameUpdated`, which the adapter merges, and the webview refreshes the library once the run ends.

Commands: `games_refresh_start(gameIds, mode)`, `games_refresh_cancel` and `games_refresh_status`.
