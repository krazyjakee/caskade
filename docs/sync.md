# Device connections and pairing

How a household's Caskade devices reach each other: the `caskade-sync` crate
(`crates/caskade-sync`), its identity, the opening every connection starts
with, and how to check it. Milestone: Devices, sync and sharing (#252).

## The crate

`caskade-sync` has no Tauri dependency and no database. It owns:

- `Identity`: this device's ed25519 key, in the OS keychain.
- `SyncEndpoint`: the iroh endpoint and router, held for the process lifetime.
  It speaks Caskade's ALPN `caskade/sync/1` and, when configured,
  `iroh_blobs::ALPN` on the same endpoint, key and socket.
- The opening (`opening`): what a connection is about and whether it's welcome.
- Framing (`frame`): one JSON object per line, each read with a bound.

The core decides who is admitted by implementing `Admission`, and takes
accepted connections through `Sessions`. Later work (sync, copies, shares)
builds on a `Session`: the authenticated peer, the purpose, and the opening's
stream left open for the purpose's own messages.

### Versions

iroh 1.0.3, iroh-blobs 0.103.1 and iroh-tickets 1.0.0: the set wobu proved
together (`wobu/docs/10-sync-spike.md`). n0 yanked iroh-blobs 0.103.0 on
2026-10-06 and released 0.103.1, which fixes `EventMask` ignoring
per-request modes; 0.103.1 is the same generation. `Cargo.lock` also holds
iroh-base, iroh-relay and iroh-dns at 1.0.3 and noq at 1.1.1, the versions
iroh 1.0.3 was built and run with. `cargo update` would otherwise move them to
the 1.4 line under iroh 1.0.3. A mismatched iroh/iroh-blobs pair fails at
resolve time with an `ed25519-dalek` error that never names iroh.

TLS goes through ring (iroh's default `tls-ring`), the provider the rest of
Caskade uses.

## Identity

The device key is a 32-byte ed25519 secret in the OS keychain, service
`dev.caskade.sync`, entry `device-key` (beside `dev.caskade.store-accounts`).
Its public half is the device's endpoint id: its name to every paired device
and its TLS certificate.

The secret is never written to the database, settings, logs, events, backups
or the webview. `Identity` has no `Serialize`, its `Debug` prints only the
public id, and the key leaves it only into iroh's endpoint builder.

`Identity::load(store, known)` reads it:

| Keychain | `known` (the id this device had) | Result |
| --- | --- | --- |
| A key | any | `Ready`, the stored identity |
| No entry | none: first use | `Ready { created: true }`, a new key, stored |
| No entry | some | `NeedsConfirmation { Missing }` |
| Not a key | any | `NeedsConfirmation { Unreadable }` |
| No answer within 20 s, or refused | any | `Unavailable`: sync waits, nothing is created |

A new identity is a new device to everything paired with this one: they
refuse it until it pairs again. So after first use it's made only by
`Identity::create`, once the user has agreed. `create` fails rather than
returning an identity it couldn't store, which would be a new device on every
start.

Each keychain call runs on a thread of its own with a deadline. On Linux a
locked keychain can block a read forever, waiting for an unlock prompt nothing
draws. A call that misses the deadline is abandoned, not joined, and the app
loads the identity off the main thread, so the window comes up regardless and
only sync waits. It isn't `spawn_blocking`, because the runtime waits for
those at shutdown.

## The opening

The dialler opens one bidirectional stream and writes one line:

```json
{"purpose":"sync","grant":"<64 hex characters>"}
```

`purpose` is `sync`, `copy`, `share`, `pair` or `forget`. `grant` is what the
dialler holds: the grant the other device gave it when they paired, or the
single-use grant from a pairing link. A grant is 32 random bytes. It's a
capability, not a key: nothing derives, signs or encrypts with it, it never
appears in `Debug` output, and it's compared in constant time.

The accepting device asks its `Admission` once with the authenticated peer id
(`Connection::remote_id`, proved by TLS), the purpose and the grant, and gets
back only yes or no. It answers with one line:

- `{"answer":"accepted"}`: the stream stays open for the purpose's messages.
- `{"answer":"refused"}` (`REFUSAL`), then a QUIC close with code 1 and reason
  `refused`.

Every refusal is those bytes and that close, whatever the reason: an unknown
device, a missing, wrong or used grant, a purpose the grant doesn't open, a
forgotten device, an unknown purpose, a line that isn't JSON or is too long,
or a dialler too slow to finish its opening. A peer learns that it was
refused and nothing about what this device holds. The dialler reports it as
`Error::Refused`, which carries no fields.

There's no key exchange, signature or challenge in application code. TLS 1.3
on the endpoint keys already encrypts the connection and authenticates both
sides.

### Framing

Every message is one JSON object on one line. The reader takes a bound per
message (256 bytes for the opening) and fails as `Error::Malformed` once a
line grows past it, before buffering more. No length a peer wrote is trusted.
Errors never quote what a peer sent.

### Timeouts

Each step of the opening (accepting the stream, reading the offer, writing the
answer, the dial itself) has `Config::open_timeout` (10 s): a peer that
connects and says nothing can't hold a task open, and one that stalls can't
hang the dialler.

## Reaching devices

Devices on one network find and reach each other directly, with or without
internet. Away from home they connect directly where routers allow, and
through a relay only when relays are allowed and nothing else works.

`Reach` says what an endpoint has. `Reach::network(never_relay, port)` is what
the app uses:

- **mDNS on the local network**, under Caskade's own service name (`caskade`),
  so only Caskade devices answer. iroh 1.0 moved mDNS into
  `iroh-mdns-address-lookup`; 0.4.0 pairs with iroh 1.0.3 (released with iroh
  1.0.0, and the version iroh-blobs 0.103 tests against). It's an address
  lookup on the endpoint, so dialling a nearby device by its id alone finds its
  local addresses. The announcement carries the device's name, kind and OS
  (`DeviceInfo`) for the nearby list (`SyncEndpoint::nearby`,
  `Lan::subscribe`). That goes only into the mDNS record: the endpoint's own
  user data stays empty, so n0's DNS never sees a device name.
- **n0's address lookup** (DNS/pkarr) through the `N0` preset: this endpoint's
  addresses, and other devices' over the internet.
- **n0's relays** only when Never use relays is off. On, it's the
  `N0DisableRelay` preset: the endpoint has no relay transport at all, direct
  paths over the internet still work, and a device only a relay could reach
  stays offline.
- **The default portmapper** (UPnP / NAT-PMP) for more direct paths.
- **A fixed UDP port** (`port`), for firewall rules; Windows asks to allow
  incoming UDP the first time. `None` lets the OS pick.

There is no DHT.

When multicast is blocked (guest Wi-Fi, client isolation), a pairing link or
a typed address still works: it carries the addresses.

### Addresses for links

`SyncEndpoint::shareable_addr(wait)` gives every local address, plus the relay
once there is one. It waits at most `wait` and never for the internet, unlike
`Endpoint::online`, which waits for a relay that never comes offline. A link
minted before the relay answered has local addresses only; mint again to
include the relay.

### Link state

A connection holds several paths at once (a relay path and direct ones) and
selects one to carry data. A device's link state comes from the selected path
of its best open connection:

| State | Selected path |
| --- | --- |
| `lan` | An IP address on the local network: private, link-local or loopback; announced by the device over mDNS here; or an IPv6 address in the same /64 as one of this device's own |
| `direct` | Any other IP address |
| `relay` | Through a relay |
| `offline` | No open connection |

`Links` follows every session's connection through its path events and sends
a `LinkChanged` whenever a device's state changes, including a connection that
started through a relay switching to a direct path once holepunching succeeds.
Relays carry data at roughly 1 MB/s (wobu measured 512 KiB in 504 ms against
22 ms direct), which is why game files require a direct path (#233).

## Pairing and devices

`caskade_core::devices::Devices` (only with the core's `net` feature) keeps
this device and the devices paired with it. Commands are in
`src-tauri/src/commands/sync.rs` (`devices_*`); the status reaches the webview
as `DevicesChanged`. Settings → Devices itself is #231.

### This device

Its endpoint id, name and kind are in the settings table under `sync.device`
(not a section). The name defaults to the host name. Name and kind go into
the mDNS announcement and are exchanged while pairing. `core.sync` holds Never
use relays (`neverRelay`) and the listening port (`port`) next to #227's
category choices; changing either rebinds the endpoint. The app starts the
service in the background (`Devices::start`): the identity loads off the main
thread, so a locked keychain holds only sync. `IdentityStatus` reports
`missing` or `unreadable` when the key is gone after first use, and
`devices_create_identity` makes a new one once the user agrees.

### Pairing by code

On the local network, one device starts a pairing with a nearby one
(`devices_pair_nearby`). Both show a 6-digit code, and the pairing completes
only when both users confirm (`devices_confirm_pairing`). The code is SHA-256
over both endpoint ids and a fresh 32-byte nonce from each side, reduced to six
digits. The dialler commits to its nonce (a SHA-256 commitment) before it sees
the other's, so a device in the middle can't choose nonces to make two
pairings show the same code: it would show each side a different one, except
by a one-in-a-million chance per attempt.

```text
dialler                                   acceptor
  Commit { sha256(nonce_d), device } ──▶
                                      ◀── Nonce { nonce_a, device }
  Reveal { nonce_d }                 ──▶   (checks the commitment)
  both show the code
  Confirm { grant } or Cancel        ◀─▶   Confirm { grant } or Cancel
```

A device takes one code pairing at a time: while one is in progress, the
admission refuses every other pairing without a grant, so a third device
can't join it. A cancel on either side, a mismatch, a two-minute timeout or a
dropped connection ends it with nothing stored.

### Pairing links

`devices_pairing_link` mints `caskade://pair?t=<ticket>`. The ticket is an
iroh ticket (kind `caskadepair`) holding this device's address (local
addresses, plus the relay once there is one), a single-use 32-byte pairing
grant and its expiry. A link works once, for ten minutes. The minting device
keeps the grant in memory and spends it in the opening, whether the pairing
then completes or not. A restart invalidates links not used yet.

The receiving device refuses a link that says it has expired without
dialling ("this pairing link has expired"). A used link, or one whose expiry
the minting device has passed, gets the opening's one refusal, which the
receiving device reports as "this pairing link was already used or has
expired; make a new one on the other device".

A pasted link goes to `devices_accept_pairing_link`. A `caskade://pair` link
opened from outside (`deep_link.rs`) doesn't pair by itself, because any web
page can open one. The adapter holds it, announces `PairingLinkOpened` (with
the device id and expiry, not the link), and pairs only when an admin calls
`devices_accept_opened_link`.

### Grants and admission

Each side generates the grant the other presents when it dials this one, and
receives the one it presents in return. `paired_devices` (migration 0031)
keeps both, with the device's name, kind, OS, last addresses, Wake-on-LAN MAC
addresses (#234), last sync time and when it was paired. Grants are
credentials: the table is local in the sync policy (#227), left out of backups
(`backup::policy`), and no grant or key is in anything sent to the webview.

The admission (`devices/gate.rs`) is in memory:

| Purpose | Admitted when |
| --- | --- |
| `sync`, `copy`, `forget` | The device is paired and presents its own grant |
| `pair` with a grant | The grant is a pairing link offered here, unused and unexpired (spent now) |
| `pair` without a grant | No other code pairing is in progress |
| `share` | Never yet (#246) |

`Devices::connect(device_id, purpose)` opens a connection to a paired device
with its grant: what library sync (#230) and copies (#233) start from.

### Forgetting

`devices_forget` drops the device's grant at once, so its connections are
refused from then on, and tells it with a `forget` connection, which makes
it drop this one too. A device that can't be reached stays in the table,
marked forgotten and refused, and is told when it can be: as soon as it tries
to connect, when it appears nearby, or every five minutes. Nothing synced is
deleted. A forgotten device reconnects only by pairing again.

### Admins only

Pairing hands another device every account and the library. So pairing,
confirming, cancelling, minting or accepting links, forgetting, renaming this
device and making a new identity are admin-only in the core
(`AccountService::require(Permission::Admin)`), and in the adapter's
permission table too. Reading the status needs a signed-in account.

## The change log

`caskade_core::sync` (#228) records every change to synced data with the
device that made it and a hybrid logical clock, deletions included, so
devices can tell each other exactly what they haven't seen and agree on the
result whatever order changes arrive in. Migration 0032 holds it; all of it
is local (sync policy).

### What a change is

`sync::change::Change`: an origin (the device's endpoint id), a clock, an
`Entity`, a key in the origin's ids, a field and a value (`None` is a
tombstone). The field `""` is a row's existence. The entities are the
portable forms of what `sync::policy` declares synced:

| Entity | Key | Fields | Category |
| --- | --- | --- | --- |
| `game` | game | `identity`, `favorite`, `hidden`, each user value (`title`, `description`, …; lists as `tags` plus `tags/<item>`) | Library |
| `keptApart` | game | `""` | Library |
| `igdbMatch` | game | `igdb` (an id, or `"none"`), hand-set only | Library |
| `artworkChoice` | game\|kind | `""` | Artwork |
| `collection` | collection | `""`, `name`, `description`, `pinned`, `rule` (kind and query), `account` | Library |
| `member` | collection\|game | `""`, `position` | Library |
| `session` | session (UUIDv7) | `""`: game, start, end and duration, once finished | PlaySessions |
| `account` | account | `""`, `name`, `avatar`, `role`, `pinHash`, `permissions`, `maxAge`, `unratedNeedsPin`, `createdAt` | Accounts |
| `approval` | account\|game | `""` | Accounts |
| `rommServer` | server id | `""`: its shared definition, as the policy projects `core.romm` | RommServers |
| `saveVersion` | version (UUIDv7) | `""`: game, account, device, session, time, size, file count, parents and the hash of its file list | Saves |
| `artwork` | game\|artwork id | `""`: the user's own image's kind, content hash and size; its file follows as a blob | Artwork |
| `gameScript` | game\|script id | `""`: a shell script's name, phase, shell, text, time limit and place | GameScripts |
| `action` | game\|release key\|action id | `""`: name, kind, tracking mode, place and a target bound on each device (a Flatpak's app id, an emulator's platform) | LaunchActions |
| `actionSettings` | game\|release key | `defaultAction`, `ask`, `builtInName`, `builtInTracking`, each when not the default | LaunchActions |
| `achievementLink` | game\|release key\|provider | `""`: the product, linked by hand | AchievementLinks |
| `addon` | device\|add-on id | `""`: an installed plugin or catalogue add-on and its version; `enabled` for a plugin | Addons |
| `copy` | device\|game\|entry id | `""`: an installed copy's release key, size, build OS, location kind (`managed`, `external`, `launcher`) and whether its folder can be copied | Library |
| `preferredEmulator` | platform | `definition`: the emulator definition its default uses | PreferredEmulators |
| `appearance` | `core.appearance` | each field changed from its default (`theme`, `density`, …) | Appearance |
| `accountSettings` | `core.accounts` | `ratingBoard`, when not the default | Accounts |

A game's user values are what `overrides_json` marks as the user's, read from
the shown record: the policy's `Receive::GameEdits` contract. Automatic
metadata never enters the log, and applying a user value puts it on top of
the automatic one (a reset brings the automatic value back), so refreshes keep
working beneath it. Lists are sets: each item is a field of its own, so two
devices adding tags concurrently keep both. The PIN throttle and the active
account never sync. A RomM server's definition travels without its password
or token: it's applied to `core.romm` through `SettingsService`, keeping each
device's own emulator and profile per platform, sync schedule and cache. A
device that gets a new server has no secret for it, so it asks for its own
(`ServerStatus::has_secret`). A server removed on another device goes here
through `RommService::remove_server`, with its downloaded ROMs. Sessions are a union: one is never deleted by sync, and a
session whose game has no entry here gets an unlaunchable `sync:sessions`
entry.

The user's own artwork (`user`-source rows: an imported image, or an
automatic one they chose) travels as `artwork` changes, and its file as an
`artwork` blob after the changes (`sync::artwork`, over `sync::blobs`). The
change waits until its file is in the cache here, so a game never shows an
image that isn't there; the blobs that arrived are applied at the end of the
exchange. A device serves only files its own user artwork uses: its cache's
full-size image, which the receiver decodes within the import limits and keeps
under the same hash (`ArtworkCache::import_received`; it can't be checked
against the hash, which is the original file's). Artwork may go through a
relay, as save files may.

Scripts, actions and achievement links belong to a release, whose entry has
an id of its own on each device, so they're keyed by a *release key*
(`db::sync_carried`) that names the same release everywhere: one of its store
ids, its RomM ROM (RomM servers keep their ids between devices), or `only` for
a game's only release. A release with none keeps them on its device. What
names a place on one device stays there (`Receive::BindLaunchLocally`): an
action's program, folders, arguments and environment, a tracking folder or
process names. So only actions that can be bound on the other device travel:
a Flatpak's app id (without arguments), and an emulator action, which takes
that release's own emulator launch on the same platform; one that can't be
bound yet waits. Only shell scripts travel (a direct command names a program
on its device), and they arrive turned off (`Receive::DisableGameScript`); one
whose command changes on another device is turned off again. A shell this
system hasn't leaves the script waiting. A Flatpak action whose app isn't
installed here is refused before anything starts, naming the app (#245,
`launch::command::flatpak_missing`), rather than failing inside `flatpak run`. A ROM hash this device verified
stays over a hand-made link from another device.

Add-ons travel as each device's own inventory, keyed by the device: the
plugins it has (and whether each is on) and what it installed from the
catalogue, with versions. Each device sends only its own and never removes
another's, so two devices with different versions never overwrite each other.
They're offers: nothing installs or turns on by itself, since a plugin's
permissions are granted on each device. Theme folders copied in by hand
aren't listed. Settings → Plugins' catalogue offers them under On your other
devices (#245, `sync::offers::addon_offers`, `addons_from_devices`): what a
paired device has and this one lacks, while Plugins and themes syncs here.
Each opens its details, where installing a plugin goes through its review; one
that isn't in the catalogue is named, with nothing to install. A preferred emulator is a platform's default emulator
definition; it becomes this device's default only where this device has an
emulator of that definition (with its first profile), and waits otherwise.
Another device clearing its default clears nothing here.

Settings travel field by field (`sync::settings`), and only fields set away
from their defaults: a device set up later would otherwise replace the
household's choices with its own newer defaults. A field set back to its
default leaves the snapshot, and its tombstone sets it back to the default on
the other devices. They're applied through `SettingsService`, so validation
and change events run; a value this device's schema refuses is left out.

`sync::change`'s test `every_synced_or_optional_rule_has_an_entity` fails
when a policy rule with a synced or optional category has nothing carrying
it (`Entity::carries`, or a blob kind).

### Clocks

`sync::clock::Hlc` is wall milliseconds plus a counter. A device's clock
never goes backwards: a reading is past the highest it has made or seen, and
that highest is on disk (the version vector, `sync_seen`), so a restart or a
clock set back carries on from it. Seeing another device's change moves the
clock past it, so an edit made after receiving one always beats it, even with
the system clock behind. Changes order by clock, then device id, so every
device picks the same winner.

### Capture and apply

Capture (`SyncEngine::capture`) compares a snapshot of every synced field here
(`LibraryStore::sync_snapshot`) with the winners recorded in `sync_fields`, and
records each difference as a change with a new clock reading; a value that's
gone becomes a tombstone. It doesn't matter what wrote the value (an editor, a
bulk edit, a merge): only what the snapshot holds can be captured, and that
excludes automatic values. A game removed here isn't a game deleted
everywhere, so its fields send no tombstones, and neither do sessions.

Apply (`SyncEngine::receive`, `apply_pending`) maps a change's keys to this
device's ids, then writes it in one transaction with its field's new winner,
if its stamp beats the winner here. A change waits, unapplied, while its
category is This device only here or what it refers to hasn't arrived (a game
whose identity is still on the way, an account a collection belongs to).
Turning the category back on applies the waiting changes from the log, and
the next exchange sends that category to each device again in full.

A row's tombstone is a value like any other: a device that never saw the
deletion can't bring the row back with an older change, and a field written to
a deleted row is ignored.

### The same game on different devices

Each device gives its games its own ids, so a change naming a game is mapped
to the game here before it applies (#229). Every game's changes include its
identity:
- its automatic title, platforms and release year;
- its IGDB id;
- the store ids of its entries (`ExternalId`, keyed as store accounts do, so
  Epic matches on namespace and catalog item);
- the other devices' games already matched to it.

Non-Steam shortcuts' ids come from local paths, and manual entries' ids
belong to one machine, so neither counts.

The first change from another device's game resolves it, in order:
1. A game here it's already known as (an id in its list, or this device's own).
2. Its IGDB id.
3. A store id one of the game's entries shares.
4. Title, platform and year.
5. Otherwise, a new title-only game with that title, platforms and year. Its
   collections and play history stay whole, and it gains an entry when the
   game is installed here (#232, #233).

The match is remembered in `sync_game_ids`. A rename changes the shown title,
not the identity's automatic one, so it never breaks a match. When a merge here
removes the matched game, the next change matches again.

A game kept apart on another device is separated here too. When it matches a
game here that also has entries whose store ids it doesn't have, those entries
of its own move into a new game kept apart, as Separate does. Merging doesn't
need a change: each device merges the same games by its own rules, and kept
apart stops both.

A session needs an entry: it goes to the game's own entry here, or to an
unlaunchable `sync:sessions` entry when the game has none.

### Compaction

`SyncEngine::compact` drops a change once every paired device has seen
whatever replaced it, and drops a tombstone once every paired device has seen
it (`sync_fields` still holds it, so an older change stays beaten). The newest
identity each device sent per game stays. What remains is what some device
hasn't seen and the live values, so a device paired later still gets the
whole current state.

### Backups

Backups keep the log, the winners and the version vector, but leave out what
other devices said they have (`sync_peers`). A restored library carries on
from its own clocks; on its next connection it receives everything it lacks,
its own later changes included, and nothing it restored is stamped as new.

## Syncing in the background

`caskade_core::sync::SyncService` (#230, `net` feature) keeps paired devices in
step on its own. The app starts it beside `Devices`, which hands it the `sync`
sessions paired devices open.

### The exchange

On a `sync` connection (the opening checked the grant), both sides run the
same exchange (`sync::exchange`):

```text
Hello { seen }      ◀─▶  Hello { seen }      what each has: its version vector
Page { changes } …  ◀─▶  Page { changes } …  what the other hasn't seen, 200 per page
Done                ◀─▶  Done
Ack { seen }        ◀─▶  Ack { seen }        what each has after applying
```

Both directions run at once. Each page is imported and applied before the next
is read, one transaction per change, and a capture keeps each row's changes
together (existence first), so a page boundary splits at most one row. A
connection that drops mid-exchange leaves both libraries consistent: every
change applied entirely or not at all, and the version vector says exactly
what arrived. The next exchange starts from there. `Ack` ends the exchange only
when both sides have applied everything, and is what compaction counts on
(`sync_peers`). Lines are read with an 8 MiB bound.

Categories set to This device only aren't sent. One turned back on is sent to
each device again in full on the next exchange. The receiver takes what it
lacks and ignores the rest, and the changes that waited here apply at once.

A synced settings section can also be This device only on its own while its
category syncs (#244, `core.sync.sections`): Appearance (`core.appearance`),
the rating board (`core.accounts`) and RomM servers (`core.romm`), each from a
Same on your other devices switch in the section itself
(`views/settings/devices/SectionSync.tsx`, admins only, shown once a device is
paired). The engine gates each change by `SyncSettings::allows_entity`: the
section's entity is held back and waits as an off category's would, and the
rest of its category keeps syncing. Turning the section back on counts its
category as turned on (`allows_whole`), so it's replayed in full. The
interface scale (`uiScale`) never syncs: it suits each screen.

### Kinds of device

`caskade_core::device_type` (#244) tells a desktop, laptop, handheld or TV
apart: Linux's DMI names known handhelds (Steam Deck, ROG Ally, Legion Go,
AYANEO, …) and laptop chassis, and a session inside gamescope is a handheld
with a battery or a TV without one. Anything else is a desktop. A device's
record starts with what was detected, and Settings → Devices changes it.

A new install (first-run setup never saved, no games) starts with its kind's
first settings, written only into sections never saved: a handheld or a TV
starts in console mode (`core.console.active`), with emulators fullscreen in
desktop mode too (`core.emulation.desktopFullscreen`) and a larger interface
(`uiScale` 115 or 125). A desktop or laptop keeps the defaults. An existing
library never changes.

A blob kind may add a note to its first round's wants (`BlobKind::announce`
and `heard`, an optional field older builds ignore). Both sides then run one
more round, so what a note offers is asked for in the same exchange. Backups
kept on a paired device (#243, `backup::peers`, see the backup README) use it:
the chosen holder is offered new backups, and every device lists what it holds
for a fresh install to restore from.

### When it runs

- Every 2 seconds it captures this device's changes, and exchanges with every
  paired device when there are any, so a change reaches a device on the same
  network within seconds.
- It also exchanges when a paired device is found on this network (mDNS), and
  whenever a paired device connects.
- Otherwise it polls each device a minute after a sync, backing off from 30
  seconds to 5 minutes while one can't be reached.
- The log is compacted every 10 minutes.

When both devices dial at once, both exchanges run; that's safe, because every
change applies in its own transaction. An unreachable device is a normal state,
never a toast. Each device's state (`syncing`, `lastSyncMs`, `syncError`) is in
`DevicesStatus` and reaches the webview as `DevicesChanged`.
`devices_sync_now` syncs with one device at once. Sync runs on background
tasks with short transactions, so the library and game launches aren't held
up.

### Not carried yet

Everything the policy declares synced or optional travels.

### Copies on other devices

Each device announces its installed copies of synced games (#232,
`db::sync_copies`, policy resource `installed_copies`): the release key, size,
the build's OS (a Windows build, or this system's own), where it lives (a
Caskade-managed folder, a folder of its own, or a launcher's: an external
install that starts through a URI or Flatpak) and whether its folder can be
copied (not a launcher's). Never a path: the entry id is an opaque UUID. Like
add-ons they're keyed by the device, which alone removes them.

Another device's copy is an entry here from source `peer:<device id>`,
uninstalled and not launchable, like a store account's owned entry; its
details stay in the log's field. Uninstalled there, its tombstone removes the
entry here on the next sync, and forgetting the device removes all of its.
The Library's Ready to Install tab and its count include copies that could
come here: copyable, and a build this system runs (a Windows build runs on
Linux through Proton). `sync::copies::peer_copies` (command
`game_other_devices`) adds each device's name, kind, OS and link, whether the
game is running there now (`sync::presence`), and why a copy can't come here
(`launcherOnly`, `otherSystem`).

The game page's On your other devices panel (`views/game/OtherDevices.tsx`)
and the game menu's Your devices section show them; peer entries stay out of
the page's releases. Both only tell for now: copying here is #233, and waking
a device or playing there is #234.

### Copying a game from another device

Copy to this computer (#233, the On your other devices panel; command
`game_copy_here`) brings a game from a device that has it as a
Caskade-managed install here: an `InstallService` job of kind `peerCopy`, in
Activity, cancellable, retried from where it stopped (`install::peer`).

1. The receiver opens a `copy` session (a paired device's grant) and asks for
   the release by its entry id there (`transfer`).
2. The sender finds the release's folder: a managed install, or a folder of its
   own with a program or emulator launch. A launcher's install is refused with
   the reason ("install it from the store there"). It adds every file,
   referenced where it is, to an iroh-blobs collection named by relative paths
   (`caskade_sync::copy::CopyStore`, `<cache>/copies/send`; only BLAKE3
   outboards are stored), saying how far it got, and offers the root hash, size,
   title, the program relative to the folder and whether it's a Windows build.
   Never a path or arguments. The offer is kept for six hours, so a copy that
   comes back resumes without the files being read again; while it's being
   sent, the release can't be moved or uninstalled.
3. Its iroh-blobs provider serves only paired devices it offered something,
   and only those collections (`CopyGate`: connections and get requests are
   intercepted; get-many, push and observe are refused).
4. The receiver checks there's room (what's still to come in the cache, and
   the install in the root: twice the game on one disk), then pulls the
   collection over a `DirectDialer`: a second endpoint with this device's key
   and no relay transport, to the sender's direct addresses. Game files can't
   pass through a relay. While only a relay reaches the sender, or it can't be
   reached, the job waits and tries again ("Only a relay reaches …"), and what
   arrived stays verified in `<cache>/copies/receive/store`.
5. The files are exported to `<cache>/copies/receive/incoming/<job>/` (the
   portable installer takes a folder outside the storage root) and installed
   from there like a dropped game folder, with the sender's title and program
   as hints: a managed install with a `caskade-install` id of its own, on the
   game that was copied, its program relative to its folder and its runtime
   as the files say (a Windows build runs through umu/Proton on Linux). The
   staging folder goes afterwards, whatever the Installer's source setting
   says.

From the device that has it, a game's menu offers **Copy to …** (#235,
`game_send_to`) for each paired device reached directly (`lan` or `direct`)
that has no copy of it, when this device has it installed in a folder that
can be copied. The device asks the other on a `copy` session (`Send`: the
release, the game's id there, title and size); the other answers once it
took the request (`Queued`). There it's an install request of kind `peer` in
the confirmation queue (`InstallRequests::queue_peer`), naming the sending
device, and nothing is written until it's confirmed there, in the storage
root picked there: confirming starts the copy above from that end. With
`core.sync.acceptCopies` on (Settings → Devices → Accept copies from my
devices; off by default, kept on each device), it starts at once into the
default storage.

Settings → Devices lists copies going on both ways (`devices_transfers`,
refreshed every two seconds while there are any). On loopback, a release build
moves 256 MiB in 0.62 s (432 MB/s; the offer took 0.19 s):
`cargo test --release -p caskade-sync --test it copy::loopback_throughput --
--ignored --nocapture`. A relay moves about 1 MB/s.

### The default admin

Each library starts with a default admin, "Player" (migration 0014). Until
someone changes it (a name, an avatar, a PIN, an age limit), it stands for
whoever set the device up, and every device's stand-in is the same household
(`db::sync_accounts`):
- it never travels as an account, only as a marker (`{"placeholder": true}`),
  so no device shows a second "Player";
- what it owns that travels (a collection, a save version's account) goes to
  the other device's household admin: that device's own stand-in, or else its
  oldest admin. The engine maps it before applying, so the field holds this
  device's id and nothing echoes back;
- a device's stand-in joins the first admin that arrives from another device
  (`LibraryStore::sync_join_stand_in`, after an apply): its collections,
  approvals, store-account ownership and save versions and heads move to that
  admin, and it's removed. Whoever was signed in as it is signed in as that
  admin (`AccountService::joined`), unless the admin has a PIN, when nobody
  is;
- a row a device already had for another's stand-in (from before stand-ins
  stayed home) hands what it owns to the household admin and goes.

Once changed, a stand-in is an account like any other and travels as one.

## Saves

`caskade_core::saves::versions` (#239, migration 0033) keeps a game's saves
as versions and carries them to paired devices (category Saves, synced by
default).

### Versions

- **A version** is a release's save files (#238's resolved locations) at one
  moment. Its file list (each file by named folder, relative path, SHA-256
  and size) is itself a blob, and the version's `content_hash` is that list's
  hash, so equal saves have equal hashes. Files are kept once by content under
  `<data>/saves/blobs/`, so versions share what didn't change.
- **Portable paths.** Each file is stored relative to the deepest named folder
  holding it: the manifest's placeholders (`<winAppData>`, `<winDocuments>`,
  `<home>`, …), or an emulator definition's variables (`{save_data}`, …). On
  Linux, Windows placeholders are folders inside the release's Proton or Wine
  prefix, so a save made on Windows restores into the prefix and back. A path
  under no named folder stays absolute and restores only where that path
  exists. A relative path never leaves its folder (`..` is refused).
- **When.** After every session that exited, crashed or was interrupted (a
  `launch.after` interceptor, hashing in the background so the session's end
  isn't held up), and before each launch when the files changed outside
  Caskade (`found`). Nothing is kept when the files are what this device's
  head already holds. Each version belongs to the account that played.
- **History.** A version names its parents: the version this device's files
  matched when it was made (its *head*, per game and account, in `save_heads`,
  which never syncs). The versions nothing descends from are the *tips*.
- **Keeping.** Each device keeps `core.saves.keep` versions per game (20 by
  default), never pruning a tip or a head, and deletes the files nothing kept
  names. Sync never deletes a version: each device prunes its own.

### Before a launch

`SaveVersions::before_play` (Play through `game_saves_before_play`, and
`caskade --launch`) asks every paired device in reach to sync and waits for
them for 4 seconds at most, then:

- keeps the files here as a version when they changed;
- **one tip, another device's:** puts its files in place and makes it the head
  (`restored`); when its files haven't arrived, the game starts with the save
  here (`notArrived`);
- **more than one tip:** both devices played since they last agreed. Nothing
  is restored, and Play asks which save to use (`SaveConflictDialog`). The
  choice becomes a new version descending from every tip, so it settles the
  conflict on every device; the others stay as versions. `caskade --launch`
  never asks: it starts with the save here and leaves the question to Play.
- `checked: false` when a paired device couldn't be reached in time: Play
  starts anyway and a note says a newer save may be waiting there.

A failed check never stops a launch. Restoring writes each file atomically and
leaves files the version doesn't list.

### Between devices

Versions made here travel as `saveVersion` records in the change log. Their
files follow on the same granted `sync` session after both `Ack`s
(`sync::blobs`): each side asks for the blobs its records name and it lacks (a
file list first, then its files), and the other sends them in 512 KiB chunks.
Each blob is checked against its hash before it's kept. Rounds repeat while
anything moved, up to four. With Saves set to This device only, neither the
records nor the files leave this device, and none are asked for. iroh-blobs'
own protocol isn't used: it serves any blob to anyone who knows its hash, with
no grant.

Save files ride the sync session, so they go through a relay only when relays
are allowed and nothing direct works; Never use relays keeps them direct. Game
files never go through a relay.

### On the game page and in Settings

- **Saves panel** (#241, `views/game/SavesPanel.tsx`, below Play from your
  library): every version, newest first, with its time, the device and
  account that made it, why it was made and its size. The version this
  device's files match is marked In use; one whose files haven't arrived
  can't be restored yet. **Restore** (`game_saves_restore`) puts an older
  version in place for the release Play starts: what's there is kept as a
  version first, the restored copy becomes a new version, and playing carries
  on from it. Restoring waits until the game has closed. **Show in folder**
  (`game_saves_reveal`, desktop mode only; it takes the entry and the
  location's place in the list, never a path) opens each save location here.
  When both devices played, **Choose a save…** asks the same question Play
  does.
- **Settings → Devices → Saves** (`views/settings/devices/SavesSection.tsx`):
  Sync saves (the Saves category of `core.sync`), Versions to keep
  (`core.saves.keep`), and the save-locations manifest's last check with
  **Update now** (`saves_manifest_status`, `saves_manifest_update`:
  `Ludusavi::update_now` checks the configured source whatever the daily
  check said, and a failure keeps the cached manifest).
- The panel follows `SaveVersionsChanged` (versions made or restored here)
  and `DevicesChanged` (a sync brought some). Everything is reachable with a
  pad in console mode (`test/saveFlows.test.tsx`).

### Playing on another device

`caskade_core::sync::presence` (#240) tells paired devices which games are
running where. It's presence, not stored data: nothing is logged or kept on
disk.

- **The frame.** Each `sync` exchange ends, after the blobs, with both sides
  sending a `Presence` frame (`version` 1): every open play session's game
  in the sender's ids, the account playing and the start time. Games are
  mapped through `SyncEngine::game`; one this device doesn't know yet is
  skipped. A peer one build behind sends no frame: the exchange still ends
  cleanly and that device shows nothing running. With Play history set to
  This device only, presence is neither sent nor taken in.
- **Within seconds.** A game starting or ending here (`GameStarted`,
  `GameExited`) asks every paired device to sync at once (`Presence::watch`).
- **Never stale for long.** A device's presence counts while it was received
  within 3 minutes (sync polls each device a minute after the last
  exchange), its link isn't offline, and its last sync didn't fail. So a
  device that crashed or left stops showing a game running.
- **Never blocks.** `devices_playing` and Play's check read only what's in
  memory: no network wait, whether the other device can be reached or not.
- **UI.** Play warns when the game is running on another device
  (`views/game/PlayingElsewhere.tsx`): Cancel, or Play anyway, after which
  each device keeps its own save and the next Play asks which to use (#239).
  When the other device's session has just ended and its save's files are
  still on their way, Play waits for them (looking every 2 seconds, up to a
  minute) with Skip. "Playing on …" shows on the game page, on the game's
  card in console mode (`data-part="playing-elsewhere"`) and in the device's
  row in Settings → Devices. `PlayingElsewhereChanged` and `DevicesChanged`
  keep them current. Play on that device comes with #234.

### Limits

- Registry-based saves (the manifest's `registry` entries) aren't kept as
  versions; only files are.
- Backups keep the versions' records but not their files (saves are never in a
  backup). After a restore, files a paired device still has come back on the
  next sync.

### Checks

Core tests run two in-process cores on loopback (`tests/save_sync.rs`): a
Windows device (a fixture machine) and a Linux device playing through a Proton
prefix. The tests cover a save made on one starting the game on the other and
back, both devices playing offline being asked and keeping both, Saves set to
This device only, and an unreachable device. `tests/save_versions.rs` covers
one device: versions, restoring an older one, pruning, and the after-session
snapshot. Not yet checked on real machines:

| Check | Result |
| --- | --- |
| Play on Windows, quit, start on a Linux device through Proton: the Windows save is there | not yet run |
| Play on the Linux device, quit, start on Windows: the Linux save is there | not yet run |

## Settings → Devices

`views/settings/devices/` (#231), over `api/sync.ts` (the native API on the
`devices_*` commands and `DevicesChanged`; `api/syncFixtures.ts` for the
browser preview and tests):

- **This device:** its name and kind, its addresses on this network, whether
  the internet answers (a relay is connected; unknown with Never use relays
  on), and Never use relays (`core.sync.neverRelay`).
- **Your devices:** each paired device with how it is reached (`peer-link`),
  its last sync, or that the last try didn't reach it (never a toast), Sync
  now and Forget. Connections close between syncs, so with none open a device
  shows `lan` while it announces itself here over mDNS, or the link of its
  last exchange while that went through under three minutes ago with no
  failure since (sync reaches each device every minute); otherwise `offline`.
- **Pair a device:** nearby Caskade, a pairing link to copy, and a pasted one.
- **What syncs:** #227's categories (`core.sync.categories`), synced and
  optional, plus what never syncs.
- **Copies and shares:** shows nothing until #233 and #246 exist, and no
  buttons.

Every account sees the tab. Only an admin's controls are enabled, and the core
refuses anyone else whatever the webview shows: the devices commands and
`core.sync` are admin-only in `permissions.rs` and `Permission::for_namespace`.

Pairing dialogs come from `components/sync/PairingHost.tsx` in the shell, over
any view:
- a code pairing started on either device (`pair-device`, its code larger in
  console mode);
- a `caskade://pair` link opened from outside, which pairs only when an admin
  presses Pair.

The status bar's `sync` part shows how many paired devices are in reach and
the best link, and opens Settings → Devices.

Waking a device (#234), copying a game here (#233), playing on another device
(#234) and shares (#246, #247) aren't shown: they come with their issues.

## Lifetime

`SyncEndpoint::bind` runs on the caller's tokio runtime; the crate creates
none. iroh's `Router` aborts its accept loop when dropped, so the app holds the
endpoint for the process lifetime and calls `SyncEndpoint::shutdown` at exit.

## Checks

- Tests: `flock /tmp/caskade-heavy.lock cargo test -p caskade-sync`. The
  integration tests (`tests/loopback.rs`) run two endpoints in one process on
  `Reach::Loopback`: one socket on 127.0.0.1, no relay, no address lookup, so
  nothing leaves the machine. They cover the opening, admission, a refusal
  that is byte-identical for every reason (including against a device that
  holds nothing), a silent dialler, the identity across a restart, and
  iroh-blobs on the same endpoint. `tests/network.rs` covers reaching devices
  against a local relay server (iroh's test relay, so nothing reaches the
  internet): Never use relays leaving a relay-only device (IP transports
  cleared) offline, the same device reached through the relay when allowed,
  a connection dialled through the relay alone switching to a direct path
  with link events following it, a typed address with no discovery, a link
  address that doesn't wait for a missing relay, and a fixed port. mDNS
  discovery runs on this machine's real interfaces with no relay and no
  internet lookup, under a service name unique to the run: the devices find
  each other, read each other's name, kind and OS, dial by id alone, and see
  a rename. Set `CASKADE_TEST_NO_MULTICAST=1` where the platform has no
  multicast.
- Pairing, in the crate (`tests/pairing.rs`): codes match and need both
  confirmations, a cancel on either side ends both, a third device is refused
  while a pairing is in progress, a link works once, and expired links are
  refused (by the link itself, and by the minting device's own record).
- The core (`cargo test -p caskade-core --test it devices::`): pairing by link
  both ways and once only; pairing by code over mDNS with no relay and no
  internet lookup, needing both confirmations, and a cancel storing nothing; a
  forgotten device refused until it pairs again, including one forgotten
  while away and told when it knocks; the admin rule for every change; a lost
  key asking before a new identity; no key or grant in the settings record
  or the status.
- The adapter (`cargo test -p caskade-desktop --lib commands::sync::`): the
  commands and `DevicesChanged` on Tauri's mock runtime, a `caskade://pair`
  link held for an admin rather than pairing, and every change refused for a
  standard account.
- Lint: `flock /tmp/caskade-heavy.lock cargo clippy -p caskade-sync
  --all-targets -- -D warnings`, and the same with
  `--target x86_64-pc-windows-gnu`.
- macOS cross lint, for this crate alone. ring's C build needs Apple SDK
  headers, so the gate compiles it with clang against check-only stubs in
  `crates/caskade-sync/xcheck/apple/` (clippy never links):

  ```sh
  CC_x86_64_apple_darwin=clang \
  CFLAGS_x86_64_apple_darwin="-nostdlibinc -I$PWD/crates/caskade-sync/xcheck/apple" \
  flock /tmp/caskade-heavy.lock cargo clippy -p caskade-sync --all-targets \
    --target x86_64-apple-darwin -- -D warnings
  ```

## Two machines on one network, internet unplugged

The in-process tests share one host, so they can't show two routers' worth of
network. This check needs two machines on one network, with the router's
internet unplugged:

1. Start Caskade on both; open Settings → Devices (#231) or call the commands.
2. Each lists the other as nearby within a few seconds.
3. Pair them by code (#226); the link state is `lan`.
4. Make an edit on one and see it on the other (#230).
5. Turn on Never use relays on one and plug the internet back in: the link
   stays `lan`.
6. Block multicast between them (or use client isolation) and pair with a
   pairing link instead.

Results:

| Date | Machines | Steps passed | Notes |
| --- | --- | --- | --- |
| — | — | — | Not run yet: needs two machines. Library sync (#230) is in place. |

### Copying a game (#233)

1. On a Windows machine, add a Windows game by hand (Add game → Add a game on
   this computer…) from a folder of its own.
2. On a Linux machine paired with it, open the game's page: On your other
   devices shows the Windows machine. Press Copy to this computer.
3. Mid-copy, unplug the network cable (or turn Wi-Fi off) on either machine,
   wait, plug it back in: the job in Activity waits ("Waiting for …"), then
   goes on from where it was, not from zero.
4. When it finishes, Play: the game starts through Proton.
5. Copy a large game (tens of GB) over the local network and record the
   speed against the disks' and the network's.

| Date | Machines | Steps passed | Speed | Notes |
| --- | --- | --- | --- | --- |
| — | — | — | — | Not run yet: needs a Windows and a Linux machine. Loopback: 432 MB/s. |
