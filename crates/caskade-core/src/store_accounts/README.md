# Store accounts and owned games (#164)

`StoreAccounts::new(store, bus)` keeps launcher and store accounts (Steam,
GOG, Epic, plugins' stores) and what each owns, separately from what is
installed. Remote stores never run in `caskade-discovery`, which stays
offline: an `AccountProvider` (built-in or from a plugin) lists an
account's library, and `register_provider` makes it available.
`ProviderCapabilities` say how it signs in (`browser`, `device`, `apiKey`,
`launcher`) and whether it lists libraries, play time and installs.

## Records

- `store_accounts` (migration 0022): the provider, the account's id at the
  provider (its ownership namespace: `connect` with the same pair returns
  the same record, so a reconnect never duplicates or mixes libraries),
  a display name, the local account that connected it (`owner_id`, NULL
  for everyone's), `enabled` (off after Disconnect) and the sync state
  (last complete refresh, last attempt, the last error as an
  `ErrorReport`). Credentials are never in the database (#165).
- `account_entitlements`: one row per account and product id, with the
  product's title, store ids (`ExternalId`) and platforms, first and last
  seen, `lost_at_ms`, and the library entry it shows as.

An owned product is a real library game: a release from source
`store:<account id>` (`store_accounts::source_id`), state `partial`, with
no location and no launch. It is never installed or playable by itself;
an installed release (discovery's, a managed install) is. A game owned by
two accounts has an entry from each.

## Refresh

`refresh(id, cancel)` asks the provider and applies the snapshot in one
transaction (`LibraryStore::apply_snapshot`):

- A new product joins the game whose entries carry one of its store ids
  (an installed copy discovery already found, else another account's), or
  becomes a new game. Existing games keep their own fields; a refresh
  only adds store ids to its own entries.
- Products listed again are updated; lost ones listed again are regained.
- Only a complete snapshot marks products it doesn't list as lost (kept,
  never deleted). A partial snapshot, an error (offline, expired,
  rate-limited, cancelled) or a disconnected account leaves what was known,
  and the error is kept in the sync state.
- A game the user removed leaves its entitlement without an entry; later
  refreshes don't bring it back.

One refresh per account runs at a time: one asked for meanwhile gets the
running one's outcome (a cancelled one answers only its caller). Refreshes
report as "Refreshing <account>" activity and `LibraryGame*` events.

## Reconciling with discovery

`reconcile()` merges games that share a store id with an account's entry
(`LibraryStore::store_id_groups`): an owned game, and an installed copy
discovery imported as a game of its own. The older game is kept and
`merge_games` moves every release, session, collection membership and
artwork. Hidden games and games kept apart (Separate on the game page)
stay out. It runs after every refresh and, through `watch()`, after every
library scan.

## Disconnect and removal

`disconnect` stops refreshes and keeps every record and game.
`removal_preview` lists the games removing the account would delete (only
its entries) and those that stay (installed, added another way or owned by
another account), which lose only its entries; `remove` does it.

## App

`commands/store_accounts.rs`: `store_providers`, `store_accounts_list`,
`game_ownership` and `store_account_removal_preview` are reads,
`store_account_refresh` needs someone signed in, and
`store_account_disconnect` / `store_account_remove` are for admins.
`StoreAccountsChanged` follows every change.

## Connecting and signing in (#165)

`begin_connect(provider, reconnect)` starts a sign-in: the provider's
`begin_auth` returns what the user does (`AuthChallenge`: a page to open, a
code to enter), and the flow (`AuthFlow`) expires after ten minutes. The
flow id doubles as the OAuth `state`. `complete_connect(flow, input,
owner)` finishes it with an `AuthInput`: a browser `Callback` (its `state`
must be the flow's, or the flow is cancelled: `ConnectError::Mismatch`), a
pasted `Code` or API key (a wrong one leaves the flow open), a device
`Poll` (`ProviderError::Pending` until done) or a `Launcher` sign-in. An
expired or cancelled flow answers `ConnectError::Expired`. Reconnecting an
account as someone else changes nothing (`ConnectError::OtherAccount`): one
account's sign-in or library never goes to another. The app routes
`caskade://store-auth?state=…` links to the flow they name.

Sign-ins (`StoreCredentials`: tokens, expiry, provider extras) are kept by
`secrets::Credentials`: the OS keychain under `KEYCHAIN_SERVICE`
(`dev.caskade.store-accounts`, apart from RomM's), keyed by store account
id, or in memory for the session when the keychain fails
(`StoreAccountSummary::session_only`). They are never in the database,
settings, command answers, logs (`Debug` hides them) or backups. Before a
refresh, a sign-in expiring within a minute is renewed with
`refresh_credentials`; one that can't be is `Expired`.

`StoreAccountSummary::state` is `notConnected` (disconnected),
`authenticating` (a reconnect under way), `syncing`, `offline` or
`expired` (from the last refresh's error, or no sign-in kept), `connected`,
or `unsupported` (no provider this session).

Refreshes count failures; a store's Retry-After (`RateLimited`, 15 minutes
when unsaid) sets `retry_at_ms`, and no refresh, manual or not, asks before
then. `cancel_refresh` stops one under way. Background refreshes
(`core.storeAccounts`: `backgroundRefresh`, off by default, and
`intervalHours`, 24) run from `watch()`: a minute after start and every five
minutes, each connected, signed-in account an interval after its last
attempt, after 30 minutes doubling four times when it failed. They share
the account's single-flight refresh, and their failures are only recorded
on the account. `disconnect` cancels a refresh, revokes the sign-in where
the provider can (`revoke`), deletes it, and keeps the library; `remove`
also deletes the sign-in.

In the app, `store_connect_begin` / `_complete` / `_cancel`,
`store_auth_flows` and `store_account_cancel_refresh` join the commands
above (connecting is for admins). Settings → Store accounts
(`views/settings/stores/`, admins only) lists each provider with Connect,
the sign-in (page link, device code, key field, or nothing for a
launcher), each account's state, counts, last refresh and error, Refresh
or Stop, Reconnect, Disconnect (explaining the library stays) and
Remove… (with the preview), and the background refresh settings.
`api/storeAccounts.ts` falls back to `storeAccountsFixtures.ts` in the
browser preview.

## Providers

Providers live beside the service, behind the `net` feature, on the shared
`http` helpers (one client, bounded answers; 429 is `RateLimited` with the
store's Retry-After, 401/403 `Expired`, 5xx and connection failures
`Offline`; URLs carry keys and are never logged). Sign-in challenges can
ask for several values (`AuthChallenge::fields`, answered with
`AuthInput::Fields`) and say what to know first (`instructions`).
`OwnedProduct` carries `playtime_minutes` and `last_played_ms` only when the
store supplies them; #169 reconciles them with local sessions.
`StoreAccounts::credentials(account)` gives other core features (achievements,
#186–#188) an account's sign-in, renewed when it can be; it never leaves
the core. A provider with `library: false` (RetroAchievements, #189) has
nothing to list: its Refresh runs the `AccountRefresher` the app sets
(`Achievements::account_refresher`), with the same flight, cancel, wait and
error record as a library refresh. Background refreshes skip it.

### Steam (#166, `steam.rs`)

- Sign-in: the user's Steam Web API key (from
  steamcommunity.com/dev/apikey) and their profile: a SteamID64, a profile
  link, or a custom URL name resolved with `ISteamUser/ResolveVanityURL`.
  `ISteamUser/GetPlayerSummaries` then names the account and checks the
  key (a refused key or an unknown profile is `Rejected`). The key and the
  SteamID64 (`extra.steamid`) are the sign-in; the SteamID64 is the
  account's id.
- Library: `IPlayerService/GetOwnedGames` with app info and played free
  games, in one answer (no pages). Games only: DLC, tools, soundtracks,
  videos and Family Sharing loans aren't listed, so aren't imported; free
  games are imported once played, when Steam lists them. Nothing is
  inferred from the store catalogue. Each game's id is
  `ExternalId::Steam { app_id }`, the same as Steam discovery's, so an
  installed copy is the same game. `playtime_forever` (minutes, 0 included)
  and `rtime_last_played` (when not 0) are reported; a game without them
  has neither.
- A private profile (game details not public, with a key from another
  account) answers without a game list: `Unavailable`, with what to change
  in Steam's privacy settings, and the library already known stays.
- Steam doesn't say which systems a game runs on: platforms stay unknown,
  and installing goes through the Steam client (#170).
- `SteamProvider::api(key)` is the Web API client other Steam features use.
- Tests: `steam_tests.rs` with sanitized answers in `fixtures/steam/`;
  `live` (ignored) signs in with `CASKADE_STEAM_PROFILE` and
  `CASKADE_STEAM_KEY` and refreshes.

### GOG (#167, `gog.rs`)

- Sign-in: GOG's login page (`auth.gog.com/auth` with GOG Galaxy's client
  id, as Heroic, Lutris and Playnite use it) ends on
  `embed.gog.com/on_login_success?…&code=…`, an address Caskade can't
  receive: the user pastes it (or the code), and `auth.gog.com/token`
  exchanges it for an access token (an hour) and a refresh token, which
  renews it before refreshes (`refresh_credentials`; a refused refresh token
  is `Expired`). `embed.gog.com/userData.json` names the account; its
  `userId` is the account's id.
- Library: `embed.gog.com/account/getFilteredProducts?mediaType=1`, every
  page (at most 500). Games (`isGame`) only: movies aren't imported, and
  DLC and extras (goodies) aren't products of their own there. A product
  repeated across pages counts once. Each game's id is
  `ExternalId::Gog { product_id }`, as GOG Galaxy, Heroic, Minigalaxy and
  Lutris discovery and managed GOG offline installers (`goggame-*.info`)
  record it, so all those copies are one game, managed storage untouched.
  `worksOn` becomes `systems` (`windows`, `mac`, `linux`); no install
  location is invented.
- One failed, cut-short or cancelled page fails the refresh: the library
  already known stays (no partial snapshot).
- No play time: capabilities say `playtime: false`.
- Tests: `gog_tests.rs` with sanitized answers in `fixtures/gog/`; `live`
  (ignored) signs in with `CASKADE_GOG_CODE`, a fresh code from the login
  page, and refreshes.

### Epic Games (#168, `epic.rs`)

- Sign-in: Epic's login page, redirected to its redirect API with the Epic
  Games Launcher's client id (as Legendary and Heroic sign in), shows a
  short JSON text with an `authorizationCode`; the user pastes the code (or
  the text), and the account service's OAuth token endpoint exchanges it
  (`token_type=eg1`) for a two-hour access token and a refresh token, which
  renews it (a refused renewal is `Expired`). The token names the account:
  `account_id` is its id, `displayName` its name.
- Library: the library service's items, page by page (`nextCursor`, at
  most 1,000 pages). Records have no titles, so the catalogue's bulk items
  per namespace (50 at a time) give each its title, categories, main game
  and platforms. Imported: public items in the `games` category that
  aren't an add-on (`mainGameItem`, `addons`, `digitalextras`). Not
  imported: DLC and add-ons, Unreal Engine marketplace items (namespace
  `ue`), private sandboxes and items the catalogue doesn't return. Claimed
  free games are library records like bought ones. A record that leaves
  the library (an ended entitlement) is marked lost by the next complete
  refresh, never deleted.
- Each game's product id is `namespace:catalogItem`, its id
  `ExternalId::Epic { namespace, catalog_item, app_name }` like Epic
  launcher, Heroic and Legendary discovery's. Store ids match on namespace
  and catalog item alone (an app name differs between some sources, and an
  id without both names nothing for sure), so local copies converge with
  the owned game whatever their app name, and two editions sharing a title
  stay two games. Installed state still comes only from discovery.
- Play time: `library/api/public/playtime/account/<id>/all` (seconds, as
  minutes) for the apps Epic lists; a game it doesn't list, or a failed
  play-time call, leaves play time unknown. Epic gives no last play.
- Any library or catalogue page that fails, a 429 (with Retry-After) or a
  cancellation fails the refresh: the cached library stays.
- Tests: `epic_tests.rs` with sanitized answers in `fixtures/epic/`; `live`
  (ignored) signs in with `CASKADE_EPIC_CODE`, a fresh code from the login
  page, and refreshes.

## Play time (#169, `db/playtime.rs`)

Play time stores and launchers report is kept apart from the sessions
Caskade tracks: a `remote_playtime` row (migration 0024) per release,
provider and account (`''` for a launcher's own total on this computer),
with the latest and largest total (NULL when unknown, which isn't zero),
the last play when the source gives one, when it was read, and what the
game's total counts for it. A game's play time (Library, Home, details,
Statistics: `PLAYTIME` in `db/query.rs`) is its sessions plus the rows'
`counted_ms`; its last play the latest session start or reported last
play. No session is ever made up from a total.

Rows are recounted per game whenever a total arrives or games merge
(`merge_in`), by family, the store identity a total is about:

- one family's account totals add up, and a launcher's own total overlaps
  them: the family counts the larger;
- each total counts the largest value seen, so a reset or a lower total
  never lowers the game's time, and an older read than the one kept is
  ignored;
- sessions Caskade tracked on the family's releases that ended before its
  newest total was read are inside that total: only what the total has
  beyond them counts, never less than nothing; later sessions count until
  a newer total covers them;
- different families (a Steam and a GOG copy) add up; totals of releases
  that name no store are one family, the game's own play.

The migration moved discovery's aggregate seed sessions (#100) into
launcher rows once; discovery now seeds launcher rows on a game's first
import only. Store totals follow `core.storeAccounts.playtimeImport`
(settings v2): `always` (the default) records every refresh, `newGames` a
release's first total only, `never` none and removes the account's. Totals
survive disconnecting; removing an account removes its releases and so
their totals. Changed games are announced with `LibraryGameUpdated`.
`game_playtime` (`PlaytimeBreakdown`) says what was tracked and what each
source reported; the game page shows it beside the play time
(`views/game/PlaytimeSources.tsx`). All arithmetic saturates.

## Installing owned games (#170, `install.rs`)

`owned_releases(game, launchers)` lists a game's owned releases with an
`OwnedInstallState`: `installed` (discovery found a copy), `available`
(Install opens its launcher), `installing` (handed off, waiting for
discovery), `launcherMissing`, `unsupportedSystem` (the store's systems
don't include this one), `unavailable` (no longer owned, disconnected, or
no provider), or `notSupported`. `launchers` are the discovery sources
whose launcher was found (`DiscoveryController`'s last detection). Each
release says when its account's library was last read in full, so an
offline library stays browsable with its age shown.

`install(entry, launchers)` opens the provider's `install_target` URI
(`steam://install/<app>`, `goggalaxy://openGameView/<id>`,
`com.epicgames.launcher://apps/<ns>%3A<item>%3A<app>?action=install`)
through the system's URI handler (`set_opener` swaps in a fake launcher in
tests) and marks the release as installing, for a day at most. Nothing is
claimed: discovery finding the installed copy, which joins the owned game,
makes it `installed`. `cancel_install` stops waiting; a failed handoff
changes nothing. Uninstalling through the launcher leaves the owned
release, which offers Install again. Installs never change the game's
default release.

The Library's Ready to Install tab (`GameFilter::Owned`, counted in
`LibraryStats::owned`) lists games an account still owns with no copy
installed. The game page's `OwnedReleases` (`views/game/`) shows each
owned release, its freshness and state, and Install with <launcher> (the
page's main action when nothing is installed), by mouse, keyboard or pad.
Commands: `game_owned_releases`, `owned_install` (installs: the Install
permission) and `owned_install_cancel`.
