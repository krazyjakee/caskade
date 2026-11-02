# Caskade — Game Streaming Plan

Status: planned; the managed-tool layer (§3.1, #254), `caskade --launch` (§8.1, #256) and the elevation helper (§8.4, #255) are built; issues #254–#267 in the Game streaming milestone, tracked in #268 · Last updated: 2026-10-30

How Caskade lets a household play a game installed on one device from another, streamed over the local network. Caskade installs, configures, updates and drives two separate open-source programs: Sunshine on the device that runs the game (the host) and Moonlight on the device that shows it (the client). Users never see their names, windows or settings: they switch hosting on, pair a device once, and choose Stream from Living room. It follows the conventions in `TECH-PLAN.md`; the managed installs build on the emulator setup (`crates/caskade-sources/emulation/README.md`).

The facts about Sunshine and Moonlight below were checked against their sources on 2026-10-30: Sunshine `v2026.914.233613` (2026-09-15) and moonlight-qt `v6.2.0` (2026-10-04). Items marked *unverified* are read from code but not yet tried.

---

## 1. Goals and non-goals

Goals:

- **Invisible.** No Sunshine or Moonlight name, icon, shortcut, tray icon, web page, dialog or setting reaches the user. Caskade's own words are "streaming", "Let other devices stream games from this one", "Stream from <device>".
- **Minimal.** Hosting is one switch, with one administrator prompt when it is turned on. A client downloads its half the first time the user streams, without asking. Pairing is one code, once per pair of devices, and none once the devices are paired through sync.
- **Library-first.** Games installed on a host appear on the client as Stream from <device> choices on the game's own page and Play button, beside the local copy and the store's install. Starting one goes through Caskade's launch pipeline on both ends, so sessions, playtime, scripts, age limits and console mode behave as for a local launch.
- **Managed like emulators.** Detection, one-click install, update notification with one-click Update and Skip, repair and removal reuse the managed-install machinery.
- **Works offline.** Everything runs on the local network; no internet is needed once both programs are installed.

Non-goals:

- **Linking either program.** Both are GPLv3 and Caskade is MIT: they stay separate processes, downloaded from their upstream releases, never patched. Rust rewrites were reviewed and set aside (Moonshine is Linux-only; every Moonlight client library is GPLv3).
- **Streaming over the internet.** The protocol is not fully encrypted at the application level; streaming stays on the local network. Remote play through sync's relays is out of scope.
- **macOS hosts** in the first version: Sunshine's macOS support is experimental and has no gamepads. macOS clients are in scope.
- **Apollo or Artemis.** Sunshine runs on every host OS, Windows included (decided 2026-10-30).
- **A virtual display on the host.** Sunshine has none yet (open PR #5114). On Windows it changes the real display's mode to match the client (§4.4).

## 2. Decisions taken

- **Sunshine and Moonlight as separate programs**, managed by Caskade and hidden from users.
- **Sunshine everywhere**, not Apollo on Windows.
- **Standalone first.** Clients find hosts on the local network and pair with a code shown on the client and entered in Caskade on the host. Once the Devices, sync and sharing milestone pairs devices, paired devices pair streaming with no code (§6.3).
- **Hosting is opt-in per device.** Turning it on installs and configures the host, with one administrator prompt. Clients install Moonlight only when the user first streams.
- **The host runs only while Caskade runs.** Quitting Caskade stops it; a window closed to the tray keeps it. (Decided 2026-10-30.)
- **Pairings survive turning hosting off and on.** (Decided 2026-10-30.)
- **Streamed play counts on the host only.** The host's Caskade records the session and playtime under its active account, as for any launch; the client records none, and sync carries the host's sessions to other devices. A host that isn't Caskade records nothing. (Decided 2026-10-30.)

## 3. Managed tools

### 3.1 A layer under emulator setup

`EmulatorSetup` (`emulation/setup/`) already resolves sources (GitHub releases without the API, URLs, Flathub), downloads, unpacks through `caskade-installer`, carries user files over on update, records managed installs and watches for updates. Its emulator-only steps are `configure()` (profiles, platform defaults), BIOS and RetroArch cores.

Built (#254). `caskade_core::managed` takes the generic part: `resolve`, the download, the release swap that carries user files over, Flathub installs, and version comparison. `EmulatorSetup` uses it and keeps only its emulator-specific steps. A `ToolDefinition` gives a folder under `<data>`, detect rules and install sources per OS; a source's `when` (`x86_64`, `aarch64`, `wayland`, `x11`, `gamescope`) picks the machine's first. `ManagedTools` installs, finds, checks, updates, skips and removes tools, records them in `managed_tools` (migration 0029) and follows `core.updates`. Removing a tool deletes only the files its release shipped, so its configuration stays (§7). Versions come from the source (the release tag); nothing runs the program to ask it. Tool definitions live in `crates/caskade-sources/tools/` (`sunshine.toml`, `moonlight.toml`), and `tests/tools.rs` checks them against the full asset lists of Sunshine v2026.914.233613 and moonlight-qt v6.2.0, so an upstream rename is a one-line fix.

### 3.2 Install sources

| | Host: Sunshine | Client: Moonlight |
| --- | --- | --- |
| Windows | `Sunshine-Windows-{AMD64,ARM64}-lite.zip`, unpacked; the service and firewall rules are Caskade's own (§4.2). Not the MSI: it adds Start-menu shortcuts, an Apps & Features entry, PATH and firewall rules all named Sunshine. | `MoonlightPortable-{x64,arm64}-<ver>.zip`: no administrator rights, no shortcuts. Not the installer (Burn bundle, UAC, Start-menu entry). |
| Linux | `Sunshine_<ver>_{x86_64,aarch64}.AppImage`, extracted; Flathub `dev.lizardbyte.app.Sunshine` as fallback. | Flathub `com.moonlight_stream.Moonlight` on Wayland sessions, gamescope (Steam Deck game mode) and aarch64; `Moonlight-<ver>-x86_64.AppImage` on X11. |
| macOS | not in the first version | `Moonlight-<ver>.dmg` (universal, signed, notarized), app copied out |

Moonlight's choice departs from the emulator rule (a download before a Flatpak) on purpose: the AppImage is built without Wayland and libdrm, and under XWayland it warns that hardware decoding may break.

### 3.3 Where

`<data>/streaming/host/` holds the host program and its config; `<data>/streaming/client/` holds the client program and its portable state. Both are Caskade's own: nothing is read from or written to a user's own Sunshine or Moonlight setup (§3.4).

### 3.4 Someone else's Sunshine or Moonlight

Detection reuses discovery's probes and the Windows uninstall entries: Sunshine's MSI UpgradeCode `{512A3D1B-BE16-401B-A0D1-59BBA3942FB8}`, the `SunshineService` service, `/usr/bin/sunshine`, the Flatpak, the `app-dev.lizardbyte.app.Sunshine` user unit, `/Applications/Sunshine.app`, Homebrew.

- A user's own **Moonlight** is left alone: Caskade's copy runs in portable mode with its own state (§5.2), so the two never see each other.
- A user's own **Sunshine** collides with Caskade's on ports and on mDNS. Hosting then offers to use it ("Streaming is already set up on this device. Use it?"), asking once for its user name and password, which go to the keychain. Caskade changes only the apps it added (§4.3) and never its other settings. Declining leaves hosting off.

## 4. The host

### 4.1 Configuration

Caskade writes the whole configuration before the first start and owns it from then on:

- `sunshine.conf`: `sunshine_name` = the device's name in Caskade; `system_tray = disabled` (no tray icon, no notifications); `origin_web_ui_allowed = pc`; `upnp = disabled`; `notify_pre_releases = disabled`; `file_apps`, `file_state`, `credentials_file` and `log_path` inside `<data>/streaming/host/`; on Windows `gamepad_driver` (§4.5) and the `dd_*` display options (§4.4).
- `apps.json`: replaced with Caskade's own list (§4.3). The default "Desktop" and "Steam Big Picture" entries never reach a client.
- Credentials: a random user name and password, set with `sunshine --creds <user> <pass>` before the first start and kept in the OS keychain (service `dev.caskade.streaming`), never in the database, settings, logs, IPC or backups.
- On Linux, `CONFIGURATION_DIRECTORY` points Sunshine at Caskade's folder. On Windows the configuration is `<exe dir>\config`, inside Caskade's folder.

### 4.2 Running it

- **Only while Caskade runs.** The host starts when Caskade starts with hosting on and stops when Caskade quits (a window closed to the tray keeps it running). A host left running by a crash is stopped at the next start.
- **Linux:** Sunshine runs as Caskade's child, ended with Caskade (`PR_SET_PDEATHSIG`), and is restarted if it crashes.
- **Windows:** Sunshine needs a service to capture the secure desktop and inject into elevated windows. `tools\sunshinesvc.exe` from the lite zip is registered as a Caskade-named service (*unverified* that the zip carries it) set to start on demand, never at boot, and launches `Sunshine.exe` in the active session. The administrator step grants the interactive user the right to start and stop that one service (its security descriptor), so Caskade starts it at launch and stops it on quit without another prompt. Firewall rules named for Caskade allow TCP 47984, 47989, 48010 and UDP 47998–48000 for that executable. The web API (47990) stays on localhost.
- **API client:** HTTPS to `localhost:47990` with basic auth, pinned to the certificate in `credentials/cacert.pem` (a custom rustls verifier; the RomM client's accept-anything switch is not good enough). Requests carry no `Origin`, which skips Sunshine's CSRF check, and JSON content types.
- **Health:** `GET /api/config` gives the version; a failed start, a crash or a refused request shows as "Streaming isn't working on this device" with Repair (§7).

### 4.3 Games published to the host

- Every installed, launchable library game that the host's active account may play becomes a Sunshine app, added and removed through `POST /api/apps` and `DELETE /api/apps/{index}` as the library changes. The app's name is the game's title; its image is the cover, written as PNG.
- One extra app, **Caskade**, starts Caskade in console mode, so a client can browse and launch the host's library with a pad.
- An app's command is `caskade --launch <game id>` (§8.1): it hands the game to the running Caskade, which launches it through the pipeline, and stays alive until the session ends, so Sunshine's own idea of when the stream ends (`auto-detach: false`, `wait-all`) matches the game's. Ending the stream (Sunshine sends WM_CLOSE on Windows, SIGTERM on POSIX) stops the game the way Caskade's Stop does.
- `SUNSHINE_CLIENT_WIDTH`, `_HEIGHT`, `_FPS` and `_HDR` reach the launch, so an emulator's display flags and console mode can follow the client.
- `POST /api/apps` re-sorts the list by name: Caskade reads it back before using an index.

### 4.4 Display

On Windows, `dd_configuration_option = ensure_active` with resolution, refresh rate and HDR set to follow the client changes the host's display mode for the stream and restores it after. Linux has no built-in mode change: a headless Linux host needs a dummy plug, an EDID or a compositor's virtual output; Caskade reports the missing display rather than hiding it. A virtual display (Sunshine PR #5114) replaces this when it lands.

### 4.5 Input and capture

- **Linux:** the administrator prompt installs `60-caskade-streaming.rules` (`uinput`, `uhid` and the virtual HID nodes, group `input`, `uaccess`) and loads `uhid`, as Sunshine's own rules do. Capture uses the unprivileged backends: X11 on X11, the desktop portal on Wayland (one remote-desktop approval, kept through `portal_token`; on KDE `flatpak permission-set kde-authorized remote-desktop … yes` skips it). KMS capture would need `setcap cap_sys_admin,cap_sys_nice+p` on the binary; Caskade does not grant it (a capability on a user-writable binary was Sunshine's own high-severity GHSA-fp6g-27w5-489j).
- **Windows:** keyboard and mouse need nothing more. Gamepads need a virtual gamepad driver, and unmodified Sunshine speaks to only two (`gamepad_driver`: `vigembus`, `virtualhid` or `all`):
  - **ViGEmBus** ([nefarius/ViGEmBus](https://github.com/nefarius/ViGEmBus), BSD-3-Clause), the one Caskade installs. Its last release, 1.22.0, is a production-signed installer (`ViGEmBus_1.22.0_x64_x86_arm64.exe`, silent with `/exenoui /qn /norestart`, *unverified* until tried), pinned by SHA-256 and run in the administrator step. The repository was archived in 2023 after a trademark dispute: the driver works but gets no more fixes. It emulates Xbox 360 and DualShock 4 pads (rumble; motion, touchpad and light bar on DS4), not Xbox One/Series, DualSense or Switch. An existing install is detected and left in place (an in-place upgrade needs its setup run twice).
  - LizardByte's **Virtual HID Driver** is source-available, not open source: its licence forbids redistribution, and it costs $14.99 a year or $49.99 for life (five machines), checked online. Caskade never installs it; a host whose user bought it gets `gamepad_driver = all`.
  - Caskade writes `gamepad_driver = vigembus`, which also stops Sunshine's driver prompts.
  - Open-source drivers Sunshine can't use without a patch, kept in view in case ViGEmBus breaks: [HIDMaestro](https://github.com/hifihedgehog/HIDMaestro) (MIT, active, Xbox One/Series and DualSense profiles, but self-signed with a root certificate it installs) and [usbip-win2](https://github.com/vadimgrn/usbip-win2) (BSD-2, Microsoft attestation-signed through OSSign; a localhost USB/IP server can present a pad). A ViGEmBus-compatible fork with the same interface (e.g. PuppetPadBus) would work unmodified if one were reviewed and signed.

## 5. The client

### 5.1 Installing

The first Stream from <device> installs Moonlight (§3.2) as a job in Activity, then streams. No question is asked; the job shows as "Getting streaming ready".

### 5.2 Isolated state

Moonlight runs in portable mode: a `portable.dat` in its **working directory** makes it keep settings, paired hosts, certificates and caches in `<cwd>/Moonlight Game Streaming Project/Moonlight.ini` and `<cwd>/cache/`. Caskade launches it with the working directory `<data>/streaming/client/state/` and pre-writes:

- `confwarnings=false`, `connwarnings=false`, `detectnetblocking=false` (no warning toasts or overlays);
- `richpresence=false` (Windows and macOS builds announce "Moonlight" on Discord otherwise);
- `uidisplaymode=2` (Moonlight's own window, shown briefly before and after a stream, is fullscreen and dark under Caskade's curtain);
- `mdns=true`.

*Unverified:* the Flatpak's working directory (`flatpak run --cwd=` inside `~/.var/app/com.moonlight_stream.Moonlight/`).

### 5.3 Moonlight's own windows

Moonlight's command line was not built to be invisible; each case has a workaround:

- A spinner window before the stream and a quitting window after: covered by Caskade's curtain (console mode) or the launch overlay (desktop mode), lifted when the stream window takes focus.
- **Errors open a dialog and the process waits; the exit code is 0 for success and failure alike.** Caskade checks what it can before streaming (host reachable, paired, app present: `list --csv` or the host's `/serverinfo`), reads stderr for `Qt Critical:` lines while streaming, then ends the process and shows its own error.
- Pairing ends on a "Pairing completed" dialog: Caskade ends the process once the host's certificate appears in the state file or `list` succeeds.
- "Are you sure you want to quit <app>?" when the host already runs another app: Caskade asks first in its own words ("Living room is playing Hades. Stop it and stream Celeste?") and runs `quit <host>` before `stream`.

## 6. Finding and pairing

### 6.1 Finding hosts

- The client browses mDNS `_nvstream._tcp` (an MIT/Apache Rust mDNS crate; Caskade has none yet), and accepts a host typed by address. The instance name is the host's `sunshine_name`, which Caskade set to the device name.
- Any Sunshine host is welcome, not only Caskade's: a friend's PC running Sunshine pairs the same way, through its own web page.
- Hosts that are known but asleep are woken: Moonlight sends Wake-on-LAN to every known host with a stored MAC before it looks for it. Sunshine gives the MAC only to paired clients.

### 6.2 Pairing with a code

1. On the client, Pair with Living room picks a random four-digit code, shows it ("Enter 4831 on Living room") and runs `moonlight pair <host> --pin 4831`.
2. Sunshine `v2026.906` and later list waiting requests (`GET /api/pin`: id, client name, address). The host's Caskade shows "Bedroom PC wants to stream games from this device" with a code field, and submits it with `POST /api/pin {pairing_id, pin, name}`. Decline cancels it with `DELETE /api/pin`. Requests expire after five minutes.
3. Both ends list the pairing; the host can remove it (`POST /api/clients/unpair`), and the client forgets the host by deleting it from its state.

Only admins pair on the host. A client account needs no special permission.

### 6.3 Pairing through sync

Once devices are paired through sync (#226) and exchange changes (#230), the client sends its code to the host over `caskade/sync/1` and the host submits it unprompted: paired devices stream with no code. Sync's view of other devices' installed copies (#232) then names exactly which game each app is (§8.2).

## 7. Updates and repair

- Both programs follow `core.updates` like managed emulators: the watch checks GitHub's `releases/latest` redirect without the API, an update shows in "Updates available" with Update and Skip, and nothing updates itself. Neither program has its own updater; Moonlight's check only shows a toolbar button that command-line launches hide.
- Updating the host stops it, swaps the files, re-applies Caskade's configuration and Linux rules if the release changed them, and starts it again; never during a stream. Updating the client never interrupts a stream.
- **Repair** re-runs setup: missing files, a configuration that drifted, rules removed by the OS, the service gone, credentials lost from the keychain (new ones, then pairing again).
- **Turning hosting off** stops the host and removes its program, service, firewall rules and Linux rules (one administrator prompt). Its configuration, credentials (keychain) and paired clients stay, so turning hosting back on reinstalls it and every paired device streams again with no new code. Removing a device unpairs it; a Forget all paired devices button clears them.

## 8. Core and app changes

### 8.1 Starting a game from outside

Built (#256, `apps/desktop/src-tauri/src/launch_cli/`). `caskade --launch <game id> [--entry <id>] [--action <id>]` hands the launch to the running Caskade, or starts one, goes through `PlayGate` and needs someone signed in like any Play, and stays alive until the session ends. It is useful beyond streaming: desktop shortcuts and other launchers.

- **Forwarding.** The single-instance plugin ends a second process at once and sends nothing back, so the caller talks to the running Caskade over a channel of its own: a Unix socket at `<cache>/run/launch.sock` (folder `0700`, socket `0600`), or on Windows a named pipe named from the cache path that refuses remote clients. With no answer, the caller starts Caskade detached (its own process group; out of the caller's job on Windows where the job allows) and waits up to 90 s for it.
- **The release.** `--entry` names one; otherwise the game's default release when it can launch, else its first installed one Caskade starts itself (as Play picks).
- **Lifetime and exit codes.** Caskade answers `started` and then `ended`, one JSON line each; the caller exits 0 when the game exited, 1 when it crashed, 3 when refused, 4 when it couldn't start or Caskade couldn't be reached, 5 when Caskade's Stop ended it, and 2 for a wrong command line. Ending the caller closes the channel, and Caskade then stops the game as Stop does: SIGTERM (or any end) on POSIX, and on Windows WM_CLOSE to the caller's hidden window, which is what Sunshine sends.
- **Refusals don't prompt.** An age limit, nobody signed in, the game already running, or a release that would ask which action or profile ends the caller with the reason on stderr (decided 2026-11-02: nobody may be in front of the host to type an admin PIN). An admin's permanent approval (#269) lets a game through.
- **Display.** `SUNSHINE_CLIENT_WIDTH`, `_HEIGHT`, `_FPS` (digits) and `_HDR` (`true`/`false`) reach the game's environment; nothing else from the caller's environment does.
- **Still to check by hand:** a Windows caller ended by WM_CLOSE (Sunshine's stream end) stops the game.

### 8.2 Streamed releases in the library

- The host's apps become uninstalled releases under a `stream:<host>` source, like `store:<account>`, joined to library games by title through the same matching as other sources (`LibraryStore::same_games`). Apps no game matches become streaming-only games. Sync (#232) replaces title matching with the game's identity.
- `LaunchSpec::Stream { host, app }` resolves to a Moonlight command in `LaunchService::start`, beside the `Web` special case; `resolve.rs`'s installed check lets it through. Tracking follows the Moonlight process.
- Play offers Stream from <device> wherever it offers a release (`PlayChoice`, `LaunchChoiceDialog`, the game menu), with the device's state: ready, asleep (wake first), busy (playing another game), unreachable.
- Streamed play counts on the host only: its launch through `caskade --launch` records the session and playtime under the host's active account. The client records no play session; it follows the stream like a session for focus and the console curtain, without writing one.
- Age limits are checked on the client for its account before streaming, and on the host for its active account at launch, as for any launch.

### 8.3 Pads during a stream

Moonlight reads pads only while its window has focus but grabs none, so Caskade's input service still sees every press. While a stream window has focus Caskade ignores pad actions, except the guide button, which shows Caskade's quick menu with End stream. Moonlight's own quit combo (Start+Select+L1+R1) stays as a fallback.

### 8.4 Elevation

Caskade never elevates today (`caskade-installer` only delegates package installs to `pkexec` and `osascript`). Hosting needs one administrator step per OS, for a fixed, auditable list of actions with an undo:

- Linux: `pkexec` running a fixed script shipped with Caskade (install or remove the udev rule and module load).
- Windows: Caskade itself started elevated (`runas`) with a fixed subcommand that registers or removes the service (on demand, startable and stoppable by the interactive user) and firewall rules, and installs ViGEmBus (§4.5).

Built (#255, `caskade_core::elevation`). Callers name a plan (`Plan::StreamingHost`); nothing else crosses, so no webview text reaches the elevated side.

- **Changes.** A plan is a fixed set of changes per OS:
  - Linux: `/etc/udev/rules.d/60-caskade-streaming.rules` (uinput, uhid and the virtual pads' nodes for group `input` and `uaccess`), `/etc/modules-load.d/60-caskade-streaming.conf` (uhid at boot) and uhid loaded now.
  - Windows: the on-demand `CaskadeStreaming` service for `tools\sunshinesvc.exe`, whose security descriptor lets the interactive user start and stop it, and the `Caskade streaming (TCP)` (47984, 47989, 48010) and `(UDP)` (47998–48000) inbound rules for the host's `sunshine.exe`, on private and domain networks.
- **One prompt, only for what's missing.** Caskade looks, unelevated, at which changes are there, asks once for the rest, and looks again. What appeared is recorded in `<data>/elevation/streaming-host.json` (change names only; a record holding anything else is refused). The undo removes the recorded changes still present, in reverse order, behind one prompt. What was there before is never touched. Nothing missing means no prompt.
- **Linux:** `pkexec /bin/sh -c <constant script> caskade <step names>`. The script refuses unknown steps before changing anything, and undoes its own earlier steps when one fails. pkexec's 126 is a cancel and 127 a refusal. A uhid still in use stays loaded after the undo, and stops counting as Caskade's.
- **Windows:** PowerShell `Start-Process -Verb RunAs` runs `caskade.exe --elevated <step names> --host <data>\streaming\host`, handled first in `run()`. The elevated side accepts only known step names and a host folder shaped `…\streaming\host` that holds `sunshine.exe` and `tools\sunshinesvc.exe`. It undoes its earlier steps when one fails; a dismissed UAC prompt exits 1223. ViGEmBus joins this prompt with #260.
- **Risk (open).** A service running a launcher from the user's own data folder lets anyone who can write that folder run code as the service. On a machine whose user is an administrator that is no new power; for a standard user who had an administrator type the password, it is. Options: copy the service files into an administrator-only folder in the same prompt (updates would then need a prompt too), or lock the host folder's ACL. Decide before ST3 ships hosting.

### 8.5 Settings and UI

- **Settings → Streaming** (admins; namespace `core.streaming`): the switch "Let other devices stream games from this one", the host's state (Ready, Setting up, Needs attention with Repair), paired devices with Remove, and waiting pairing requests. Client side: known devices, Pair with…, Add by address, and stream quality (Automatic, or resolution, frame rate, bitrate) passed as `stream` options.
- The game page and Play button (§8.2), the pairing prompt as a notification and dialog on the host, and setup jobs in Activity.

## 9. Milestones

| #   | Contents |
| --- | --- |
| ST0 | Research, this plan. |
| ST1 | Managed-tool layer (§3.1) under emulator setup; tool definitions with table tests. |
| ST2 | Elevation helper (§8.4) and `caskade --launch` (§8.1). |
| ST3 | Host: install, configuration, credentials, supervision, the pinned API client (§4.1–4.2); adopting a user's Sunshine (§3.4). |
| ST4 | Host: publishing games (§4.3), display (§4.4), input and capture (§4.5). |
| ST5 | Client: install and isolated state (§5); finding hosts (§6.1). |
| ST6 | Pairing with a code (§6.2). |
| ST7 | Streaming a game: `LaunchSpec::Stream`, streamed releases, Moonlight's windows, pads, wake (§5.3, §8.2–8.3). |
| ST8 | Settings → Streaming and the game page on the real core (§8.5). |
| ST9 | Updates, repair and turning hosting off (§7). |
| ST10 | Pairing through sync (§6.3), after #226 and #230. |

### Manual checks

Each item names its issue; record the date and result here.

- [ ] #255 Linux: switching hosting on shows one pkexec prompt, and cancelling it leaves `/etc/udev/rules.d` and `/etc/modules-load.d` unchanged. After allowing it, the rules and module list are there, `/dev/uinput` and `/dev/uhid` belong to group `input` with a seat ACL, and uhid is loaded. Switching hosting off removes exactly those, with one prompt.
- [ ] #255 Windows: one UAC prompt (Caskade, verified publisher); cancelling changes nothing (`sc query CaskadeStreaming` fails, `netsh advfirewall firewall show rule name="Caskade streaming (TCP)"` finds none). After allowing it, the service is demand-start, a standard user can `sc start` and `sc stop` it, and both firewall rules name the host's `sunshine.exe`. A host path with a space and an apostrophe works. Switching hosting off removes the service and both rules, with one prompt.
- [ ] #256 Windows: with Caskade running, `caskade.exe --launch <game id>` starts the game; `taskkill /PID <caller>` (WM_CLOSE, without `/F`), or ending a Sunshine stream, quits the game and ends its session, and the caller exits 5.

### Testing

- Tool definitions: asset patterns match the names recorded in this plan (table test).
- The Sunshine API against wiremock (apps, pins, clients, config, an expired request, a refused login), including the pinned certificate and a wrong one.
- Moonlight through a fake executable that prints its CLI's outputs (`list --csv`, `Qt Critical:` errors, a pairing that blocks on its dialog), for the launch, error and pairing paths.
- `caskade --launch` on Tauri's mock runtime: forwarding, `PlayGate`, staying alive until the session ends, ending on a signal.
- `live` tests (ignored) download the real releases into a temp folder; a manual checklist covers a real stream per OS pair, Wake-on-LAN, a Windows host's display mode and gamepads, and the Steam Deck as a client.

## 10. Risks and open questions

- **ViGEmBus is unmaintained.** If a Windows update stops it loading, Windows hosts stream keyboard and mouse only until a signed, compatible replacement exists (§4.5). Caskade checks that the driver answers when hosting starts and says so if it doesn't.
- **Linux host capture on Wayland:** the portal's one-time approval is the only prompt Caskade can't hide.
