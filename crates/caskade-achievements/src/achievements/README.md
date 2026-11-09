# Achievements (#185)

`Achievements::new(store, bus, accounts)` reads achievements for the store
accounts the library has connected (`store_accounts`, #164/#165) and caches
them in the library database (migration 0023). Providers
(`AchievementProvider`) only read; nothing is ever unlocked, awarded or
changed at a service.

## Sets

A set is one store account's achievements for one service game:
`achievement_sets` (account, product id) with its achievements in
`achievement_items` (the service's definition and the account's state).

A game's sets are found from its releases:

- a release's store ids (`ExternalId`), which the provider maps to its
  game id (`AchievementProvider::product`): a Steam app id, a GOG product id,
  an Epic namespace and catalog item;
- a link (`achievement_links`): a release's game at a service that its
  store ids don't give, verified by ROM hash or chosen by hand
  (RetroAchievements, #189).

Each visible account of that provider gives the game a set: a game owned on
Steam and GOG, or by two Steam accounts, shows each separately, and nothing
is joined by title. Sets belong to releases, so a merged game shows its
releases' sets side by side and a separated release takes its own along.
A viewer (the active local account) sees the sets of store accounts it
connected and of shared ones (`owner_id` NULL).

## Fetching

`refresh_game(game, viewer, cancel)` fetches each set; one fetch per set at
a time. What comes back:

- **Complete**: definitions replaced in the service's order; achievements it
  no longer lists go. The first complete answer is the set's *baseline*.
- **Incomplete** (a page failed): only what it lists changes; nothing is
  dropped.
- An unlock already seen stays unlocked whatever an answer says (services
  don't relock; a locked answer after an unlock is a partial one). Hardcore
  unlocks are kept apart from standard ones.
- Unlocks first seen after the baseline are announced
  (`Event::AchievementsUnlocked`, when `notifyUnlocks` is on); the first
  fetch's history never is.

`SetSync::status` tells apart: `not_fetched`, `ready`, `no_achievements`
(the service says none), `unsupported`, `private` (the provider's
`Unavailable`), `sign_in_needed` (disconnected, expired or rejected). An
offline or failed fetch keeps the status and data and records the error
(stale); a service's Retry-After (`RateLimited`, 15 minutes when unsaid)
sets `retry_at_ms` and no fetch asks before then. A cancelled fetch
records nothing. `Event::AchievementsChanged { game_id }` follows every
fetch.

Sign-ins come from `AccountAccess::credentials` (the store account's
keychain sign-in, renewed when about to expire) right before each fetch;
they are never stored or logged here.

## Background

`watch()` while `core.achievements` (`enabled`, `background`,
`notifyUnlocks`, all on by default) allows: a game's sets 15 s after it
exits, and each hour the sets of games played in the last fortnight that
weren't fetched for six hours, at most ten per round. A service's wait is
always kept.

## RetroAchievements (#189)

`retroachievements::RetroAchievements` is both a store-account provider and
an achievement provider:

- **Connecting.** It takes a user name and web API key (`ApiKey` sign-in, checked with `API_GetUserProfile`) and has no library (`library: false`). The key lives in the keychain like any store sign-in, and Settings → Store accounts connects, reconnects and disconnects it.
- **Linking** (`ReleaseLinker`). An emulated release is linked to a RetroAchievements game only by a verified hash:
  - The release's ROMs are hashed by RetroAchievements' own rules (`retroachievements::hash`): the whole-file MD5, or without the NES/FDS, SNES, PC Engine, Lynx or 7800 header, an N64 ROM in big-endian order, or an arcade set's name.
  - Each hash must be one of exactly one game's hashes in `API_GetGameList` (`h=1`, cached a day per system), and all of a release's ROMs must be that same game.
  - Discs and other systems with other rules, compressed files, unknown hashes (another revision or region, a modified ROM), shared hashes and ROMs of different games stay `Unresolved`, with the reason. They are tried again after a week; a title is never used.
  - A link set by hand (`Manual`) is never replaced. Clearing it lets matching look again.
- **Refreshing the account** (Settings → Store accounts → Refresh, `Achievements::refresh_account`) links every release still due, then fetches the account's set of each linked game. It stops at the first wait, refused key or unreachable service, and that error is the account's.
- **Progress** comes from `API_GetGameInfoAndUserProgress`.
  - Softcore and hardcore unlocks are kept apart, from `DateEarned` and `DateEarnedHardcore` (UTC).
  - Rarity is unlocks over the game's distinct players; points are the achievement's points.
  - Badges come from `media.retroachievements.org`, with `_lock` for locked icons.
  - RetroAchievements has no hidden achievements.

Caskade only reads. Achievements are earned in an emulator core that supports RetroAchievements, signed in there.

`retroachievements::tests::live` (ignored) checks a real account: `CASKADE_RA_USER`, `CASKADE_RA_KEY`, optionally `CASKADE_RA_GAME`.

## Steam (#186)

`steam::SteamAchievements` reads with the Steam account's own sign-in (#166): its Web API key and SteamID64. A game's set is the account's for its app id, so discovery's installed copy and the account's owned copy (both carrying `ExternalId::Steam`) share one set.

- **Definitions.** `GetSchemaForGame` gives the name, description, `hidden`, and unlocked and locked icons. A game with no achievements in its schema (or no schema at all) is `no_achievements`, and the player isn't asked.
- **Player state.** `GetPlayerAchievements` gives `achieved` and `unlocktime`; seconds become milliseconds, and 0 means unknown.
  - A private profile's error ("Profile is not public") is `private`, never an empty list.
  - "Requested app has no stats" is `unsupported`.
  - A refused key means a new sign-in is needed.
- **Rarity** comes from `GetGlobalAchievementPercentagesForApp`, and is optional: a failure there leaves rarity out.
- Steam reports no progress or points here, so those stay unknown.

`steam::tests::live` (ignored) checks a real key: `CASKADE_STEAM_KEY`, `CASKADE_STEAM_ID`, optionally `CASKADE_STEAM_APP`.

## GOG (#187)

GOG publishes no achievement API of its own. `gog::GogAchievements` reads them the way GOG Galaxy does (as Heroic and Comet do too), with the GOG account's sign-in (#167):

1. **The game's Galaxy client.** The product's newest generation-2 build (`content-system.gog.com/products/<id>/os/windows|osx/builds`) links to a manifest (zlib JSON) naming `clientId` and `clientSecret`.
2. **A token for that client** from `auth.gog.com/token`, using the account's refresh token with `without_new_session=1`, so the account's own sign-in stays as it was.
3. **Definitions and unlocks together** from `gameplay.gog.com/clients/<client>/users/<user>/achievements`, page by page. `visible: false` means hidden, `date_unlocked` is the unlock time, and `rarity` is a percentage.

**Limits:**

- A product without a build, without a Galaxy client in its manifest, or without achievements at that client (404) is `unsupported`. It is never shown as "all locked".
- A failed later page leaves an incomplete answer, which keeps the rest as last fetched.
- An expired or missing sign-in (no refresh token) needs a new connection. Launcher credential files are never read.
- GOG gives no progress or points.
- These are Galaxy's service endpoints, not a published API, so GOG can change them.

`gog::tests::live` (ignored) checks a real sign-in: `CASKADE_GOG_REFRESH`, `CASKADE_GOG_USER`, `CASKADE_GOG_PRODUCT`.

## Epic Games Store (#188)

`epic::EpicAchievements` reads the store's GraphQL service (`store.epicgames.com/graphql`) as the store's own achievement pages and Playnite's SuccessStory do, with the Epic account's launcher token and `account_id` (#168).

- **Sets are per sandbox**, which is the namespace of `ExternalId::Epic`. Editions in one namespace share a set; games with the same title in other namespaces never do.
- **Definitions** come from `Achievement.productAchievementsRecordBySandbox(sandboxId, locale)`: the unlocked or locked name and description, `hidden`, icons, XP (kept as points) and rarity. The record also names the sandbox's product.
  - No record means the game uses no Epic achievements (`unsupported`).
  - An empty list means `no_achievements`.
- **The account's unlocks** come from `PlayerProfile.playerProfile(epicAccountId).productAchievements(productId)`: `unlocked` and `unlockDate`. An answer that isn't `…Success` (a private profile) is `private`, and a refused token needs a new sign-in.
- **Progress** is left unknown: Epic's `progress` field has no documented unit, so it isn't guessed.
- **Limit:** this GraphQL is the store's own service, not a published API, so Epic can change it.

`epic::tests::live` (ignored) checks a real sign-in: `CASKADE_EPIC_TOKEN`, `CASKADE_EPIC_ACCOUNT`, `CASKADE_EPIC_NAMESPACE`.

## In the app (#190)

A game page shows its sets in **Achievements** (`components/achievements/GameAchievements.tsx`, over `AchievementsApi` in `api/achievements.ts`; fixtures in `achievementsFixtures.ts`). The hero shows the counts across sets (`AchievementSummary`); pressing it moves to the panel.

- **Sets:** one at a time, chosen by service and account (`Steam · Ada`) when there are several. Nothing is joined: a game owned on two stores, or by two accounts, shows each set apart.
- **Counts and attribution:** unlocked of total (and hardcore unlocks and points where the service has them), the account, and when the set was last fetched.
- **States**, each with its own text:
  - `not_fetched`: refresh to see them.
  - `no_achievements`, `unsupported`.
  - `private`: make the details public at the service, then refresh.
  - `sign_in_needed`: Open store accounts.
  - Stale (`ready` with an error): the last answer with when it's from and why the refresh failed.
  - A service's wait (`retryAtMs`) says until when.
  - Loading and a failed read differ too.
- **Each achievement:** its icon, name, description, unlocked or locked, unlock time, progress, rarity and points. Each is shown only where the service gives it (`capabilities`). For RetroAchievements, the hardcore unlock and time show beside softcore, or "Softcore only".
- **Hidden achievements** read "Hidden achievement" with a Reveal button until unlocked; a switch reveals them all. A concealed one's icon is never loaded.
- **Icons** load through `game_achievement_icon` (`commands/achievement_icons.rs`). It takes an achievement of one of the game's own sets, never an address, and downloads its icon through the artwork cache as a 96 px WebP, only for rows near the screen. Without one, the row draws a trophy, a lock or a concealed mark.
- **Refresh achievements** fetches the game's sets (`game_achievements_refresh`), and Stop refreshing cancels it. `achievementsChanged` re-reads after any fetch, background ones included.
- **A pad** reaches every control and row: rows are focus stops labelled with their state, a concealed row's stop is Reveal, and focus stays on the row it revealed. Nothing needs hover. Styles come from theme tokens; the meters' motion follows reduced motion.
- **Notifications:** `achievementsUnlocked` items in the feed, one per set and batch of new unlocks, from `Event::AchievementsUnlocked` (`notifications/service.rs`). They open the game's page.

Settings → Store accounts → Achievements switches `core.achievements` (show, background refresh, notify new unlocks).
