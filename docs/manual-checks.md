# Manual checks still open

These are the checks that tests can't do: real machines, real networks, real
accounts and real hardware. Each one closes an unchecked box on a GitHub
issue, unless it says it clears no box. They are grouped by what you need set up, simplest first.

How to report:

- When a check passes, tick its box on the issue it names (or comment there
  with what happened, if the box is worded differently).
- When a check fails, comment on the issue with what you saw: the machines,
  what you pressed, and what happened instead.
- Some issues also want the result in a table in the docs. The check says
  where. Those tables and `docs/STREAMING-PLAN.md` §9 use GitHub numbers.
  Three-digit numbers elsewhere in those two docs are archived Gitea issues
  (Gitea #233 is GitHub #11, Gitea #255 is #21); the table at the end of this
  page maps the ones behind open checks.

Use a build of the current `master` on every machine. Caskade's data folder
is `~/.local/share/caskade` on Linux and `%APPDATA%\Caskade\Caskade\data` on
Windows. Sync and streaming checks need the devices paired in Settings →
Devices first, unless the check is about pairing.

## One Linux PC

About 1½ hours. Needs: a Linux PC with no Sunshine installed (L3–L5), a
debug build for L1 and L7 (`just dev`), and Sunshine installed by you for L6.

- [ ] **L1.** In a debug build, choose the pseudo-locale (`en-XA`) in
  Settings → General → Language. Go through every view in desktop and console
  mode, open the dialogs (game menu, Add game, editor, move, uninstall,
  share), a card's quick look in console mode (X on a pad) and a plugin's
  tab. **Pass:** every text is accented and bracketed; nothing is plain
  English. (#9)
- [ ] **L2.** Edit a game's details and artwork, select several games and use
  bulk edit and Refresh details, then restart Caskade and let it scan.
  **Pass:** your values, play sessions, installed games' folders and
  collections are all still there. (#8)
- [ ] **L3.** Settings → Streaming: turn hosting on and cancel the password
  prompt. Then turn it on again and allow it. **Pass:** each time there is
  exactly one prompt. After the cancel, hosting is off and
  `/etc/udev/rules.d` and `/etc/modules-load.d` are unchanged. After
  allowing, `60-caskade-streaming.rules` and `60-caskade-streaming.conf` are
  there, `ls -l /dev/uinput /dev/uhid` shows group `input`, `getfacl` shows
  your user on them, and `lsmod | grep uhid` lists uhid. (#21)
- [ ] **L4.** Put a file of your own in `/etc/udev/rules.d` first, then turn
  hosting off. **Pass:** one prompt; both Caskade files are gone, your file
  is still there, and `~/.local/share/caskade/streaming/host` holds no
  program. (#21, #31)
- [ ] **L5.** With hosting on, break one thing at a time and press Repair in
  Settings → Streaming: delete a file from the host's program folder; edit
  `sunshine.conf` under `streaming/host/config/sunshine/`; delete
  `/etc/udev/rules.d/60-caskade-streaming.rules`; delete the
  `dev.caskade.streaming` entry in your keyring (Seahorse). **Pass:** each
  Repair brings hosting back to ready; the rules case asks for the password
  once. (#31)
- [ ] **L6.** Install Sunshine the usual way (distro package or Flatpak), set
  its web user name and password, and add an app of your own in its web page.
  Turn hosting on in Caskade. **Pass:** Caskade offers to use it and installs
  nothing; a wrong password is refused and the right one works; Caskade's
  games appear in Sunshine's app list beside yours. Turn hosting off: only
  Caskade's apps go. (#24)
- [ ] **L7.** Open Settings → Plugins and install Playtime stats and the Ember
  theme from the published catalogue. It holds one version of each add-on, so
  the update needs a test catalogue: in a clean profile, point a debug build at
  a local one with a test key (`scripts/addons/README.md`) and install an
  add-on from it. Publish a newer release to it ("Update a package" in
  `docs/plugins/publishing.md`), press Check for updates and update. Then
  publish a release that can't start (a backend that isn't a component, as in
  `commands/addon_smoke_tests.rs`) and update to that. **Pass:** the published
  catalogue lists Playtime stats, GameMode and Ember, and both install; the
  good update installs; the failed one says the previous version is back,
  settings are kept, and an update never adds permissions without asking.
  (#8)

## One Windows PC

About 2 hours. Needs: Windows 10 or 11, a Windows user with admin rights and
a standard (non-admin) one, a game in the library, an Xbox controller, and no
Sunshine installed (W8 installs one). To find a game's id, open
`library.sqlite` in the data folder with any SQLite viewer:
`select id, title from games`.

- [ ] **W1.** Quit Caskade (tray → Quit). In a command prompt run
  `caskade.exe --launch <game id>`, play a minute, then quit the game. Do it
  again with Caskade already running. **Pass:** both times the game starts,
  the command returns when the game exits (`echo %ERRORLEVEL%` gives 0), and
  the game page shows the session. (#22)
- [ ] **W2.** Start a game with `caskade.exe --launch <game id>`, then end the
  caller with `taskkill /PID <pid>` (no `/F`; find the pid in Task Manager →
  Details with the Command line column). **Pass:** the game quits, its
  session ends, and the caller's exit code is 5. (#22)
- [ ] **W3.** Settings → Streaming: turn hosting on and cancel the UAC
  prompt; then turn it on and allow it. **Pass:** one prompt each time, naming
  Caskade. After the cancel, hosting is off, `sc query CaskadeStreaming` says
  there is no such service and
  `netsh advfirewall firewall show rule name="Caskade streaming (TCP)"` finds
  nothing. After allowing, `sc qc CaskadeStreaming` shows `DEMAND_START`, the
  standard user can `sc start` and `sc stop` it, and both firewall rules point
  at the host's `sunshine.exe`. (#21) On a PC that never had ViGEmBus, also run
  `sc query ViGEmBus` after allowing and comment on #26 what it says and whether
  an installer window showed: it should be running with none shown, but its
  silent install has never been tried.
- [ ] **W4.** With hosting on, look around Windows. **Pass:** no Start menu
  entry, tray icon or browser page for Sunshine; Services shows only
  "Caskade streaming"; firewall rules only "Caskade streaming (TCP)" and
  "(UDP)". `icacls "%ProgramFiles%\Caskade\Streaming\host"` gives Users read
  and execute only, and the standard user can't change or delete a file
  there. (#23)
- [ ] **W5.** Repeat W3's allow step signed in as a Windows user whose name
  has a space and an apostrophe (for example `Jo O'Neil`). **Pass:** hosting
  turns on. (#21)
- [ ] **W6.** Turn hosting off. **Pass:** one UAC prompt; afterwards there is
  no `CaskadeStreaming` service, no "Caskade streaming" firewall rule, and no
  program file under `%ProgramFiles%\Caskade\Streaming`, only `kept\`.
  (#21, #31)
- [ ] **W7.** With hosting on, break one thing at a time and press Repair:
  delete a firewall rule (`netsh advfirewall firewall delete rule
  name="Caskade streaming (UDP)"`), delete the service (`sc delete
  CaskadeStreaming`), delete a file under `%ProgramFiles%\Caskade\Streaming\host`.
  **Pass:** one UAC prompt puts back what's missing and hosting is ready
  again. (#31)
- [ ] **W8.** Install Sunshine from its own installer, set its web user name
  and password, and add an app of your own. Then do L6's steps. **Pass:** as
  L6. (#24)
- [ ] **W9.** Look over the game editor, selection and bulk edit, the
  achievements panel and the shell in this WebView2 build. **Pass:** a
  plugin's stylesheet loads, and scrolling or tabbing never pushes the shell
  out of the window. (Clears no box: #8 lets this stay deferred.)
- [ ] **W10.** Work through "Windows: a real XInput pad" in
  `docs/console-native-checklist.md`. **Pass:** every step as written; note
  whether the Xbox button brings Caskade to the front. (Clears no box: #8 lets
  this stay deferred.)

## One PC with your store accounts

About 1½ hours. Needs: a clean profile (a new OS user, or the data folder
moved aside), your Steam Web API key and profile, GOG and Epic accounts, a
RetroAchievements user name and web API key, the Steam client, and on
Windows GOG Galaxy and the Epic Games Launcher (they have no Linux builds).

- [ ] **A1.** Connect Steam, GOG and Epic in Settings → Store accounts.
  **Pass:** each connects, its owned games show under Library → Ready to
  Install, and each sign-in is in the OS keychain (Credential Manager or
  Seahorse), not in any file under the data folder. (#8)
- [ ] **A2.** Install an owned game from Ready to Install through its launcher
  (Steam; on Windows also GOG and Epic). Play it for a few minutes. **Pass:**
  once installed it joins the owned game (one game, not two), Play starts it,
  and the playtime matches the time played. (#8)
- [ ] **A3.** Turn on achievements in Settings → Store accounts and open games
  you have achievements in, including an emulated game for
  RetroAchievements. **Pass:** progress matches each store; hidden ones stay
  hidden until revealed; a private Steam profile shows as private; a game
  with none says so; RetroAchievements keeps hardcore and softcore apart.
  (#8)

## A PC with the OS in Swedish

About 1 hour. Needs: a fresh install (no profile) on a PC whose OS language
is Swedish. On Linux, the `sv_SE.UTF-8` locale must be installed.

- [ ] **SV1.** Go through first-run setup, both shells, every view, the tray
  menu, a notification (the games-added one after the first scan) and a core
  error (a wrong RomM address in setup). **Pass:** all of it is in Swedish.
  Game descriptions and OS error details may stay English. If you can, also
  check the tray and notifications on Windows and macOS. (#9)
- [ ] **SV2.** Switch Settings → General → Language to English. **Pass:**
  everything, the tray menu included, changes at once without a restart.
  (#9)

## A handheld, a TV and pads

About 1 hour. Needs: a Steam Deck (or a ROG Ally or Legion Go, on Linux or
Windows) with a fresh install, a desktop PC with a fresh install, and a
DualSense paired over Bluetooth to a Linux desktop.

- [ ] **H1.** Start Caskade for the first time on the handheld, then on the
  desktop. **Pass:** the handheld starts in console mode and Settings →
  Devices calls it a Handheld; the desktop starts in the desktop shell.
  Use a Windows handheld too if you have one: CI proves the registry reads on
  a virtual machine, not a real handheld's names. (#16)
- [ ] **H2.** Work through "Steam Deck game mode" in
  `docs/console-native-checklist.md`. **Pass:** every step as written. (Clears
  no box: #8 lets this stay deferred.)
- [ ] **H3.** Work through "Linux: a DualSense over Bluetooth" in the same
  file. **Pass:** every step as written. (Clears no box: #8 lets this stay
  deferred.)

## Two PCs on the same network

About 1½ hours, plus the copy. Needs: two paired PCs, Ethernet and Wi-Fi, a
large game (tens of GB) installed on one in a folder of its own or by
Caskade (not by a launcher), and a standard account with an age limit.

- [ ] **N1.** On the PC without the game, open its page and press Copy to
  this computer under On your other devices. Time it over Ethernet, then
  over Wi-Fi. **Pass:** the speed is close to the slower of the disks and
  the network, far above a relay's 1 MB/s. Write the figures in
  `docs/sync.md`, "Copying a game". (#11)
- [ ] **N2.** During a copy, unplug the cable (or turn Wi-Fi off), wait a
  minute, and plug it back in. **Pass:** the job in Activity says "Waiting
  for …", then carries on from where it was, not from zero. (#11)
- [ ] **N3.** On one PC, open a game installed on the other and press Play on
  … **Pass:** it shows "Started on …", the game runs on the other PC, it
  shows as running on both, and after you quit the session is on both. (#12)
- [ ] **N4.** Sign in the age-limited account on the target PC and ask for a
  game rated above its limit. **Pass:** the target asks for the admin PIN,
  and nothing starts until it's given. (#12, #20)

## Two PCs, internet unplugged

About 2 hours. Needs: two PCs on one router with the router's internet
cable unplugged. PC A has been online: an emulator installed through
Settings → Emulation → Get emulators, its Hasheous dumps and save manifest
downloaded. PC B is a fresh install that has never been online. For O7, both
need the same older release of a game installed from a link (a GitHub
release, say), with A updated to the newer one while online.

- [ ] **O1.** On B's first start, choose Set up from another device and pair
  with A by code. **Pass:** the first sync finishes; B has A's library,
  accounts (it asks for the admin's PIN if there is one) and collections,
  and keeps its own storage folder, emulators and settings. Fill in the table
  in `docs/sync.md`, "Setting up a new device". (#15)
- [ ] **O2.** After O3 (an emulator's dump comes once it's installed), on B,
  still never online, look in the cache folder's `hasheous/dumps/`
  (`~/.cache/caskade` on Linux, `%LOCALAPPDATA%\Caskade\Caskade\cache` on
  Windows), at Save locations list in Settings → Devices → Saves, and at a
  game's Saves tab in its editor. **Pass:** the dumps' `.zip` files are there
  (no screen in the app lists them), the list doesn't say "Not downloaded
  yet", and the tab names save locations, all from A. (#34)
- [ ] **O3.** On B, install the same emulator through Get emulators. **Pass:**
  it installs, from A. (#34)
- [ ] **O4.** Take two PCs that aren't paired (or Forget the pairing on both).
  Open Settings → Devices on both, pair by code, then edit a game, make a
  collection and play a game for a minute on each side, and copy a game.
  **Pass:** each lists the other as nearby within seconds, the link shows the
  local network, every change arrives the other way, and the copy finishes.
  Fill in `docs/sync.md`, "Two machines on one network, internet unplugged".
  (#10, #20)
- [ ] **O5.** Turn on Connect only directly in Settings → Devices on one PC
  (#10's box calls it Never use relays) and plug the internet back in.
  **Pass:** the link stays on the local network. (#10)
- [ ] **O6.** Block local discovery (a guest Wi-Fi that blocks multicast, or
  a firewall rule dropping UDP port 5353 on one PC) and pair with a pairing
  link instead. **Pass:** they pair and sync. (#10)
- [ ] **O7.** Unplug the internet. On B, press Sync now in Settings → Devices,
  then update the game A already updated. **Pass:** the update installs, from
  A. Fill in `docs/sync.md`, "Downloads from a device nearby". (#13)

## A Windows PC and a Linux PC

About 1 hour, plus the rest of WL3. Needs: the two PCs paired,
and on Windows a game in a folder of its own that keeps its saves in a known
place (one the Ludusavi manifest lists).

- [ ] **WL1.** On Windows, add the game with Add game → Add a game on this
  computer… On Linux, copy it with Copy to this computer and press Play.
  **Pass:** it starts through Proton. (#11, #20)
- [ ] **WL2.** Play on Windows, make progress and quit; start it on Linux.
  Then play on Linux, quit, and start it on Windows. **Pass:** each side
  starts with the other's save. Fill in the table in `docs/sync.md`,
  "Saves → Checks". (#14, #20)
- [ ] **WL3.** Fill in the Linux ↔ Windows row of `docs/sync.md`'s "Manual
  checklist" (steps 1–6; most are N, O, WK and T checks here). (#19)

## Streaming: a host and a client on one network

About 4 hours. Needs: a host PC that never had Sunshine (Linux, then
Windows), a client device (another PC and the Steam Deck), a pad with
rumble on the client, a TV for S12, and for single checks: Moonlight
installed by you on the client with a host added (S8), Discord running on
the client (S8), a 1440p Windows host with a 1080p client (S10), and
ViGEmBus installed on a Windows host beforehand (S11). Record each result
in `docs/STREAMING-PLAN.md` §9 too.

- [ ] **S1.** On the clean host, turn hosting on in Settings → Streaming.
  **Pass:** one prompt; no `~/.config/sunshine`, menu entry, tray icon or
  browser page appears (on Windows, W4 covers the rest). Do it on a clean
  Linux host and a clean Windows host. (#23)
- [ ] **S2.** On the client, Settings → Streaming → Pair with a device. Type a
  wrong code on the host first, then the right one. Start another pairing and
  leave it unanswered. **Pass:** no Moonlight or Sunshine window on either
  side; the wrong code says so and can be retried; the right one pairs; the
  unanswered request is gone from the host after five minutes. (#28, #30)
- [ ] **S3.** On the client, press Stream from <device> on a game page, then
  from the Play button and the game menu. Do it on each desktop you have
  (Cinnamon, GNOME, KDE, Windows, gamescope on the Deck). **Pass:** the
  first time shows "Getting streaming ready"; the game plays on the host and
  fills the client's screen; no Moonlight window shows before or after.
  (#27, #29, #23)
- [ ] **S4.** During a stream, press the guide button and choose End stream.
  **Pass:** the client is back in Caskade, the game stops on the host, and the
  host recorded a session. (#25, #29)
- [ ] **S5.** Stream for ten minutes. **Pass:** the host's playtime for the
  game grew by about ten minutes; the client's didn't. (#29)
- [ ] **S6.** Try to stream when: the host's Caskade is quit; the host has
  forgotten this device (Settings → Streaming on the host); the game is
  uninstalled on the host. **Pass:** each shows Caskade's own error, and no
  Moonlight process is left (`pgrep -fa moonlight`, or Task Manager). (#29)
- [ ] **S7.** On the host, close Caskade to the tray, then Quit it; on Linux
  also `kill -9` it. **Pass:** with the window closed to the tray the client
  can still stream; after Quit or the kill the host is gone. (#23)
- [ ] **S8.** On a client with your own Moonlight, stream through Caskade.
  **Pass:** your Moonlight still has its hosts and settings, and no toast,
  overlay or Discord status names Moonlight. If Caskade's client is the
  Flatpak, its state is in `~/.var/app/com.moonlight_stream.Moonlight/caskade`.
  (#27)
- [ ] **S9.** Play with the client's pad on a Linux host (an X11 and a Wayland
  session) and a Windows host. **Pass:** the pad drives the game, with
  rumble; on Wayland the screen-sharing approval is asked once. (#26)
- [ ] **S10.** Stream from the 1440p Windows host to the 1080p client.
  **Pass:** the stream is 1080p, and the host's screen mode comes back
  afterwards (HDR too, on an HDR screen). (#26)
- [ ] **S11.** On a Windows host that already had ViGEmBus, turn hosting on
  and stream with a pad. **Pass:** ViGEmBus stays installed and the pad
  works. (#26)
- [ ] **S12.** Ask someone new to it to turn hosting on, pair a device and
  stream a game. Then do the same with a pad in console mode on the TV,
  including the pairing prompt. **Pass:** they never leave Caskade and never
  see "Sunshine" or "Moonlight". (#30)
- [ ] **S13.** Turn hosting off, then on again. **Pass:** the host
  reinstalls, and the client paired before streams with no new code. (#31)
- [ ] **S14.** When Settings → Streaming offers an update for either program,
  press Update. **Pass:** pairings and settings stay; on Windows it takes one
  UAC prompt. (#31)
- [ ] **S15.** With two PCs paired in Settings → Devices (and not yet for
  streaming), pair them for streaming. **Pass:** they pair with no code
  typed. (#32)

## A PC that can sleep (Wake-on-LAN)

About 30 minutes. Needs: a desktop with Wake-on-LAN on in its firmware and
network settings, paired with another device and synced once while awake.
For WK2 it also hosts streaming and is paired for it.

- [ ] **WK1.** Put the desktop to sleep. On the other device, open a game
  installed on the desktop and press Wake and play on … Repeat with an
  age-limited account signed in on the desktop. **Pass:** it shows
  "Waking …", the desktop wakes, the game starts and shows as running on
  both; with the age-limited account the PIN is asked on the desktop first.
  Fill in `docs/sync.md`, "Waking a device to play". (#12, #20)
- [ ] **WK2.** Put the host to sleep and press Stream from <device>.
  **Pass:** the host wakes and the stream starts. (#29)

## Two networks (a phone hotspot)

About 1 hour. Needs: one PC on the home network, another on a phone's
hotspot. For T3, the two PCs paired.

- [ ] **T1.** From the game menu choose Share with someone…, and open the link
  on the other PC (or Add game → Receive a shared game…). **Pass:** the
  game installs and plays. If it says it's waiting for a direct connection
  instead, note it: the routers didn't allow a direct path. (#17, #20)
- [ ] **T2.** Share again, and while it copies, Stop sharing (Settings →
  Devices → Copies and shares). **Pass:** the copy fails at once, and the
  link is refused after. (#17, #20)
- [ ] **T3.** With Connect only directly on, check how Settings → Devices
  shows the other PC. Turn it off; if the other PC shows as reached through a relay,
  press Copy to this computer. **Pass:** with it on, the link is direct or
  offline, never relay. With it off, a copy over a relay waits ("Only a relay
  reaches …") and moves nothing. (#11, #20)

## macOS

About 2 hours. Needs: a Mac, plus the Linux and Windows PCs.

- [ ] **M1.** Fill in the Linux ↔ macOS and Windows ↔ macOS rows of
  `docs/sync.md`'s "Manual checklist". (#19)
- [ ] **M2.** Do W9's look-over in the Mac app (WKWebView). **Pass:** as W9.
  (Clears no box: #8 lets this stay deferred.)
- [ ] **M3.** Use the Mac as a streaming client: S2, S3 and S8. **Pass:** as
  those. (#27, #28, #29)

## Boxes no check here covers

Tick or decide these on the issue itself:

- #8: "All 30 implementation issues…" can be ticked: all 30 are ticked in the
  issue, and #6 and #7 closed on 4 Nov. "Record offline, rate-limited…
  operations" is in the issue's 30 Oct comments, and "Confirm no backlog… was
  added" in eng1's audit there.
- #10: boxes 2–4 were met by tests (the 2 Nov comment); O5 and O6 repeat
  them on real machines.
- #20: "Every rule holds with the webview bypassed" is met by the adapter
  tests listed in its 3 Nov comment.
- #21: "No webview-supplied string reaches the elevated process" is met by the
  tests listed in its evidence comment.
- #31: "Repair fixes each listed breakage in a test" is
  `host::tests::repair_fixes_each_breakage`; L5 and W7 try it for real.
- Trackers #8, #20 and #33: their sub-issue boxes tick when the sub-issues
  close.
- Gitea #262 (finding hosts): it has no GitHub issue, so its check in
  `docs/STREAMING-PLAN.md` §9 clears no box and isn't listed on this page.

## Which checks finish each issue

| Issue | Gitea | Checks |
| --- | --- | --- |
| #8 Library, store and plugin milestone | 194 | A1, A2, A3, L2, L7 |
| #9 Localization | 221 | L1, SV1, SV2 |
| #10 Local network first | 225 | O4, O5, O6 |
| #11 Copy a game | 233 | N1, N2, WL1, T3 |
| #12 Play on another device | 234 | N3, N4, WK1 |
| #13 Downloads from a device nearby (closed; optional) | 236 | O7 |
| #14 Save versions and sync | 239 | WL2 |
| #15 Set up a new device (closed; optional) | 242 | O1 |
| #16 Per-device settings (closed; optional) | 244 | H1 |
| #17 Share a game once | 246 | T1, T2 |
| #19 Sync on every platform | 250 | WL3, M1 |
| #20 Sync milestone | 252 | O4, WL1, WL2, N4, WK1, T1, T2, T3 |
| #21 Administrator steps | 255 | L3, L4, W3, W5, W6 |
| #22 `caskade --launch` | 256 | W1, W2 |
| #23 Hidden host | 257 | S1, W4, S3, S7 |
| #24 The user's own Sunshine (closed; optional) | 258 | L6, W8 |
| #25 Games on the host | 259 | S4 |
| #26 Display, pads, capture | 260 | S9, S10, S11 |
| #27 Client on first stream | 261 | S3, S8, M3 |
| #28 Pairing with a code | 263 | S2, M3 |
| #29 Stream from Play | 264 | S3, S4, S5, S6, WK2, M3 |
| #30 Settings → Streaming (closed; optional) | 265 | S2, S12 |
| #31 Update, repair, remove | 266 | L4, L5, W6, W7, S13, S14 |
| #32 Pairing through sync (closed; optional) | 267 | S15 |
| #33 Streaming milestone | 268 | closes with #21–#32 |
| #34 Nearby downloads for emulators and data (closed; optional) | 277 | O2, O3 |

Issues marked closed were met by the tests their Validation sections ask for,
so their checks now only confirm them on real machines.

`scripts/lan-unplugged.sh` runs O1 and most of O4 at the core on one PC, in
network namespaces with no uplink. It needs no root and doesn't replace two
real machines.

W9, W10, H2, H3 and M2 finish no issue: #8's keyboard, mouse and controller box
lets native checks stay deferred, so they only look for regressions on real
hardware.
