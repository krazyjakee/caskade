# Manual checks still open

These are the checks that tests can't do: real machines, real networks, real
accounts and real hardware. Each one closes an unchecked box on a GitHub
issue, unless it says it clears no box or its issue is marked closed in the
table at the end, which makes it optional. They are grouped by what you need set up, simplest first.

How to report:

- When a check passes, tick its box on the issue it names (or comment there
  with what happened, if the box is worded differently).
- When a check fails, comment on the issue with what you saw: the machines,
  what you pressed, and what happened instead.
- Some checks also want the result in a table. The check says which; the
  tables are under "Procedures and result tables" at the end of this page.
  Three-digit issue numbers are archived Gitea issues (Gitea #233 is GitHub
  #11, Gitea #255 is #21); the "Which checks finish each issue" table maps
  the ones behind open checks.

Use a build of the current `master` on every machine. Caskade's data folder
is `~/.local/share/caskade` on Linux and `%APPDATA%\Caskade\Caskade\data` on
Windows. Sync and streaming checks need the devices paired in Settings →
Devices first, unless the check is about pairing.

## What ran in virtual machines (6 Nov 2026)

These ran on one PC, in KVM guests built from `master`: two Ubuntu 24.04 cloud
images and Windows Server 2022 Evaluation, on a QEMU multicast LAN with the
default route pointing at a dead gateway. Real here: kernels, udev, polkit's
desktop dialog, UAC, the Windows service and firewall, Sunshine, Moonlight,
`caskade --launch`, `/dev/uinput`, ACPI S3 sleep. Not real: GPUs (libx264 and
software decoding), Wi-Fi, routers, sound on Windows, physical pads (a
`uinput` pad on the client). Screens were Xvfb on Linux with Openbox as the
window manager, not Cinnamon, GNOME or KDE. A kernel without
`linux-modules-extra` has no `uhid`, so turning hosting on fails cleanly
there: the administrator step undoes itself and hosting stays off.

These runs are enough for the issues closed on them on 6 Nov: #10, #12, #21,
#22, #23, #25 and #31. Each closing comment gives its limits, and those
issues' checks below are optional on real hardware. The table at the end lists
the checks each open issue needs. The boxes on this page stay for real
machines, so a result here doesn't tick one.

| Check | Result |
| --- | --- |
| L3 | Passed: one dialog; cancel left `/etc` unchanged and said so; allowing gave the rules, module and groups. `getfacl` shows no user entry, as there's no seat over SSH |
| L4 | Passed: one dialog, both files gone, a file of our own kept, no program left |
| L5 | Passed, all four breakages; only the deleted rules asked for a password |
| W1 | Passed, Caskade running and not running, exit 0, sessions recorded |
| W2 | Passed: `WM_CLOSE` to the hidden window and a real Sunshine stop both gave exit 5 and stopped the game |
| W3 | Passed: one UAC prompt each way. A standard user can stop the service but can't change its files. ViGEmBus's installer ran silently and refused Windows Server, so the driver is still untried on a client edition |
| W4 | Passed |
| W5 | Passed as `Jo O'Neil` |
| W6 | Passed: one prompt, only `kept\` left |
| W7 | Passed: firewall rule, service and host file, one prompt each |
| O4 | Passed with two VMs, no internet: nearby, code, link `lan`, library and sessions both ways |
| N1 | Finished, hashes equal, 3 GB. About 11 MB/s on a link TCP measured at 77 MB/s, in a debug build on a loaded PC, so this doesn't pass; see "Copying a game (#11)" below for release figures |
| N2 | Passed: held at 100 MB for 60 s, carried on from there. "Waiting for…" wasn't seen |
| N3 | Passed: "Started on lanhost", session on both |
| WK1 | Passed with real S3 sleep and a real magic packet. A script on the PC stood in for the NIC: it read the frame off the LAN and woke the VM |
| WK2 | Passed the same way, streaming within 7 s of Stream |
| S1 | Passed on Linux and Windows |
| S2 | Passed for Linux → Linux, Linux → Windows and Windows → Linux, a wrong code included. The unanswered request wasn't timed |
| S3 | Linux host to Linux client, Windows host to Linux client, Linux host to Windows client. Page and Play button; the game menu wasn't tried. Openbox only |
| S4 | Passed on Linux hosts and a Windows host, from a Linux client's guide button. No pad on the Windows client, so End stream is untried there |
| S5 | Passed: 719 s on the host, none on the client until sync carried the host's session |
| S7 | Passed for `kill -9` and for the service restarting. Without a tray in the VM, closing to the tray wasn't tried |
| S8 | Passed on Linux: the user's file unchanged, no Moonlight page or toast in frame-by-frame capture |
| S9 | Passed on Linux with a `uinput` pad: buttons and rumble both ways, X11 only. Windows needs ViGEmBus |
| S13 | Passed |

Fixed since: a host no longer lists itself under Play from other devices; a
link that has just dropped no longer shows "Internet, through a server";
"Ready" no longer shows above the public-network warning; the polkit dialog no
longer prints the script's first lines (L3 looks at it). A system clock that
stepped while a game ran left Stop unable to match its process by start time,
retrying until restart (seen after a VM paused for hours, whose clock then jumped); fixed since:
`tracking::ProcessIdentity`, and `tests/clock_step.rs` steps a real clock in CI.

## What ran on 7 Nov 2026

The same PC, the two Ubuntu guests and Windows Server, in release builds, plus
Windows 10 Enterprise Evaluation (a client edition, 90 days, `win10.qcow2` in
the VM lab) and, for the shares, a GitHub runner in a data centre. Enough for
the issues closed on them: #9, #11, #14, #17 and #26.

| Check | Result |
| --- | --- |
| WL1 | Passed: added by hand on Windows, copied to Linux, started through real Proton on Play |
| WL2 | Passed both ways and back, all four save folders, the prefix built on first launch |
| N1 | 8.1 GiB. Windows to Linux 23 MB/s, the guest's UDP limit (19 MB/s bare); Linux to Linux 60 MB/s on a link TCP measured at 92 MB/s. A QEMU LAN, not Ethernet or Wi-Fi; see "Copying a game (#11)" below |
| T1 | Passed between this PC behind its router and a GitHub runner, with n0's relays: `Direct`, 256 MiB in 26 s, and the program ran. Not a hotspot |
| T2 | Passed the same way: stopped at 20 MB, failed with the reason in seconds, link refused after |
| S9 | Windows 10 host, ViGEmBus: A and B reached XInput, and a game's rumble reached the client's pad at its own strength. A Linux host under Wayland (sway with no GPU) took the pad and sent rumble back, but its picture stayed black: wlroots' software renderer is all this VM has, so Wayland video and a desktop-portal approval (GNOME, KDE) are still unproved |
| S10 | Passed: a 1080p client took a 2560×1440 host to 1920×1080 at 60 Hz, and Sunshine reverted it when the game ended. The display was a virtual 60 Hz one (Virtual Display Driver), as QEMU's report 1 Hz and Sunshine's request fails on them. No HDR |
| S11 | Passed: hosting off left ViGEmBus running, hosting on again didn't reinstall it (Windows Installer's log shows one install), and the pad and rumble worked |
| W3's ViGEmBus note | Passed on a PC that never had it: installed silently in the one UAC prompt, running, no window |

| N4 | Passed: an unrated game asked of a limited account waited for the admin's PIN on the target, then started |
| WK1 | Passed with the limited account signed in on the sleeping host: real S3, real magic packet, the PIN asked there, then "Started on lanhost" |
| S3 | No client window before or after: the game menu and quick look on Cinnamon, the page on GNOME Shell and KDE Plasma (all X11 on Xvfb, llvmpipe), and quick look and page on a Windows 10 client. Frame-by-frame on each. Not every button on every desktop |
| S4 | On a Windows 10 client too, with a pad alone: stream, guide, End stream, the game stopped on the host. The guide here was Select + Start on a virtual XInput pad; the Xbox button itself wasn't tried |
| SV1 | Passed: a user with `sv_SE.UTF-8` in a Cinnamon session, fresh install: setup, both shells, every view, all 17 Settings tabs in console mode, tray, the feed's notification, a core error |
| SV2 | Passed: English at once, tray included, no restart |

Found and fixed: iroh-blobs' handler shut the whole copy store down with its
router, so after the first transfer through two routers, or a change to Connect
only directly, a device served no more copies or shares until it restarted.
Windows' add-game dialog said a Windows program runs through a compatibility
tool. A full disk named what was short in bytes. Games from a streaming host
were listed in the notification under their raw source id. Under GNOME the
client's window opened beneath Caskade's always-on-top one, and the Shell said
"Moonlight is ready". On Windows a firewall prompt named Moonlight, a pad did
nothing at all (Windows Gaming Input sends nothing behind WebView2, so gilrs now
uses XInput, and DualSense and other non-XInput pads need Steam Input or the
like there), the guide button never reached Caskade over a game, the guide menu
stayed under a stream, and helper programs flashed console windows. A paired
host's games waited up to ten minutes, and Look again didn't look.

`scripts/across-networks.sh` runs T1 and T2 at the core, sharing from a GitHub
runner (`.github/workflows/across-networks.yml`) so this PC's address stays out
of a public log.

## One Linux PC

About 1½ hours. Needs: a Linux PC with no Sunshine installed (L3–L5), a
debug build for L1 and L7 (`just dev`), and Sunshine installed by you for L6.

- [ ] **L1.** The core's language list has no pseudo-locale, so this needs the
  browser preview: run `VITE_CASKADE_FIXTURES=1 just web` and open a page with
  `?lang=en-XA`. Go through every view in desktop and console
  mode, open the dialogs (game menu, Add game, editor, move, uninstall,
  share), a card's quick look in console mode (X on a pad) and a plugin's
  tab. **Pass:** every text is accented and bracketed; nothing is plain
  English. (#9)
- [ ] **L2.** Edit a game's details and artwork, select several games and use
  bulk edit and Refresh details, then restart Caskade and let it scan.
  **Pass:** your values, play sessions, installed games' folders and
  collections are all still there. (#8)
- [ ] **L3.** Settings → Streaming: turn hosting on and dismiss the password
  prompt. Then turn it on again and answer it with your real password. **Pass:**
  each time there is exactly one prompt, which names Caskade game streaming
  and shows none of a script. Dismissing it reports a cancel ("Nothing changed:
  the administrator's permission wasn't given."), not an error or a refusal;
  hosting is off and
  `/etc/udev/rules.d` and `/etc/modules-load.d` are unchanged. After
  allowing, `60-caskade-streaming.rules` and `60-caskade-streaming.conf` are
  there, `ls -l /dev/uinput /dev/uhid` shows group `input`, `getfacl` shows
  your user on them, and `lsmod | grep uhid` lists uhid. The VM proof of
  today's pkexec command answered through a polkit rule, so a typed password
  and a dismissed prompt are this check's to confirm. (#21)
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
- [ ] **L8.** Idle memory, alongside the others: start an installed build
  (deb or AppImage) with your own library, leave it in the tray for an hour,
  then run
  `grep -E 'RssAnon|VmSwap' /proc/$(pgrep -xo caskade)/status`. **Pass:**
  RssAnon plus VmSwap is under 400 MB with a library the size of 8,400
  games. Record it in "Idle memory with a large library". (#71)
- [ ] **L9.** Startup, once in an X11 session and once in a Wayland one,
  with an installed build (deb or AppImage). Settings → General → Startup:
  turn on Launch on system startup, set Open to Library, log out and in.
  **Pass:** `~/.config/autostart/caskade.desktop` exists with
  `Exec=<caskade or the AppImage> --login`, Caskade starts at login and opens
  on the Library. Turn on Start minimized and log out and in again: the tray
  icon is there with no window; the tray's Show, or starting Caskade again,
  shows it. Set Open to Last opened view, open Collections and quit: the next
  start opens Collections. Turn Launch on system startup off: the file is gone
  and the next login starts nothing. (#114)
- [ ] **L10.** As L9 in a Flatpak build of Caskade (none is published yet).
  **Pass:** turning Launch on system startup on asks through the desktop's
  background permission once, and `~/.config/autostart/` gets an entry that
  runs `flatpak run … --login`; the rest as L9. (#114)

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
- [ ] **W2.** Start a stream of a game from a Windows host, or start one with
  `caskade.exe --launch <game id>` and post `WM_CLOSE` to the caller's hidden
  window. `taskkill /PID <pid>` without `/F` won't do: it reports success but
  doesn't reach hidden windows. Sunshine's own stop does reach them. **Pass:**
  the game quits, its session ends, and the caller's exit code is 5. (#22)
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
  On a network Windows calls Public, the firewall rules don't apply, so other
  devices can't connect, and Settings → Streaming says so. Set the network to
  Private: the warning goes within about a minute, with no Repair or restart.
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
- [ ] **W10.** Work through "Windows: a real XInput pad" under
  "Console mode on real hardware" below. **Pass:** every step as written; note
  whether the Xbox button brings Caskade to the front. (Clears no box: #8 lets
  this stay deferred.)
- [ ] **W11.** Startup, with the installed build. Turn on Launch on system
  startup and set Open to Collections; sign out and in. **Pass:**
  `reg query HKCU\Software\Microsoft\Windows\CurrentVersion\Run /v Caskade`
  shows `"<install folder>\caskade.exe" --login`, Task Manager → Startup apps
  lists Caskade, and it starts at sign-in on Collections. With Start minimized
  on too, the next sign-in leaves only the tray icon; a click on it, or
  starting Caskade again, shows the window. Turning Launch on system startup
  off deletes the value. (#114)

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
- [ ] **H2.** Work through "Steam Deck game mode" under
  "Console mode on real hardware" below. **Pass:** every step as written. (Clears
  no box: #8 lets this stay deferred.)
- [ ] **H3.** Work through "Linux: a DualSense over Bluetooth" in the same
  section. **Pass:** every step as written. (Clears no box: #8 lets this stay
  deferred.)

## Two PCs on the same network

About 1½ hours, plus the copy. Needs: two paired PCs, Ethernet and Wi-Fi, a
large game (tens of GB) installed on one in a folder of its own or by
Caskade (not by a launcher), and a standard account with an age limit.

- [ ] **N1.** On the PC without the game, open its page and press Copy to
  this computer under On your other devices. Time it over Ethernet, then
  over Wi-Fi. **Pass:** the speed is close to the slower of the disks and
  the network, far above a relay's 1 MB/s. Write the figures in
  "Copying a game (#11)" below. (#11)
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
  in "Setting up a new device (#15)" below. (#15)
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
  Fill in "Two machines on one network, internet unplugged" below.
  (#10, #20)
- [ ] **O5.** Turn on Connect only directly in Settings → Devices on one PC
  (#10's box calls it Never use relays) and plug the internet back in.
  **Pass:** the link stays on the local network. (#10)
- [ ] **O6.** Block local discovery (a guest Wi-Fi that blocks multicast, or
  a firewall rule dropping UDP port 5353 on one PC) and pair with a pairing
  link instead. **Pass:** they pair and sync. (#10)
- [ ] **O7.** Unplug the internet. On B, press Sync now in Settings → Devices,
  then update the game A already updated. **Pass:** the update installs, from
  A. Fill in "Downloads from a device nearby (#13)" below. (#13)

## A Windows PC and a Linux PC

About 1 hour, plus the rest of WL3. Needs: the two PCs paired,
and on Windows a game in a folder of its own that keeps its saves in a known
place (one the Ludusavi manifest lists).

- [ ] **WL1.** On Windows, add the game with Add game → Add a game on this
  computer… On Linux, copy it with Copy to this computer and press Play.
  **Pass:** it starts through Proton. (#11, #20)
- [ ] **WL2.** Play on Windows, make progress and quit; start it on Linux.
  Then play on Linux, quit, and start it on Windows. **Pass:** each side
  starts with the other's save. Fill in the table in
  "Saves between Windows and Linux (#14)" below. (#14, #20)
- [ ] **WL3.** Fill in the Linux ↔ Windows row of "Sync between real
  machines (#19)" below (steps 1–6; most are N, O, WK and T checks here). (#19)

## Streaming: a host and a client on one network

About 4 hours. Needs: a host PC that never had Sunshine (Linux, then
Windows), a client device (another PC and the Steam Deck), a pad with
rumble on the client, a TV for S12, and for single checks: Moonlight
installed by you on the client with a host added (S8), Discord running on
the client (S8), a 1440p Windows host with a 1080p client (S10), and
ViGEmBus installed on a Windows host beforehand (S11). Record each result
under "Streaming on real machines" below too.

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
  Fill in "Waking a device to play (#12)" below. (#12, #20)
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
  "Sync between real machines (#19)" below. (#19)
- [ ] **M2.** Do W9's look-over in the Mac app (WKWebView). **Pass:** as W9.
  (Clears no box: #8 lets this stay deferred.)
- [ ] **M3.** Use the Mac as a streaming client: S2, S3 and S8. **Pass:** as
  those. (#27, #28, #29)
- [ ] **M4.** Startup: turn on Launch on system startup and Start minimized,
  set Open to Library, log out and in. **Pass:**
  `~/Library/LaunchAgents/dev.caskade.app.login.plist` exists, System
  Settings → General → Login Items lists Caskade under Allow in the
  Background, and Caskade starts at login in the menu bar with no window; the
  menu's Show or a Dock click shows it, on the Library. Turning the setting
  off removes the file. (#114)

## Boxes no check here covers

Tick or decide these on the issue itself:

- #8: "All 30 implementation issues…" can be ticked: all 30 are ticked in the
  issue, and #6 and #7 closed on 4 Nov. "Record offline, rate-limited…
  operations" is in the issue's 30 Oct comments, and "Confirm no backlog… was
  added" in eng1's audit there.
- #10: nothing left to tick. It closed on 6 Nov with all four boxes ticked:
  box 1 on the virtual machine run and `lan_unplugged::two_devices_pair_sync_and_copy`,
  boxes 2–4 on the tests named at the end of the issue's body. O4–O6 repeat
  them on real machines and are optional.
- #20: "Every rule holds with the webview bypassed" is met by the adapter
  tests listed in its 3 Nov comment.
- #21: nothing left to tick. It closed on 6 Nov with all three boxes ticked.
  "No webview-supplied string reaches the elevated process" is met by
  `elevation::tests::the_linux_command_is_constants_and_step_names`,
  `the_windows_launcher_passes_names_and_caskades_own_paths` and
  `a_tampered_record_never_reaches_a_prompt`, as the issue's body says. Its
  2 Nov evidence comment also names
  `the_windows_elevated_side_takes_names_and_rolls_back`, which no longer
  exists: `a_changed_file_or_manifest_is_refused_and_a_failure_undoes_everything`
  now proves unknown steps and foreign folders run nothing. That comment's
  `/bin/sh -c` is now `/bin/sh -s`, with the script on standard input.
- #31: "Repair fixes each listed breakage in a test" is
  `host::tests::repair_fixes_each_breakage`; L5 and W7 try it for real.
- Trackers #8, #20 and #33: their sub-issue boxes tick when the sub-issues
  close.
- Gitea #262 (finding hosts): it has no GitHub issue, so its check under
  "Streaming on real machines" clears no box and isn't listed on this page.

## Which checks finish each issue

| Issue | Gitea | Checks |
| --- | --- | --- |
| #8 Library, store and plugin milestone | 194 | A1, A2, A3, L2, L7 |
| #9 Localization (closed; optional) | 221 | L1, SV1, SV2 |
| #10 Local network first (closed; optional) | 225 | O4, O5, O6 |
| #11 Copy a game (closed; optional) | 233 | N1, N2, WL1, T3 |
| #12 Play on another device (closed; optional) | 234 | N3, N4, WK1 |
| #13 Downloads from a device nearby (closed; optional) | 236 | O7 |
| #14 Save versions and sync (closed; optional) | 239 | WL2 |
| #15 Set up a new device (closed; optional) | 242 | O1 |
| #16 Per-device settings (closed; optional) | 244 | H1 |
| #17 Share a game once (closed; optional) | 246 | T1, T2 |
| #19 Sync on every platform | 250 | WL3, M1 |
| #20 Sync milestone | 252 | O4, WL1, WL2, N4, WK1, T1, T2, T3 |
| #21 Administrator steps (closed; optional) | 255 | L3, L4, W3, W5, W6 |
| #22 `caskade --launch` (closed; optional) | 256 | W1, W2 |
| #23 Hidden host (closed; optional) | 257 | S1, W4, S3, S7 |
| #24 The user's own Sunshine (closed; optional) | 258 | L6, W8 |
| #25 Games on the host (closed; optional) | 259 | S4 |
| #26 Display, pads, capture (closed; optional) | 260 | S9, S10, S11 |
| #27 Client on first stream | 261 | M3 (S3, S8 passed in VMs) |
| #28 Pairing with a code | 263 | M3 (S2 passed in VMs) |
| #29 Stream from Play | 264 | S3 on a Steam Deck, M3 (the rest passed in VMs) |
| #30 Settings → Streaming (closed; optional) | 265 | S2, S12 |
| #31 Update, repair, remove (closed; optional) | 266 | L4, L5, W6, W7, S13, S14 |
| #32 Pairing through sync (closed; optional) | 267 | S15 |
| #33 Streaming milestone | 268 | closes with #21–#32 |
| #34 Nearby downloads for emulators and data (closed; optional) | 277 | O2, O3 |
| #71 Idle memory with a large library | — | L8 |
| #114 Startup settings | — | L9, L10, W11, M4 |

Issues marked closed were met by the tests their Validation sections ask for
or by the virtual machine runs above, so their checks now only confirm them on
real machines.

`scripts/lan-unplugged.sh` runs O1 and most of O4 at the core on one PC, in
network namespaces with no uplink. `scripts/two-networks.sh` runs T1, T2, T3,
O5 and O6 the same way, behind router namespaces doing NAT with a relay between
them (results in git history, in the retired `docs/sync.md`). Neither needs root, and neither replaces real
machines, routers, Wi-Fi or a phone's hotspot.

W9, W10, H2, H3 and M2 finish no issue: #8's keyboard, mouse and controller box
lets native checks stay deferred, so they only look for regressions on real
hardware.

## Procedures and result tables

These came from the design plans retired on 8 Nov 2026 (git history keeps the plans). Checks above that say "fill in" a table record their result in the table here, as well as on the issue.

### Sync between real machines (#19)

CI covers what one machine can: two or more
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

### Two machines on one network, internet unplugged

The in-process tests share one host, so they can't show two routers' worth of
network. This check needs two machines on one network, with the router's
internet unplugged:

1. Start Caskade on both; open Settings → Devices or call the commands.
2. Each lists the other as nearby within a few seconds.
3. Pair them by code; the link state is `lan`.
4. Make an edit on one and see it on the other.
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
| 2026-11-07 | Windows Server 2022 and Ubuntu 24.04 in KVM, release builds | 1, 2, 4 | See below | No cable to pull between them here; the two Linux guests did that on 6 Nov. |

**Windows to Linux in virtual machines (2026-11-07).** The game was added by
hand in Windows' Add game dialog, copied with Copy to this computer and started
with Play: Proton built the prefix and ran it from `G:\Game` with its data
found. An 8.1 GiB game of one archive and a thousand small files, release builds
on both sides, on a QEMU LAN between KVM guests on one PC:

| From | To | Copy, steady | Whole job | That link, measured |
| --- | --- | --- | --- | --- |

| Linux | Linux | 60 MB/s | 2 min 40 s, then 80 s placing | TCP 92 MB/s; bare UDP 129 MB/s; disk write 174 MB/s |

QUIC sends what the guest's network card lets UDP send: Windows' copy ran at
that limit, so the emulated e1000e, not Caskade, set its speed. Linux reached
two thirds of the link with the PC's load around 10. Both are far above a
relay's 1 MB/s. A real network wasn't involved, and the sender's cache held part
of the Windows game; N1's figures on Ethernet and Wi-Fi are still worth having.
A game too big for the disk failed before moving a byte, naming what was short.

**Step 4 on one PC (2026-11-06).** `scripts/proton-real.sh` needs umu, umu's

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

### Saves between Windows and Linux (#14)

| Check | Result |
| --- | --- |
| Play on Windows, quit, start on a Linux device through Proton: the Windows save is there | 2026-11-06, one PC: passed under real Proton from a fixture Windows device, first launch included (the prefix didn't exist yet), and the game read the save. No Windows machine involved. |
| Play on the Linux device, quit, start on Windows: the Linux save is there | 2026-11-06, one PC: the version kept from the real prefix (four files, each once) restored to the fixture Windows device's folders, and nothing else. Reading it back on real Windows not yet run. |
| Windows and Linux on two machines (WL2) | 2026-11-07, two KVM guests: passed both ways with the app's own windows, real Windows Server 2022 and real Proton. `level=1` written on Windows, read under Proton on its first launch; `level=2` written under Proton, read on Windows; `level=3` read back under Proton. All four save folders followed. Not physical machines. |

### Setting up a new device (#15)

| Check | Result |
| --- | --- |
| A new device on a network with no internet set up from another: same library, accounts and collections; its own storage, emulators and settings | passed 2026-11-06 in `scripts/lan-unplugged.sh` (two processes in network namespaces, the core's steps, no setup screens); not yet run on two machines |
| Games chosen during setup copy from the other device over the local network (Gitea #276) | not yet run |

### Streaming on real machines

Each item names its GitHub issue; record the date and result on the issue.

VM runs on 6 and 7 Nov 2026 (Linux and Windows hosts, Windows 10 with ViGEmBus, Linux and Windows clients) are recorded above, in "What ran in virtual machines" and "What ran on 7 Nov 2026". Boxes below stay for real hardware.

- [ ] #21 Linux: switching hosting on shows one pkexec prompt, and cancelling it leaves `/etc/udev/rules.d` and `/etc/modules-load.d` unchanged. After allowing it, the rules and module list are there, `/dev/uinput` and `/dev/uhid` belong to group `input` with a seat ACL, and uhid is loaded. Switching hosting off removes exactly those, with one prompt.
- [ ] #21 Windows: one UAC prompt (Caskade, verified publisher); cancelling changes nothing (`sc query CaskadeStreaming` fails, `netsh advfirewall firewall show rule name="Caskade streaming (TCP)"` finds none). After allowing it, the service is demand-start, a standard user can `sc start` and `sc stop` it, and both firewall rules name the host's `sunshine.exe`. A host path with a space and an apostrophe works. Switching hosting off removes the service and both rules, with one prompt.
- [ ] #23 Linux, clean machine: switching hosting on shows one prompt, and Moonlight on another device pairs  and streams a game. No `~/.config/sunshine`, desktop entry, tray icon or browser page appears. Closing Caskade to the tray keeps the stream available; Quit ends it; killing Caskade (`kill -9`) ends the host too.
- [ ] #23 Windows, clean machine: the same with one UAC prompt. Services shows only "Caskade streaming", and firewall rules only "Caskade streaming (TCP/UDP)". `%ProgramFiles%\Caskade\Streaming\host` grants users read and execute only. A standard user can't change the host's files.
- [ ] #27 per OS: the first Stream from a device installs the client under "Getting streaming ready", then streams. A Moonlight the user installed keeps its hosts and settings afterwards. No warning toast, overlay or Discord presence names Moonlight during or after the stream. Flatpak: `flatpak run --cwd=` keeps the state in `~/.var/app/com.moonlight_stream.Moonlight/caskade`.
- [ ] Gitea #262 (Gitea closed it and it has no GitHub issue, so this clears no box): a host on the LAN appears in `streaming_hosts` within seconds, and shows unreachable after it leaves the network. After pairing (#28), its games show as "Stream from <device>" on the matching library games.
- [ ] #24 per OS: with Sunshine installed the usual way, switching hosting on offers to use it, and the right password works, a wrong one doesn't. Caskade's games appear in its apps beside the user's; switching off removes only Caskade's.
- [ ] #25 per OS: a client lists the host's games with their covers, plus "Caskade". Starting one plays it on the host with a session there, and quitting the stream stops it. Installing, uninstalling, hiding or renaming a game changes the client's list after a refresh.
- [ ] #28 per OS pair: Pair with a device shows a code; entering it on the host's Caskade pairs with no window from either program. A wrong code says so and can be tried again; an unanswered request disappears from the host after five minutes.
- [ ] #29 per OS pair, the Steam Deck as a client included: Stream from <device> on the game page, the Play button and the game menu starts the stream with no Moonlight window in sight before or after (Cinnamon, GNOME, KDE, Windows, macOS, gamescope). The pad plays the game; guide brings up the quick menu and End stream returns to Caskade with the game stopped on the host. A host asleep wakes; one switched off, one that forgot this device and a game it no longer offers each show Caskade's error with no Moonlight process left. The host's playtime grows and the client's doesn't.
- [ ] #30 per OS: a new user switches hosting on, pairs a device and streams a game without leaving Caskade or seeing a technology name; the same with a pad in console mode on the TV, the request prompt included.
- [ ] #31 per OS: after switching hosting off, nothing of the host remains: Linux has no `/etc/udev/rules.d/60-caskade-streaming.rules` or `/etc/modules-load.d/60-caskade-streaming.conf`, and no program under `<data>/streaming/host`. Windows has no `CaskadeStreaming` service, no "Caskade streaming" firewall rule, and no program file under `%ProgramFiles%\Caskade\Streaming` (only `kept\`). Switching on again reinstalls it, and a device paired before streams with no new code. An update of either program from Settings → Streaming keeps pairings and settings. On Windows it applies with one UAC prompt into the administrator-only folder. Each repair case works on a real machine.
- [ ] #26: a 1080p client streaming from a 1440p Windows host gets a 1080p stream, and the host's mode comes back afterwards (HDR too, on an HDR screen). A pad on the client drives the game on a Linux host (X11 and Wayland, the portal's approval asked once) and on a Windows host, with rumble. A Windows host where ViGEmBus was already installed keeps it and works, and one without gets it in the same UAC prompt (`/exenoui /qn /norestart`; W3 above ran the installer silently on Windows Server, where it refuses, so installing it needs a client edition). A host with no screen, or whose pads don't work, says so in Settings → Streaming.
- [ ] #32: two of the household's devices, paired in Settings → Devices, pair for streaming with no code typed. A host on a synced device names each streamed game as that device's installed copy.
- [ ] #22 Windows: with Caskade running, `caskade.exe --launch <game id>` starts the game; `taskkill /PID <caller>` (WM_CLOSE, without `/F`), or ending a Sunshine stream, quits the game and ends its session, and the caller exits 5.

### Console mode on real hardware (#153)

CI proves console mode with a synthetic pad (`apps/desktop/src-tauri/tests/console_e2e.rs`, under Xvfb). These need real hardware: they are W10, H2 and H3 above. Issue numbers in this section are archived Gitea issues.

Build: `pnpm tauri build --debug` (the AppImage or deb), or `just dev` on the device.

Build: `pnpm tauri build --debug` (the AppImage or deb), or `just dev` on the device.

#### Steam Deck game mode (gamescope)

Install the AppImage on the Deck. In desktop mode, add it to Steam as a non-Steam
game, then switch to game mode and launch it from the library.

1. **Starts in console mode.** It opens fullscreen in console mode, with no
   `--console` flag: Settings → General → Start in is **Automatic**, and
   gamescope is detected. Set **Start in** to **Desktop**, relaunch, and check
   it now opens on the desktop shell.
2. **Steam Input on** (the default for non-Steam games). The d-pad and left stick
   move focus. A selects, B goes back, X opens a card's details, and Y goes to
   the search. LB/RB page Home's shelves and switch Settings tabs, and LT/RT
   scroll a long list by a screen. The hint bar shows Xbox glyphs.
3. **Steam Input off** (the controller settings for Caskade → Disable Steam
   Input). Repeat 2. The pad is read directly, and the glyphs follow the
   Deck's own pad.
4. **Guide.** Launch a game from Caskade. Press the Steam button (or Select +
   Start, the guide chord) during play. Caskade comes to the front with the
   quick menu. **Return to game** puts the game back in front. Do it again and
   choose **Stop game**: the game ends, and its page shows the new session's
   playtime. Note whether gamescope or Steam itself takes the Steam button
   first.
5. **Fullscreen.** Console mode fills the screen with no window chrome. Press
   **View** to go to desktop mode, then back. In desktop mode, the gamepad
   button next to maximize returns to console mode.
6. **On-screen keyboard.** Press Y to focus the search, and Steam + X opens
   the keyboard. Type, press down for the suggestions, then A. Settings →
   Plugins in console mode takes a typed folder path the same way, since there
   is no folder picker.
7. **Suspend and resume.** Suspend the Deck from the power menu with Caskade in
   front, then resume. The pad still drives it, focus is where it was, and the
   clock in the hint bar is current.

#### Windows: a real XInput pad

On Windows 10 or 11 with an Xbox controller (USB or Bluetooth):

1. Start Caskade and press **View** on the pad: it switches to console mode,
   fullscreen. The glyphs are Xbox's.
2. Repeat Steam Deck step 2 without Steam.
3. Guide: launch a game, then press the Xbox button (or Select + Start, which
   a VM has shown working). Caskade comes to the front, or flashes in the
   taskbar if Windows refuses focus. Note which, and whether the Game Bar takes
   the button first. Check Return to game and Stop
   game as in Steam Deck step 4.
4. Close the window while in console mode, then reopen Caskade. The desktop
   window has its old size and position (window-state), and with **Start in**
   set to **Automatic** it opens in console mode, the mode it was left in.

#### Linux: a DualSense over Bluetooth

On a desktop Linux session (GNOME or KDE), pair the controller over Bluetooth:

1. `caskade --console` opens in console mode. The hint bar shows PlayStation
   glyphs (✕ ○ □ △, L1/R1).
2. Repeat Steam Deck step 2. ✕ selects and ○ goes back.
3. The PS button summons Caskade over a running game (Steam Deck step 4). On
   Wayland, if the compositor refuses focus, Caskade asks for attention
   instead: note which.
4. Disconnect the controller with Caskade in front, then reconnect it. Input
   resumes without a restart.

#### What each check covers

The commits that implement what each check exercises. Test at or after the
newest one listed.

| Check | Commits |
|---|---|
| Starting in console mode, gamescope detection, Start in | `b6f3f3c` (with `3dee3f5`, which carried `commands/console.rs`) |
| Pad navigation, Steam Input on and off, hot plug | `dae8c00` (#146, gilrs input), `a687963` (#147, focus engine) |
| Hint bar glyphs per pad family, LB/RB and LT/RT paging | `b6f3f3c` (glyphs from `3dee3f5`) |
| Guide, the quick menu, Return to game, Stop game | `62b0387` (#149) |
| Fullscreen, View and the console button, window state on exit | `b6f3f3c` |
| Search by Y with the OS keyboard, typed plugin folder | `b6f3f3c` |
| Suspend and resume (input service, the hint bar's clock) | `dae8c00`, `b6f3f3c` |
| Frame rate on NVIDIA under X11 | `bdb6460` |

### Idle memory with a large library (#71)

The budget is the main `caskade` process's anonymous memory, RssAnon plus
VmSwap from `/proc/<pid>/status`, after an hour idle: under 400 MB with
8,400 games. The WebKit web process is separate and isn't counted.
`WebKitWebProcess` was about 270 MB in the report that opened #71.

To find what holds memory, run a copy of the library under Xvfb with
jemalloc's profiler (`LD_PRELOAD=/usr/lib/x86_64-linux-gnu/libjemalloc.so.2
MALLOC_CONF=prof:true,prof_accum:true,lg_prof_interval:30,prof_prefix:<dir>/jeprof`)
and symbolize its dumps against `/proc/<pid>/maps` with `addr2line`. Delete
the copy afterwards: its keyring holds your sign-ins.

| Build | Where | Result |
| --- | --- | --- |
| 0.1.2 (deb) | Linux Mint 22.3, NVIDIA, X11, 8,439 games | 1.65–1.71 GB for 14 hours, 60 % of it swapped out |
| `9d786ab` (debug) | copy of the same library under Xvfb, glibc, 100 minutes | 337 MB at start; from 10 minutes on 209–315 MB, mean 226 MB; 105 threads |
| `cacf049` (debug) | the same copy restarted, 20 minutes | 152–155 MB between discovery's 5-minute polls, 178–231 MB for the 40 s after each |
