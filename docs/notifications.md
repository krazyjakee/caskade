# Notifications

Activity has two tabs: **Jobs** (running downloads, installs and questions
waiting for an answer) and **Notifications**, a feed the core keeps in the
library database (`caskade_core::notifications`, migration
`0016_notifications`). The feed and its read state survive restarts and
switches between desktop and console mode. OS notifications are out of scope.

The bell in the top bar has two counts: running jobs (top, `job-count`,
`data-status="needs-user"` while one waits for an answer) and unread
notifications (bottom, `notification-count`). It opens Jobs while a job waits
for an answer, else Notifications while some are unread, else Jobs.

## Read state

- Showing the feed marks nothing read. An item is read when it is **opened**
  (its list, its game, the settings it is about), or marked read by hand, one
  item or all.
- Each change worth reading gives the item the feed's next revision as its
  `version`. Marking read sends the version that was shown, so an item that
  changed since (more games in the batch) stays unread. Mark all read sends the
  revision the page was read at, so items that changed after that stay unread.
- A stale mark never moves read state backwards. Mark unread always works.
- Repeating an event that changes nothing (a rescan, the same failure, the same
  update found again) never touches read state.

## Game additions

Every game insert, from any source (local and launcher scans, ROM folders,
RomM syncs, account imports, installs, web games, games added by hand), writes
a `game_additions` row in the **same transaction**, through a trigger on
`games`. An upsert that updates an existing game doesn't fire it, so rescans,
metadata updates and retries add nothing. The notifications service reads the
rows it hasn't seen into batches, in one transaction with marking them seen:

- At start, after library events (once a burst has settled for 2 s) and every
  minute. A failed pass never touches the import; the next pass catches up.
  Nothing depends on an in-memory event arriving.
- **Batches are hourly**: a batch opens at its first addition and takes every
  addition less than 60 minutes after it. The next addition opens the next
  batch. It shows as soon as its first game is read, and updates as games
  arrive. It stores its first and last addition, so its label stays the actual
  time range ("Oct 28, 2026, 14:05–14:50"), never "the last hour".
- A batch counts each game once (its members are keyed by game id) and shows
  the count per source ("RomM 6 · Steam 4"). A large first import is one item
  (or one per hour it takes).
- A game merged into another, removed or hidden before the service read it
  isn't counted. A game split off another (Separate) isn't an addition. A game
  without an entry yet waits 30 s for its entries, for its source.
- Opening a batch lists its games. Each links to its page; a game merged since
  links to the game it joined (members keep the entry they came with), and a
  removed one says it is no longer in the library.
- New games in a batch that was read make it unread again.

## Event / category matrix

| Category | Event | Severity | Grouping | Opens | Default | Needs action |
| --- | --- | --- | --- | --- | --- | --- |
| `gamesAdded` | A game new to the library (journal, not events) | info | hourly batch from the first addition (`games.added:<start>`) | the batch's games | on | no |
| `jobFailed` | An install, update, move, uninstall or check failed (`install.job.changed`, status failed) | error | one per job (`job.failed:<job>`); the same error again changes nothing, another error makes it unread again | the game's page (updates, moves, checks) or Jobs | on | yes |
| `jobFailed` resolved | The same job later completes or is cancelled (a retry worked) | — | resolves and reads the item | — | — | — |
| `updatesAvailable` | Installed games (`game_updates`) and managed emulators with a newer release on offer, read from the database on every pass | info | one item (`updates.available`); an update it didn't list makes it unread; one installed or skipped leaves quietly; none left resolves it | the list: each game's page, Settings → Emulation | on | optional |
| `updatesAvailable` (Caskade) | `appUpdateChanged` with a release on offer | info | one item (`app.update`), per version; resolved once up to date or installed | Settings → General | on | optional |
| `pluginDisabled` | `plugin.disabled`: a backend plugin that kept crashing was turned off | warning | one per plugin (`plugin.disabled:<id>`); the same reason again changes nothing | Settings → Plugins | on | yes |
| `shareArrived` | `share.arrived` (#247): a game someone shared with this device arrived and installed | info | one per receive job (`share.arrived:<job>`) | the game's page | on | no |
| `shareReceived` | `share.received` (#247): someone received everything of a game this device shared once | info | one per share and person (`share.received:<share>:<endpoint>`) | Settings → Devices (Copies and shares) | on | no |
| `achievementsUnlocked` | `achievements.unlocked` (#190): achievements unlocked since a set's last fetch. The core sends it only after the set's first complete fetch (its baseline) and once per unlock, so an import or a repeated refresh adds nothing | info | one per set and batch of unlocks (`achievements.unlocked:<set>:<names>`) | the game's page | on (`core.achievements.notifyUnlocks`) | no |

Routine progress is never notified: it stays in Jobs and the status bar.
Automatic recovery (#160) retries quietly; a failure is notified when the job
ends failed, once per job and error.

### Deferred (tracked follow-ups)

- **Jobs waiting for a decision** and recovery that needs input: shown in Jobs
  and by the bell's `needs-user` count, not as feed items.
- **Successful installs and update batches** ("3 games updated"): the Jobs tab
  shows completions; a summary item is a follow-up.
- **RomM / account sync failures, expired sign-ins and disconnected sources or
  storage roots**: need the RomM service and storage to report persistent
  failures (#160) before they can be notified and resolved.
- **Missing emulators or BIOS that block a launch**: the launch error and
  Settings → Emulation report them today.
- **Plugin failures short of disablement**, and resolving `pluginDisabled` when
  the plugin is turned back on.
- **Per-category switches** for the informational categories (game additions,
  updates), in Settings → General. Until then grouping is the noise control:
  one item per hour of additions, one item for every update on offer.
- **Per-account read state**: the feed and its read state are shared by every
  account.

## Retention

Read items untouched for 90 days are deleted on each pass. Unread items are
never deleted automatically, however old. Journal rows of games long gone are
deleted with them; a game still in the library keeps its row, so it is never
counted again.

## Interfaces

- Core: `Notifications::{list, unread, members, mark_read, mark_unread,
  mark_all_read, watch, refresh}`, storage in `LibraryStore` (`db/notifications.rs`).
- Commands (`src-tauri/src/commands/notifications.rs`, all open to every
  account): `notifications_list(unreadOnly, offset, limit)`,
  `notifications_unread`, `notification_members(id, offset, limit)`,
  `notification_mark_read(id, version)`, `notification_mark_unread(id)`,
  `notifications_mark_all_read(revision)`. The `notificationsChanged` event
  carries the unread count after every change.
- Webview: `src/api/notifications.ts` (hooks and `NotificationsProvider`),
  `views/activity/NotificationFeed.tsx`; anatomy roots `notifications` and
  `notification`.
