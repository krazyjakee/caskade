# Devices, sync, saves and sharing

How a household's Caskade devices reach each other, keep the library in
step, carry saves, copy and share games and start them on each other.
Milestone: Devices, sync and sharing (tracked in #20).

The manual-check sections use GitHub issue numbers. Three-digit numbers
elsewhere on this page are archived Gitea issues, the numbers old commits and
code comments cite; `docs/manual-checks.md` maps the ones behind open checks.

## At a glance

- **Connections.** Devices find each other on the local network first
  (mDNS), connect directly over the internet where the routers allow it, and
  use n0's relays only when allowed and nothing direct works; *Connect only
  directly* turns relays off. Everything works with no internet on one network.
  Game files never go through a relay. ("Reaching devices", "Link state".)
- **Pairing.** By matching codes on one network, or by a one-time link that
  works for ten minutes; an admin confirms either. Forgetting a device tells
  it. ("Pairing and devices".)
- **What syncs.** The library's user values, favourites, hidden games,
  collections, play history, accounts and PINs, artwork the user chose,
  achievement links, RomM servers (never their passwords), appearance and
  saves; launch actions, game scripts, preferred emulators, emulator
  configuration and add-ons when the user turns them on. Installs, folders, emulators, store sign-ins and
  caches never sync. Each category is Sync or This device only on each
  device. ("What a change is", "Settings → Devices".)
- **Conflicts.** The later change to a field wins (hybrid logical clocks, so
  a wrong clock can't win); lists merge item by item; play sessions add up.
  Saves are the exception: when both devices played since they last agreed,
  both saves are kept and Play asks which to use. ("Clocks", "Saves".)
- **Saves.** Where each game keeps its saves comes from the Ludusavi manifest,
  cached so it works offline; versions are kept and travel as files beside
  the change log. ("Saves".)
- **Copies, sending and sharing.** A game copies from another device as a
  managed install, sends to one that confirms it, or shares once with someone
  else through a revocable link. ("Copying a game from another device",
  shares.)
- **Playing elsewhere.** Play on … starts a game on another device, waking a
  sleeping desktop with Wake-on-LAN first. ("Playing a game on another
  device".)
- **Settings → Devices** explains every switch ("Settings → Devices"), and
  "Manual checklist (#19)" is what only real machines can show.

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
- **n0's relays** only when Connect only directly is off. On, it's the
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
| `relay` | Through a relay, heard from in the last 12 s |
| `offline` | No open connection, or a relay path that has gone quiet |

`Links` follows every session's connection through its path events and sends
a `LinkChanged` whenever a device's state changes, including a connection that
started through a relay switching to a direct path once holepunching succeeds.

A selected relay path also has to be hearing something. A direct path that
stops answering times out after 15 s and iroh falls back to the relay path,
however long that has been silent, then keeps it selected until the whole
connection times out at 30 s. After a pulled cable that read as "Internet,
through a server" with no internet, and stayed as the device's last link. Each
end sends a keep-alive on every open path about every 5 s, so `Links` looks at
what relay paths have received every 2 s: after 12 s of nothing the link is
`offline` and the last link is forgotten, and anything received brings it back.
Measured on two namespaces with a local relay (`tests/network.rs` keeps the
in-process half): the state went from `lan` to `offline` when the direct path
timed out, 17 s after the cable went, for either end's cable.
Relays carry data at roughly 1 MB/s (wobu measured 512 KiB in 504 ms against
22 ms direct), which is why game files require a direct path (#233).

### Through two routers

Game files travel on endpoints with no relay transport, so none can fall back
to a relay. That also means they don't use the sockets hole punching opened: a
router lets in what answers the port that wrote out, and the transfer's
endpoint has a port of its own. So a dial from it reaches a device with a
public address, a mapped port or a full-cone router, and nothing behind an
ordinary one. When the plain dial fails, both ends make a way
(`caskade_sync::punch`, frame `Punch` on the `copy` or `share` session):

1. Each opens a UDP port of its own for this transfer (`open_port`). A
   throwaway endpoint with a key of its own, so the device's connection to its
   relay stays undisturbed, asks a relay what public address the port has,
   then closes. With Connect only directly on there's no relay to ask, and no
   way is made.
2. The receiver binds its relay-free dialer on that port and sends its
   addresses. The sender binds a relay-free `FileServer` on its port, with the
   device's own provider and gate, starts dialling the receiver from it, and
   answers with its addresses. The session may be on a relay: it carries no
   game data.
3. The receiver dials. Each side's first packet leaves a hole in its own
   router, so the dial gets through once the sender's has gone out. The
   `FileServer` closes with the session.

A session on a relay gets `PUNCH_WAIT` (5 s) to move to a direct path
(`Session::direct_path`). If it doesn't, nothing is tried: the job waits and
says only a relay reaches the device. A router that picks a new port per
destination (symmetric) defeats iroh's punching and this alike, whichever end
it's on, and a relay-free dial can't be told where the mapping is.

## Pairing and devices

`caskade_core::devices::Devices` (only with the core's `net` feature) keeps
this device and the devices paired with it. Commands are in
`src-tauri/src/commands/sync.rs` (`devices_*`); the status reaches the webview
as `DevicesChanged`. Settings → Devices itself is #231.

### This device

Its endpoint id, name and kind are in the settings table under `sync.device`
(not a section). The name defaults to the host name. Name and kind go into
the mDNS announcement and are exchanged while pairing. `core.sync` holds Connect
only directly (`neverRelay`) and the listening port (`port`) next to #227's
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
| `sync`, `copy`, `forget`, `stream` | The device is paired and presents its own grant |
| `pair` with a grant | The grant is a pairing link offered here, unused and unexpired (spent now) |
| `pair` without a grant | No other code pairing is in progress |
| `share` | Whoever keeps the shares says so (`Devices::set_share_admission`, #246); a paired device's grant never opens one |

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
Another device clearing its default clears nothing here. A waiting one is
offered in Settings → Emulation → Emulators under From your other devices
(#275, `sync::offers::emulator_offers`, `emulators_from_devices`): the
platform, the emulator and the device that chose it, with Install (one-click
emulator setup, where Caskade can install it here) and Keep mine
(`keep_emulator`, `emulator_offer_keep`: the waiting changes are set aside, so
installing that emulator later changes nothing). Nothing installs by itself.
When emulators change here (`EmulationChanged { scope: "emulators" }`),
`offers::watch_emulators` applies what waited, so an emulator installed from
the offer is the platform's default at once.

Emulator configuration (#275, optional, `sync::config_files::EmulatorConfig`)
carries the files an emulator's definition names in `[config]` (RetroArch:
`{config}/**`, its config_directory, with input remaps, core and game
overrides, core options and shader presets). It travels in the blob exchange
like add-ons' offers, not the change log: each exchange's `emulatorConfig`
note lists this device's files for every emulator it has, each by definition,
named folder (from the definition's variables, as save versions name theirs)
and path inside it, with its SHA-256, size and when it last changed. An index
in `<data>/emulator-config/index.json` reads a file again only when its size
or time changed. A file another device changed later is fetched (at most
4 MiB) and written into this device's own folder of that name: the same
RetroArch remap goes to `~/.config/retroarch/config/remaps/…` beside an
AppImage, inside `~/.var/app/org.libretro.RetroArch/` for the Flatpak, and
beside `retroarch.exe` on Windows. A file changed here since it was last seen
is kept and goes the other way next time; a device without that emulator
takes nothing, and removals aren't carried. Test:
`cargo test -p caskade-core --test it emulator_config::`.

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
with a battery or a TV without one. Windows checks the same names against the
firmware's in the registry (`SystemProductName`, `SystemFamily`), and a battery
without one of them is a laptop. Anything else is a desktop. A device's record
starts with what was detected, and Settings → Devices changes it.

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
   the reason ("install it from the store there"). It adds every file but the
   folder's own `.caskade` (this device's install record, and its Wine or
   Proton prefix with the saves in it, none of which the receiver wants),
   referenced where it is, to an iroh-blobs collection named by relative paths
   (`caskade_sync::copy::CopyStore`, `<cache>/copies/send`; only BLAKE3
   outboards are stored), saying how far it got, and offers the root hash, size,
   title, the program relative to the folder and whether it's a Windows build.
   Never a path or arguments. An emulated release goes as its ROM files
   alone, never its folder, which may be a ROM scanner's holding every game
   of the platform (#279, `transfer::rom`): they're linked together under
   `<cache>/copies/send/roms/<entry>/`, and the offer adds the platform and
   the emulator definition and profile they play with. The receiver places
   them as a release of the same game in a folder of its own in the chosen
   storage, pinned to its emulator of that definition and profile, or else
   emulator setup installs it (as a shared ROM, #248). The offer is kept
   for six hours, so a copy that comes back resumes without the files being
   read again; while it's being sent, the release can't be moved or
   uninstalled.
3. Its iroh-blobs provider serves only paired devices it offered something,
   and only those collections (`CopyGate`: connections and get requests are
   intercepted; get-many, push and observe are refused).
4. The receiver checks there's room (what's still to come in the cache, and
   the install in the root: twice the game on one disk), then pulls the
   collection over a `DirectDialer`: a second endpoint with this device's key
   and no relay transport, to the sender's direct addresses. Game files can't
   pass through a relay. Behind routers no plain dial gets through, and the
   two make a way first ("Through two routers"). While only a relay reaches
   the sender, or it can't be reached, the job waits and tries again ("Only a
   relay reaches …"), and what arrived stays verified in
   `<cache>/copies/receive/store`.
5. The files are exported to `<cache>/copies/receive/incoming/<job>/` (the
   portable installer takes a folder outside the storage root) and installed
   from there like a dropped game folder, with the sender's title and program
   as hints: a managed install with a `caskade-install` id of its own, on the
   game that was copied, its program relative to its folder and its runtime
   as the files say (a Windows build runs through umu/Proton on Linux). The
   staging folder goes afterwards, whatever the Installer's source setting
   says.
6. A collection holds names and bytes, not file modes. On export a file that
   starts like a script or a Linux or macOS program is made executable, so a
   Linux game arrives able to start (a sender on Windows has no modes to give
   anyway).
7. Everything in a copy store is kept by a tag (an offer's, a fetch's from
   its start, a temporary one while files are added), and the store collects
   what no tag keeps every 10 s (#278). Exports leave the store pointing at
   files that then move on into an install, and reading those would spoil
   the store, so a fetch or `local_bytes` that finds leftovers of the same
   collection waits for a collection first (`CopyStore::settle`, up to about
   half a minute). Collecting never touches files outside the store.

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
refreshed every two seconds while there are any). Speeds measured so far are
in "Copying a game (#11)" under Manual checklist. A relay moves about 1 MB/s.

### Downloads from a device nearby

When one device has downloaded a file, the others get it from that device
over the local network before the internet (#236, `install::nearby`):
faster, without spending the connection, and with no internet at all.

- **Keeping.** A download the installer finished from its source (a game, an
  update, any install from a link) is kept in
  `<cache>/peer-downloads/<sha256>/<file>` (a hard link where it can be),
  offered through the copy store (`download:<sha256>`) and listed in
  `index.json` with its URLs, size, SHA-256 and the source's validator (a
  strong ETag, or a Last-Modified date the source's clock vouches for;
  `DownloadedFile::validator`). Kept files past 32 GiB go oldest first, and
  only when a new one is kept, which is after a download from the internet.
  One whose file is gone or changed size (a cache clean) is let go of, its
  copy-store tag released, before the next exchange tells anyone about it.
- **Telling.** Each exchange's blob round carries a `download` note
  (`BlobKind`, governed by its own setting) listing them. The device that
  tells lets the other fetch each one through the copy gate.
- **Finding.** Before the installer downloads (`caskade_installer::fetch::LocalSource`,
  asked by the pipeline's download step), the file is looked for on a paired
  device whose link is `lan`: by SHA-256 where the request names one,
  otherwise by URL. A URL match is taken only when the source, asked with a
  `HEAD`, answers with the same validator and size, or doesn't answer at all.
  The file comes over the `DirectDialer` only (game files never relay) into
  `.caskade/downloads/<job>/near/`.
- **Checking.** The installer checks it as it checks any download: the
  request's size and SHA-256, and also the size and SHA-256 the other device
  said it had. A file that fails is deleted and the download goes to its
  source, as it does when no device has the file or one stops answering.
  The pipeline then reads it like any download (archive checks included).

`core.sync.peerDownloads` (Settings → Devices → Downloads from my devices; on
by default, kept on each device) turned off keeps, offers and asks for
nothing: every download goes to its source. Tests:
`cargo test -p caskade-core --test it peer_downloads::` (two cores on
loopback and a wiremock source: an update installs on the second with the
source gone, a file that isn't what the other device said is discarded and
downloaded from the source, and turned off the source is used).

Outside the installer (#277), the same `PeerDownloads` serves what other
services download, through `PeerDownloads::fetch_to` (by URL, checked against
the size and SHA-256 the other device said it has) and `keep_path`:

- **Emulators, RetroArch cores and managed tools** (`managed::download::fetch`,
  `EmulatorSetup::set_nearby`, `ManagedTools::set_nearby`) ask first and keep
  what they download, with its ETag or Last-Modified. A release a source has
  to resolve first (GitHub's newest release) is also kept under a release key
  (`emulator:<id>:<os>:<arch>`, `tool:…`) with its version, so a device that
  can't reach GitHub installs the newest release a device nearby has
  (`PeerDownloads::release`). Setup shows the bytes as they arrive.
- **BIOS files** (`EmulatorSetup::fetch_bios`): what RomM's firmware supplied
  is kept under `caskade-bios:md5/<md5>` (the definition's MD5), and what it
  couldn't, or every file with no RomM server, is asked of devices nearby and
  checked against that MD5.
- **Hasheous dumps** (`Hasheous::set_nearby`): asked before Hasheous, checked
  against the listing's size and MD5; with no listing (never online) the
  dump's URL is still known (`api/v1/Dumps/platforms/<name>.zip`), so it comes
  from a device nearby and is indexed as usual.
- **The save manifest** (`Ludusavi::set_nearby`): while this device has no
  usable manifest from the same source, `refresh` asks devices nearby first,
  online or not. One from a device counts as never checked, so the next
  refresh online asks the source.

IGDB artwork doesn't travel this way: it is thousands of small files that
would swell every exchange's note, the artwork users chose already syncs
(#273), and covers download again once online. Tests:
`peer_downloads::an_emulator_installed_on_one_device_installs_on_another_without_the_internet`
(release, file and BIOS from the Desk with the fake GitHub gone) and
`peer_downloads::a_hasheous_dump_and_the_save_manifest_reach_a_device_never_online`.

### Playing a game on another device

Play on … (#234: the On your other devices panel and the game menu's Your
devices; command `game_play_on`) starts a game on a paired device that has
it installed (`remote_play`). The requester opens a `play` session (its own
grant: an unpaired device is refused before anything is read) and names the
game in its ids. The target maps it through library sync's game matching,
refuses when `core.sync.remotePlay` is off (Settings → Devices → Play requests
from my devices; on by default, kept on each device) or the game isn't in its
library, and hands it to the app (`RemotePlayRequested`):
`components/sync/RemotePlayHost.tsx` plays it as Play there would, with that
device's signed-in account. So an age limit asks for the admin PIN on the
target, and console mode shows the game as for a local launch. How it went
(`remote_play_finished`) goes back on the session within three minutes: the
requester shows Starting on …, then Started on …, or the reason it didn't. The
game running there then shows on the requester through presence (#240), and
its session lands on both devices through library sync.

A device out of reach is woken first when its Wake-on-LAN addresses are
known: each device sends its wired and wireless MAC addresses in every sync's
`Hello` (recorded in `paired_devices.wol_macs_json`), and Wake sends a magic
packet to every local broadcast address on ports 9 and 7, then waits up to two
minutes for the device to come into reach ("Waking …"; "Wake and play on …").

### Sharing a game once

A game can be shared once with someone else, through a link (#246,
`caskade_core::share::Shares`, `net` feature): **Share with someone…**, then
the link, `caskade://receive?t=…` (`caskade_sync::share::ShareLink`, a ticket
of kind `caskadeshare`). It holds the sharer's address (local addresses, and
the relay once there is one), a grant made for that share alone and when it
expires (a day, a week, or until stopped). Only `to_url` writes a link out:
`Debug` leaves the grant out, and `parse` reads at most 4 KiB.

- **Sharing.** The release's folder goes through the copies' checks
  (`Transfers::source`: installed, a managed install or a folder of its own,
  not a launcher's) and is read into the send store under `share/<id>` in the
  background (`ShareState::Preparing`, then `Ready`). The share and its grant
  are `shares` rows, and who opened it `share_recipients` (migration 0034).
  Both stay on this device: not synced, not backed up (the grant is a
  credential). Shares that were running start again with Caskade (`resume`).
- **Admission.** A `share` session is admitted only with the grant of a share
  that is running now (`Devices::set_share_admission`). A paired device's
  grant never opens one, and a share's grant opens nothing else. Stop sharing
  (`stop`) and the expiry both end it at once: the link is refused from then
  on, sessions still open are told `Stopped` and closed, and every gate
  revokes the collection, which stops transfers part-way. What was received
  stays.
- **Looking.** `inspect` asks the sharer what the link shares (`About`:
  title, size, OS of the build, sharer, expiry) and whether it was reached
  directly or only through a relay (`ShareReach`). Nothing is fetched. A
  link past its expiry says "This link expired on <date>" without dialling;
  a refused one says the share was stopped or has expired (the refusal is
  the same bytes either way); a sharer out of reach says Caskade keeps
  trying.
- **Receiving.** `receive` runs the copy job (`InstallService::copy_with`
  with a `PeerFetcher` of the share's): the same waiting, resume, cancel and
  install as a copy, as a game of its own. The offer names the collection,
  its size, the program relative to the folder and the sharer's own direct
  addresses.

**In the app** (#247, `components/shares/`, `api/shares.ts`):

- **Share with someone…** in a game's menu (Your devices) opens `ShareGameDialog`. The
  user picks how long the link works (a day, a week, until stopped), then gets the link
  with Copy, what the share is doing, who received it, and Stop sharing. Console mode adds
  the link as a QR code (`share_qr`, an SVG from the `qrcode` crate), for a phone to pass
  on. The dialog also takes #248's items (`ShareTarget` `item`), from where
  each lives: a save version's *Share…* in the game's Saves panel; *Share
  with someone…* in a collection's menu (its page's More, its card), which
  says nothing is downloaded; and the game menu's *Share with someone…*,
  which shares a web game's offline copy or an emulated game's ROM as
  those kinds rather than the folder they're in.
- **Add game → Receive a shared game…** (`ReceiveShareDialog`) takes a pasted link. A pad
  pastes on the on-screen keyboard's Paste key (`clipboard_read_text`; Copy buttons write
  through `clipboard_write_text`, since a webview allows the clipboard only for a click or
  a key). It asks the sharer what the link holds (`ShareInfoDetails`: who, size, build,
  expiry, whether only a relay reaches them; for #248's kinds, where it goes and which of
  a collection's games are here). Receive asks where it goes (the storage picker, which
  skips itself when there's no choice) and starts the copy job. Help others receive it
  after it's here sets `keep_helping`, and a collection is added to the account's
  collections and opens.
- **Links from outside.** A `caskade://receive` link opened from outside waits in the
  install confirmation with the same details. A collection's link is confirmed with
  `confirm_share_collection`.
- **Settings → Devices → Copies and shares** lists the shares (Copy link and Stop sharing
  while one runs, Forget after; each recipient's progress and whether they help) and what
  was received (the Help others receive it switch).
- **Notifications.** Someone receiving everything of a share is a notification on the
  sharer's device (`shareReceived`, `Event::ShareReceived`), and a received game that
  installed is one on the recipient's, opening its page (`shareArrived`,
  `Event::ShareArrived`).

**Off the local network.** The `share` session itself may go through a relay
(it carries no game data), and that connection is how both ends learn each
other's direct addresses: iroh's hole punching and the port mapping it asks
the router for (UPnP, NAT-PMP, PCP) put the sharer's public address on the
session's paths. `inspect` gives hole punching 5 s before it says the sharer
is `RelayOnly`. The files then come only over a relay-free endpoint: straight
to the sharer when a plain dial reaches it, or through a way the two make
("Through two routers"). When the session stays on the relay, or no way opens,
the job waits and says so ("Only a relay reaches …"); game files never take
the relay. So a share works across networks when the routers let the two punch
a path or the sharer's router maps a port; a symmetric router at either end
waits. A share's other recipients behind routers aren't made a way to: the
game comes from the sharer. Tested on loopback with the relay transport removed
(`Reach::loopback`): `cargo test -p caskade-core --test it shares::`, and
behind routers in namespaces: `scripts/two-networks.sh`. Two people on
different networks is the manual check (step 5 of the manual checklist below).

**Recipients help each other** (#249, `caskade_sync::swarm`). A recipient
fetches with iroh-blobs' downloader from several providers at once: the
collection split per file, each file asked of the providers in a shuffled
order, every chunk checked against the BLAKE3 tree whoever sent it. The
providers are the sharer and the share's other recipients that serve it.
While it receives, a recipient serves what it has (`Serving` with its direct
addresses). The sharer names the recipients that serve to each new one
(`Providers`), after asking them to let it in (`Allow`, answered `Allowed`;
each recipient's gate allows only that collection, only to recipients the
sharer named). Once the game is here it stops serving, unless the user chose
**Help others receive it** (`Shares::keep_helping`) while it arrived: then
the store keeps its own copy of the files (`Transfers::export_copy`, so the
game takes its space twice while it helps) and serves them until the share
stops, the session with the sharer ends, or the switch goes off. A recipient
serves only while its session with the sharer is open, so stopping the share
stops every recipient serving it. Test: `shares::the_third_recipient_…` (four
cores) and `cargo test -p caskade-sync --test it swarm::`.

**Saves, web games, ROMs and collections** (#248, `share::items`). The
same links share smaller things (`Shares::share_item`, command
`share_item` with a `ShareItem`): a version of a game's saves (its Saves
panel), a web game's offline copy, an emulated release with its emulator
profile, or a collection. Each one's description travels in the session's
`About` frame (`items::Details`, kept in `shares.details_json`), so a
recipient's `share_inspect` answers what it holds and what of it they
have (`ShareInfo.contents`) before anything comes. The files, where there
are any, travel as a game's do (direct only, resumable, recipients helping
each other):

- **A save** is its files, named by their SHA-256, with each file's named
  folder (`<winAppData>`, `{save_data}`, …) and path. The recipient finds the
  same game by its identity, as sync does (IGDB id, store ids, then title),
  or adds a title-only record of it, and keeps the files as a new version
  of its saves for the account signed in (`SaveVersions::add_shared`,
  reason *Shared by someone*). The version descends from nothing, so it
  never replaces a save without asking: with saves of their own, the next
  Play asks which to use; with none, it is simply their save. Files that
  were at an absolute path on the sharer's computer have no place
  elsewhere and are left out.
- **A web game's offline copy** (`WebGames::save_offline`'s bundle) becomes
  the offline copy of the web game at the same address, in the storage the
  recipient chose; the game is added when they haven't got it. An offline
  copy they already have stays.
- **A ROM** brings its files, its platform, and the emulator definition and
  profile it played with. It arrives as a game of that platform in a folder
  of its own in the chosen storage, pinned to the recipient's emulator of
  that definition and profile when they have one; otherwise it follows the
  platform's default, and emulator setup installs that emulator when the
  platform has none.
- **A collection** is only its list: each game's identity, up to 1,000,
  read through a larger `About` (1 MiB; every other frame stays at 64 KiB).
  `share_inspect` shows which games the recipient has installed, owns
  through a store account, has in the library or hasn't got;
  `share_receive_collection` adds it to the signed-in account's
  collections, the games they have and a title-only record of each one they
  don't. Nothing downloads.

Tests: `cargo test -p caskade-core --test it share_items::` (one per kind,
between cores that aren't paired).

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
  here (`notArrived`). A managed Windows game that hasn't started here has no
  prefix yet: it gets the prefix's user folder first, and Proton or Wine builds
  the rest around the save;
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
are allowed and nothing direct works; Connect only directly keeps them direct. Game
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
snapshot.

Two more run a real program, `tests/fixtures/saves/proof_game.rs`, which asks
Windows for its folders, reads its saves there and writes the next ones:

- `tests/proton_real.rs` (`scripts/proton-real.sh`, see "Copying a game (#11)")
  does it under real Proton in a copied game's prefix. The other device is a
  fixture Windows machine: its Windows folders are folders standing in for
  them, and the test writes its save.
- `tests/windows_saves.rs` does it on Windows, with the real `%APPDATA%`,
  `%LOCALAPPDATA%`, Documents and Saved Games. The other device is again a
  fixture Windows machine, as a fixture Linux machine can't run on a host whose
  paths have drive letters; a version names its files by those folders on
  every device. CI's `windows-elevation` job runs it as a step that can't fail
  the job yet. So far that binary has run only cross-compiled under Proton
  (rehearsed with `CASKADE_PROOF_WINE=1`), never on Windows.

| Check | Result |
| --- | --- |
| Play on Windows, quit, start on a Linux device through Proton: the Windows save is there | 2026-11-06, one PC: passed under real Proton from a fixture Windows device, first launch included (the prefix didn't exist yet), and the game read the save. No Windows machine involved. |
| Play on the Linux device, quit, start on Windows: the Linux save is there | 2026-11-06, one PC: the version kept from the real prefix (four files, each once) restored to the fixture Windows device's folders, and nothing else. Reading it back on real Windows not yet run. |
| Two real machines, Windows and Linux (WL2) | not yet run |

## Setting up a new device

First-run setup's welcome offers **Set up from another device** (#242,
`onboarding/DeviceStep.tsx`) wherever devices can pair. The new device pairs
with a nearby Caskade (both show the code, confirmed on both) or by a pasted
pairing link, then asks that device to sync and waits for the first sync to
finish. That brings the household's library, accounts with their PIN hashes,
collections, play history and the settings that sync. The new device's own
untouched admin joins the household's (see The default admin), and when that
admin has a PIN, setup asks for it before going on. Setup then asks about this
device as on any first start (language, folder, sources, how it starts) and
runs the usual tasks, emulator setup included. What policy keeps on each
device (storage, installs, emulators, console and window settings, the
interface scale) stays the new device's own. None of it needs the internet:
devices find each other on the local network and connect directly.

After the folder question, setup from another device asks which of that
device's games to copy here (#276, `onboarding/GamesStep.tsx`):
`sync::offers::device_games` (command `device_games`) lists the games it has
installed whose copy can come here (its own folder or a Caskade-managed one,
a build that runs on this system, Windows builds through Proton on Linux; no
launcher installs, hidden games or games installed here already), each with
its size. The chosen total, plus the largest game again (a copy arrives in
the cache before it installs, so on one disk it takes twice its size while it
does), is checked against the chosen folder's free space, and Copy waits while
it doesn't fit; the copy job's own room check still has the last word. Each chosen game becomes a copy job (#233) in Activity and arrives while
setup goes on; choosing none copies nothing.

`tests/new_device.rs` covers it with two cores on loopback, where there's no
relay and no address lookup: the new device gets the library, both accounts
(the admin's PIN included), the collection and a synced setting, and keeps its
own storage root, no emulators, its console mode and its interface scale.
`onboarding/fromDevice.test.tsx` covers the setup screens. Not yet checked on
real devices:

| Check | Result |
| --- | --- |
| A new device on a network with no internet set up from another: same library, accounts and collections; its own storage, emulators and settings | passed 2026-11-06 in `scripts/lan-unplugged.sh` (two processes in network namespaces, the core's steps, no setup screens); not yet run on two machines |
| Games chosen during setup copy from the other device over the local network (Gitea #276) | not yet run |

## Settings → Devices

`views/settings/devices/` (#231), over `api/sync.ts` (the native API on the
`devices_*` commands and `DevicesChanged`; `api/syncFixtures.ts` for the
browser preview and tests):

Every control, section by section:

- **This device**
  - *Device name*: what other devices and the nearby list call this one;
    empty goes back to the computer's name (`devices_rename`).
  - *Kind*: desktop, laptop, handheld or TV, for its icon elsewhere; until
    the user picks one, what the device looks like (`device_type`, #244).
  - Its addresses on this network, and whether the internet answers (a relay
    is connected; unknown with Connect only directly on).
  - *Connect only directly* (`core.sync.neverRelay`, off): connections stay
    on this network or over the internet where the routers allow it; a
    device only a relay could reach shows offline. Game files never use a
    relay whatever this says.
  - *Play requests from my devices* (`core.sync.remotePlay`, on): other
    devices' Play on … starts games here (#234, "Playing a game on another
    device").
  - *Accept copies from my devices* (`core.sync.acceptCopies`, off): a game
    another device sends here copies into the default storage without asking
    (#235); off, it waits in the install confirmations.
  - *Downloads from my devices* (`core.sync.peerDownloads`, on): a download
    comes from a device on this network that has the same file before the
    internet (#236, "Downloads from a device nearby").
- **Your devices**: each paired device with how it is reached (`peer-link`),
  its last sync, or that the last try didn't reach it (never a toast), what
  it is playing now (#240), and *Sync now* and *Forget* (forgetting tells the
  other device when it can be reached, and removes its copies listed here).
  Connections close between syncs, so with none open a device shows `lan`
  while it announces itself here over mDNS, or the link of its last exchange
  while that went through under three minutes ago with no failure since
  (sync reaches each device every minute); otherwise `offline`.
- **Pair a device**: Caskade on this network with *Pair…* (both show a code;
  pair when they match), *Copy a pairing link* (works once, for ten
  minutes), and *Paste a pairing link from another device* with *Pair*.
- **What syncs**: Sync or This device only for each of #227's categories
  (`core.sync.categories`), the synced ones first, then the optional ones
  (launch actions, game scripts, preferred emulators, emulator
  configuration, add-ons) that start
  This device only; and **Never synced**, what stays on each device. The
  Appearance, Accounts and RomM panels each have *Same on your other
  devices* (`core.sync.sections`, #244): off keeps that section on this
  device while its category syncs.
- **Saves** (#241): *Sync saves* (the Saves category), *Versions to keep*
  (`core.saves.keep`), and the *save locations list* with its last check and
  Update now.
- **Copies and shares**: copies going on both ways (`devices_transfers`),
  with progress, shown only while there are any. Then the shares this
  device made (#247): each with its state, who received it and how far
  each got (and whether they help others receive it), *Copy link* and
  *Stop sharing* while it runs, and *Forget* once it stopped or expired.
  Then what was received here: each "<title> from <sharer>" with its
  *Help others receive it* switch (`share_keep_helping`, #249).

Every switch here is kept on each device: `core.sync` is local in the sync
policy, so turning one off on one device never turns it off elsewhere.

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

A game's page shows its copies on the other devices with *Copy to this
computer* and *Play on …* / *Wake and play on …* (#232–#234), and its menu
offers *Copy to …* for devices without it (#235).

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
  internet): Connect only directly leaving a relay-only device (IP transports
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
- Two or more cores (#250): `crates/caskade-core/tests/cores.rs` builds a
  device in one process (its own library, settings, device key, copies and a
  storage root) on loopback, or through a local relay (`core_with`); the
  sync, copy, send and play-there tests start from it (`sync_network`,
  `game_copy`). `sync_network::through_a_relay_the_library_syncs_and_game_files_wait`
  runs two cores whose only link is a local relay
  (`caskade_sync::testing::local_relay`, the `test-utils` feature): the
  library syncs, the link says `relay`, and a copy waits for a direct
  connection with no game bytes moved.
- Networks of their own (`scripts/lan-unplugged.sh`, `scripts/two-networks.sh`,
  no root): a process per device in a network namespace of its own, on a
  bridge with no uplink, or behind router namespaces doing NAT with a real iroh
  relay on an "internet" bridge between them. `two-networks.sh` measures the
  routers' NAT before using it, counts the relay's traffic with iptables rather
  than taking iroh's word, and runs each scenario's negative control, which has
  to fail with the failure it names. CI doesn't run either.
- CI: the Rust app suites in `.github/workflows/ci.yml` run
  `caskade-sync`'s tests with the core's on Linux on every change to either,
  and its Windows GNU cross Clippy builds them for Windows. No test reaches
  the internet: endpoints are on loopback or a local relay, mDNS runs on this
  machine's interfaces under a service unique to the run, and the ignored
  `loopback_throughput` measures a copy by hand.
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

## What iroh costs the build (#19)

Measured on 2026-11-03 on the development machine (Linux x86-64, a warm
`~/.cargo` registry, the shared target's release profile cold):
`cargo build --release -p caskade-desktop --bin caskade --timings`.

- **Build time.** 5 min 2 s wall clock (56 min CPU) for 1,408 units. The
  iroh family (iroh, iroh-blobs, iroh-relay, iroh-dns, noq, netwatch,
  portmapper, redb, swarm-discovery, simple-dns, pkarr, irpc, bao-tree and
  `caskade-sync`; 46 units) takes 152 s of the 2,276 s serial build time,
  6.7 %: portmapper 22 s, iroh-blobs 18 s, redb 18 s, iroh 16 s, then the
  rest. Most of it builds in parallel with the rest of the app.
- **Size.** The release binary is 200.9 MB, 142.0 MB stripped. The iroh
  family's own code symbols are 3.6 MB of 105.9 MB (3.4 %); its generic code
  instantiated in `core` and `tokio` adds a little to that.

Measure again with the same command; the timing report is under
`<target>/cargo-timings/`, and `nm -S --size-sort -C` on the binary groups
code by crate.

## Manual checklist (#19)

CI covers what one machine can (below, and in `## Checks`): two or more
cores on loopback, relay paths through a local relay, no internet. What only
real machines can show (other operating systems, Wi-Fi against Ethernet,
NAT, a sleeping computer, two separate networks) is this checklist, run on
Linux, Windows and macOS, each pair of them:

1. **Pairing and sync**, with the router's internet unplugged: the procedure
   in "Two machines on one network, internet unplugged" below.
2. **A large copy over Wi-Fi, then over Ethernet**: "Copying a game (#11)"
   below, with a game of tens of GB. Record the speed each way against the
   disks' and the network's.
3. **Downloads from a device nearby**: "Downloads from a device nearby
   (#13)" below.
4. **Wake-on-LAN**: "Waking a device to play (#12)" below (the desktop of
   the pair).
5. **A share across two networks** (#17), between two people: share a
   game from one machine on the home network; open the link on a machine on
   another network (a phone's hotspot). The files come straight from the
   sharer (a direct path through the routers, or the port the sharer's router
   mapped), or the receiver says it waits for a direct connection; never
   through a relay. The game installs and plays. Stop the share: the link
   fails at once, mid-copy too. With a third machine receiving after the
   second chose Help others receive it, both serve it (#18).
6. **Everything with the internet unplugged**: steps 1 to 4 again with the
   router's internet unplugged (and step 5's receiver says "Can't reach whoever
   shared this: their Caskade may be closed or offline. Caskade keeps trying.").

| Pair | 1 Pairing, sync | 2 Copy (Wi-Fi / Ethernet) | 3 Nearby downloads | 4 Wake-on-LAN | 5 Share across networks | 6 Offline | Date, notes |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Linux ↔ Windows | — | — | — | — | — | — | Not run yet |
| Linux ↔ macOS | — | — | — | — | — | — | Not run yet |
| Windows ↔ macOS | — | — | — | — | — | — | Not run yet |

Step 5 on one PC, in network namespaces (`scripts/two-networks.sh`, no root,
2026-11-06). This isn't two networks: every device is a process on one Linux
kernel, the "routers" are namespaces running iptables, the "internet" is a
bridge with a real iroh relay (a local one, not n0's) and no latency or loss
but the rate limit T2 sets, the link crosses as a note between processes, and
n0's address lookup is off. A 32 MiB fixture game; the relay's traffic counted
by iptables, both ways, for the whole transfer or wait.

| Network (sharer / recipient) | Result | Through the relay |
| --- | --- | --- |
| cone / cone | Hole punched, the link `direct`; the game arrived, installed and its program ran | 95 KB |
| full cone / cone | Arrived by a plain dial and ran | 42 KB |
| symmetric / symmetric | `RelayOnly`; the job waited 40 s saying "Only a relay reaches …", 0 bytes | 301 KB |
| symmetric / cone | The same | 296 KB |
| cone / symmetric | The same | 301 KB |
| cone / cone, Stop sharing part-way (limited to 64 Mbit/s) | The copy failed "stopped" within one 100 ms poll of the stop returning, and the link was refused after | not measured |
| A copy between paired devices, cone / cone | `direct`, arrived and ran | 44 KB |

"Cone" is plain `MASQUERADE`: one mapping per inside port, the port kept, and
only what the device wrote to gets back in. "Symmetric" is `MASQUERADE
--random-fully`, a new port per destination; "full cone" adds a `DNAT` of
everything else to the device. A probe measures each before it's used. The
meter's control: 8 MiB sent through the relay counted 17.5 MB. Each scenario's
negative control fails with the failure it names (a share arriving between
symmetric NATs, one waiting between cones, the same copy left alone finishing).

Limits: no real router, carrier NAT, hotspot, Wi-Fi, IPv6 path, UPnP or NAT-PMP
mapping, Windows or macOS. A port that keeps its mapping after the throwaway
endpoint closes is what `caskade_sync::punch` relies on: Linux does; a real
router that doesn't makes the job wait. With a router that accepted
unsolicited packets on its WAN side (its conntrack entry then collides with the
device's mapping) hole punching failed and the link stayed `relay`, so the
routers here drop them, as home routers do. Recipients helping each other
across routers isn't covered, and a share's other recipients aren't made a way
to.

T3 the same way (#11, #20), paired through the relay first:

| Network | Connect only directly | Result |
| --- | --- | --- |
| symmetric / symmetric | Off | The link `relay`; a copy waited 40 s ("Only a relay reaches …"), 0 bytes, 319 KB through the relay |
| symmetric / symmetric | On, both | Never `relay` in 30 s of samples every 100 ms; `offline`, and no session opens |
| symmetric / symmetric | On, one device | Never `relay` on that device; no session opens from the other, which goes on showing its last link for up to 3 minutes |
| cone / cone | On, both | Never `relay`; `offline`: with no relay nothing punches |
| Public address and fixed port / cone | On, both | Never `relay`; `direct` |

## Two machines on one network, internet unplugged

The in-process tests share one host, so they can't show two routers' worth of
network. This check needs two machines on one network, with the router's
internet unplugged:

1. Start Caskade on both; open Settings → Devices (Gitea #231) or call the commands.
2. Each lists the other as nearby within a few seconds.
3. Pair them by code (Gitea #226); the link state is `lan`.
4. Make an edit on one and see it on the other (Gitea #230).
5. Turn on Connect only directly on one and plug the internet back in: the link
   stays `lan`.
6. Block multicast between them (or use client isolation) and pair with a
   pairing link instead.

Results:

| Date | Machines | Steps passed | Notes |
| --- | --- | --- | --- |
| 2026-11-06 | One PC: two processes in network namespaces on a bridge with no uplink (`scripts/lan-unplugged.sh`) | 1–4, plus a copy | Not two machines. Real mDNS, pairing by code, an edit, a collection and a session both ways, link `lan`, a copy of a fixture game. No real router, Wi-Fi or Windows. Steps 5–6 not run. With the bridge's ports isolated it fails at discovery, as it should. |
| 2026-11-06 | One PC: two processes in network namespaces on one bridge (`scripts/two-networks.sh`) | 5, 6 | Not two machines. Step 5: Connect only directly on one, then the router's uplink brought up to a local relay; the other device's relay answered, and the link and each session's real path stayed on 192.168.10.x both ways for 20 s while edits crossed. With the LAN cut instead, it fails. Step 6: `mcast_flood off` on the bridge's ports; neither saw the other nearby in 10 s, a pairing link paired, an edit crossed each way, link `lan`. With multicast passing, they see each other. No real router, Wi-Fi or guest network. |
| 2026-11-06 | Two Ubuntu VMs on a virtual LAN, dead gateway, app built from master | 1–4 | Real mDNS between kernels, pairing by code, link `lan`, library and sessions both ways. No real router or Wi-Fi. Discovery needs a default route: with none at all, `swarm_discovery` can't send. |
| — | Two physical machines | — | Not run yet. |

### Copying a game (#11)

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
| 2026-11-06 | One Linux PC, both devices (`scripts/proton-real.sh`) | 1 and 4 in part | — | Not a Windows machine and not two machines: see below. |
| — | A Windows and a Linux machine | — | — | Not run yet. |

**Step 4 on one PC (2026-11-06).** `scripts/proton-real.sh` needs umu, umu's
Steam Runtime and a UMU-Proton build where Caskade and umu keep them, copies
them to a scratch home under `/tmp/caskade-proton`, and runs
`tests/proton_real.rs` on an Xvfb of its own in a namespace with loopback and
no internet (`--keep` keeps the scratch home between runs).

- Real: a Windows PE program (`tests/fixtures/saves/proof_game.rs`) that opens
  its data by a relative path and asks Windows for its folders; umu-launcher
  1.4.4, pressure-vessel, the Steam Runtime, UMU-Proton-10.0-4 and the prefix it
  builds; the copy job, the install and the launch pipeline with
  `InstallerRunnerResolver`, as the app wires them. The program's own log says
  it ran on Wine 10.0 from `G:\Game` with its data found. The first play took
  23–28 s with the prefix to build.
- Not real: the sender is a Linux process holding a game added by hand with a
  Windows runtime, so no Windows machine sent anything. Both cores share one
  process, on loopback. The game is a small program, not a commercial one, and
  no video or input was exercised.

**Speed, without two PCs (2026-11-06).** `scripts/lan-unplugged.sh --speed
[rate]` copies a 2,054 MiB game (one archive and a thousand small files) in a
release build, between two processes in network namespaces joined by veth pairs
and a bridge. The 256 MiB test is `cargo test --release -p caskade-sync --test
it copy::loopback_throughput -- --ignored --nocapture`. The PC has 24 threads,
and other builds kept its load between 15 and 30 throughout.

| Path | Transfer | Whole job | Runs |
| --- | --- | --- | --- |
| veth and bridge, no limit | 94, 129, 142 MB/s | 51, 83, 84 MB/s | 3 |
| veth and bridge, `tc` limit of 1 Gbit/s | 94 MB/s | 72 MB/s | 1 |
| Loopback, 256 MiB, transfer alone | 154, 177, 224 MB/s (432 MB/s on a quieter day) | not measured | 3 |

The whole job adds the sender reading and hashing the folder (1–2 s) and
placing what arrived in the install (6–18 s). This shows the transfer isn't
held to a relay's speed on one PC, and that it filled about three quarters of a
limited 1 Gbit/s link (125 MB/s) on a busy machine. It doesn't clear N1: no
real network, Wi-Fi, second disk or tens of GB were involved, the sender read
files still in the page cache, and the veth link has no real latency or loss.

### Downloads from a device nearby (#13)

1. On two paired machines on one network, install the same older release of
   a game from a link (a GitHub release, say) on both.
2. Update it on the first from the internet.
3. Unplug the router's internet. On the second, wait for a sync (or press
   Sync now in Settings → Devices), then update: the update installs, from
   the first machine.
4. Plug the internet back in, turn off Downloads from my devices on the
   second and install another game the first has downloaded: it downloads
   from the internet.

| Date | Machines | Steps passed | Notes |
| --- | --- | --- | --- |
| — | — | — | Not run yet: needs two machines. Loopback with a wiremock source passes. |

Emulators, dumps and the manifest (#34): with the first machine's emulator
installed through Get emulators and its Hasheous dumps and save manifest
downloaded, set up the second with the router's internet unplugged. Install
the same emulator there, then check that `hasheous/dumps/` in the cache folder
holds the dumps' `.zip` files (no screen in the app lists them), that Settings →
Devices → Saves no longer calls the Save locations list "Not downloaded yet",
and that a game's Saves tab in its editor names save locations.

| Date | Machines | Steps passed | Notes |
| --- | --- | --- | --- |
| — | — | — | Not run yet: needs two machines. Loopback with a fake GitHub, Hasheous and manifest server passes. |

### Waking a device to play (#12)

1. On a desktop with Wake-on-LAN on in its firmware and network settings, sync
   once with the other device (it learns the MAC addresses), then put the
   desktop to sleep.
2. On the other device, open a game installed on the desktop: On your other
   devices offers Wake and play on …. Press it.
3. The desktop wakes ("Waking …"), the game starts there ("Started on …"),
   and the game shows as running on both devices.
4. With an age-limited account signed in on the desktop, the admin PIN is
   asked there before it starts.

| Date | Machines | Steps passed | Notes |
| --- | --- | --- | --- |
| 2026-11-06 | Two Ubuntu VMs, real S3 sleep | Wake and play; Stream from a sleeping host | Real magic packets. A script on the PC stood in for the NIC. No physical Wake-on-LAN. |
| — | — | — | Not run yet: needs a sleeping desktop with Wake-on-LAN. |
