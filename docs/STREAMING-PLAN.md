# Caskade — Game Streaming Plan

Status: built; the real-machine checks in §9 and `docs/manual-checks.md` are still open. Built: the managed-tool layer (§3.1, Gitea #254), `caskade --launch` (§8.1, #22), the elevation helper (§8.4, #21), the host (§4.1–4.2, #23), the client's install and state (§5.1–5.2, #27), finding hosts with their games in the library (§6.1, §8.2, Gitea #262), using a user's own Sunshine (§3.4, #24), publishing the library's games (§4.3, #25), pairing with a code (§6.2, #28), streaming a game from Play (§5.3, §8.2–8.3, #29), Settings → Streaming (§8.5, #30), updates, repair and removal (§7, #31), the host's display, input and capture (§4.4–4.5, #26) and pairing through sync (§6.3, #32); issues #21–#32 in the Game streaming milestone, tracked in #33 · Last updated: 2026-11-05

This header and §9 use GitHub issue numbers. Three-digit numbers elsewhere on this page are archived Gitea issues, the numbers old commits and code comments cite; `docs/manual-checks.md` maps the ones behind open checks.

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

### 3.4.1 Built (#258)

- **Detection** (`streaming::existing`):
  - Windows: the MSI's UpgradeCode under `Installer\UpgradeCodes` (packed `B1D3A21561EBB1040A1D95BB3A49F28B`) and the `SunshineService` service.
  - Linux: `/usr/bin/sunshine`, the Flatpak's install folders and the `app-dev.lizardbyte.app.Sunshine` user unit.
  - macOS: `Sunshine.app` and Homebrew.
  - With each, its `credentials/cacert.pem` where that install keeps it.
- **Switching hosting on** then installs nothing and answers `existingFound`. `streaming_use_existing` takes its web user name and password, checks them against its API (pinned to its own certificate), keeps them in the keychain (`existing`) and sets `core.streaming.existing`.
- **Only Caskade's apps change** (`streaming::publish`). An app is Caskade's when its command runs this Caskade's `--launch`; `sync` adds and removes only those. Turning hosting off withdraws them and leaves the host, its settings and its other apps as they were. Caskade never starts or stops someone else's host; its status is only whether the API answers.
- Choosing which games to offer, and keeping them current, is #259.

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

### 4.2.1 Built (#257)

`caskade_core::streaming::StreamingHost`, with commands `streaming_host_status` and `streaming_set_hosting` (admins). Checked against the real Sunshine v2026.914.233613 on Linux on 2026-11-02:

- **Install:** managed tools put the release in `<data>/streaming/host`. On Linux the AppImage is unpacked at install (`extract = true` in `tools/sunshine.toml`), and `usr/bin/sunshine` runs directly. That needs no FUSE, and skips the AppImage's `AppRun`, which forks Sunshine (so a death signal never reaches it) and creates `~/.config/sunshine`.
- **Configuration folder:** with `CONFIGURATION_DIRECTORY=<dir>`, Sunshine keeps `sunshine.conf`, `apps.json`, `sunshine_state.json`, `credentials/` and its log in `<dir>/sunshine/`, so no path keys are needed. Caskade writes `sunshine.conf` before every start and an empty `apps.json` once; after that the list changes only through the API. On Windows the folder is `config` beside `sunshine.exe`, and the administrator step copies it with the program.
- **Credentials:** `sunshine --creds <user> <pass>` writes a salted hash to `sunshine_state.json`. It ignores a configuration-file argument, but honours `CONFIGURATION_DIRECTORY`. The hash isn't a plain SHA-256 of password and salt, so Caskade always uses `--creds`.
- **Certificate:** the web API serves exactly `credentials/cacert.pem` (CN `Sunshine Gamestream Host`, no subject alternative name), so the client pins those bytes and ignores names. `GET /api/config` gives `version` and `platform`; a wrong password is 401.
- **Running:** a dedicated thread per run, because a death signal belongs to the thread that started the process. The host runs in its own process group, and `run.json` records it by PID and start time. Readiness is the API answering; a crash restarts it with backoff, and five crashes in ten minutes stop the restarts.
- **Device name:** Sunshine announces itself over mDNS under the **OS host name**, not `sunshine_name`. `sunshine_name` is what `/serverinfo` calls `hostname`, so the client takes a host's name from `/serverinfo` (§6.1).

### 4.3 Games published to the host

- Every installed, launchable library game that the host's active account may play becomes a Sunshine app, added and removed through `POST /api/apps` and `DELETE /api/apps/{index}` as the library changes. The app's name is the game's title; its image is the cover, written as PNG.
- One extra app, **Caskade**, starts Caskade in console mode, so a client can browse and launch the host's library with a pad.
- An app's command is `caskade --launch <game id>` (§8.1): it hands the game to the running Caskade, which launches it through the pipeline, and stays alive until the session ends, so Sunshine's own idea of when the stream ends (`auto-detach: false`, `wait-all`) matches the game's. Ending the stream (Sunshine sends WM_CLOSE on Windows, SIGTERM on POSIX) stops the game the way Caskade's Stop does.
- `SUNSHINE_CLIENT_WIDTH`, `_HEIGHT`, `_FPS` and `_HDR` reach the launch, so an emulator's display flags and console mode can follow the client.
- `POST /api/apps` re-sorts the list by name: Caskade reads it back before using an index.

#### Built (#259)

`caskade_core::streaming::publish::Publisher`, watched while Caskade runs:

- **Offers:** every installed game with a release Caskade starts itself (not a plugin's, not hidden) that the host's signed-in account may play (#209). Each app runs `"<caskade>" --launch <game id>`, with `auto-detach` off and `wait-all` on. Its cover goes to `<data>/streaming/covers/<game id>.png`, 600×800.
- **The console app:** one more app, "Caskade", runs `"<caskade>" --console` as a detached command.
- **Flatpak hosts:** on a user's own Flatpak Sunshine, commands leave its sandbox through `flatpak-spawn --host`.
- **Batching:** library, install, uninstall and account events mark the list dirty, and one sync goes 2 s after the first. A host that comes up gets its list at once. Nothing is sent while the host isn't Ready.
- **Ownership:** `sync` removes Caskade's apps that are no longer offered or have been renamed (rereading indexes after each deletion), then adds the missing ones. An app is Caskade's when its command, or the console app's detached command, runs this Caskade; nothing else is touched. The same sync serves a user's own Sunshine (#258).

### 4.4 Display

On Windows, `dd_configuration_option = ensure_active` with resolution, refresh rate and HDR set to follow the client changes the host's display mode for the stream and restores it after. Linux has no built-in mode change: a headless Linux host needs a dummy plug, an EDID or a compositor's virtual output; Caskade reports the missing display rather than hiding it. A virtual display (Sunshine PR #5114) replaces this when it lands.

### 4.5 Input and capture

- **Linux:** the administrator prompt installs `60-caskade-streaming.rules` (`uinput`, `uhid` and the virtual HID nodes, group `input`, `uaccess`) and loads `uhid`, as Sunshine's own rules do. Capture uses the unprivileged backends: X11 on X11, the desktop portal on Wayland (one remote-desktop approval, kept through `portal_token`; on KDE `flatpak permission-set kde-authorized remote-desktop … yes` skips it). KMS capture would need `setcap cap_sys_admin,cap_sys_nice+p` on the binary; Caskade does not grant it (a capability on a user-writable binary was Sunshine's own high-severity GHSA-fp6g-27w5-489j).
- **Windows:** keyboard and mouse need nothing more. Gamepads need a virtual gamepad driver, and unmodified Sunshine speaks to only two (`gamepad_driver`: `vigembus`, `virtualhid` or `all`):
  - **ViGEmBus** ([nefarius/ViGEmBus](https://github.com/nefarius/ViGEmBus), BSD-3-Clause), the one Caskade installs. Its last release, 1.22.0, is a production-signed installer (`ViGEmBus_1.22.0_x64_x86_arm64.exe`, silent with `/exenoui /qn /norestart`: W3 ran those flags and the installer stayed silent, then refused because the machine runs Windows Server, so the driver is still untried on a client edition), pinned by SHA-256 and run in the administrator step. The repository was archived in 2023 after a trademark dispute: the driver works but gets no more fixes. It emulates Xbox 360 and DualShock 4 pads (rumble; motion, touchpad and light bar on DS4), not Xbox One/Series, DualSense or Switch. An existing install is detected and left in place (an in-place upgrade needs its setup run twice).
  - LizardByte's **Virtual HID Driver** is source-available, not open source: its licence forbids redistribution, and it costs $14.99 a year or $49.99 for life (five machines), checked online. Caskade never installs it; a host whose user bought it gets `gamepad_driver = all`.
  - Caskade writes `gamepad_driver = vigembus`, which also stops Sunshine's driver prompts.
  - Open-source drivers Sunshine can't use without a patch, kept in view in case ViGEmBus breaks: [HIDMaestro](https://github.com/hifihedgehog/HIDMaestro) (MIT, active, Xbox One/Series and DualSense profiles, but self-signed with a root certificate it installs) and [usbip-win2](https://github.com/vadimgrn/usbip-win2) (BSD-2, Microsoft attestation-signed through OSSign; a localhost USB/IP server can present a pad). A ViGEmBus-compatible fork with the same interface (e.g. PuppetPadBus) would work unmodified if one were reviewed and signed.

#### Built (#260)

- **Windows configuration** (`config::render`, `HostConfig.windows`):
  - `dd_configuration_option = ensure_active`, plus `dd_resolution_option`, `dd_refresh_rate_option` and `dd_hdr_option = auto`: the display follows the client during a stream, and Sunshine puts it back afterwards (its default).
  - `gamepad_driver = vigembus`.

  Linux gets neither: Sunshine picks an unprivileged capture itself (X11, the desktop portal, KWin), keeps the portal's approval in its own `portal_token` in the configuration folder (which survives updates, #266), and Caskade never grants it capabilities.
- **ViGEmBus** (`elevation::Change::GamepadDriver`, Windows):
  - **Download:** `streaming::gamepads::fetch` downloads `ViGEmBus_1.22.0_x64_x86_arm64.exe` unelevated into `<data>\streaming\drivers`. It keeps the file only when its SHA-256 is `89220a7865076b342892f98865f3499fb7c4cfd673159e89d352c360fd014c6a` (checked 2026-11-02: 6,278,576 bytes, Advanced Installer, signed by Nefarius Software Solutions e.U.; GitHub records no digest for it).
  - **Install:** it is part of the one administrator prompt when `sc query ViGEmBus` finds no driver. The elevated side copies the installer into the administrator-only folder, checks the hash there, runs it with `/exenoui /qn /norestart`, and deletes the copy.
  - **Best effort:** a failed install never fails the step; hosting then warns that pads won't work.
  - **Kept:** a driver that was there before is never touched, and the undo leaves Caskade's install in place too (`Change::kept_on_undo`), because other programs use it.
  - LizardByte's Virtual HID Driver isn't detected yet; `gamepad_driver = all` for its users is open.
- **Checks at Ready** (`HostChecks::here`, `HostStatus.warning`):
  - Linux: a desktop session (`DISPLAY` or `WAYLAND_DISPLAY`), and `/dev/uinput` writable (the #255 rule).
  - Windows: the `ViGEmBus` service running.

  A host with no screen ("Connect a monitor or a display dummy plug") or pads that won't work says so in Settings → Streaming. It is reported, not hidden, and Repair re-runs the administrator step.
- **Tests.**
  - The configuration per OS.
  - The driver joining the Windows prompt, staying after the undo, and not asked for when already there.
  - Only the pinned installer running, never from staging, best effort.
  - A download that isn't the pinned file is refused.
  - A ready host's warnings, and the warning in Settings.

## 5. The client

### 5.1 Installing

The first Stream from <device> installs Moonlight (§3.2) as a job in Activity, then streams. No question is asked; the job shows as "Getting streaming ready".

### 5.2 Isolated state

Moonlight runs in portable mode: a `portable.dat` in its **working directory** makes it keep settings, paired hosts, certificates and caches in `<cwd>/Moonlight Game Streaming Project/Moonlight.ini` and `<cwd>/cache/`. Caskade launches it with the working directory `<data>/streaming/client/state/` and pre-writes:

- `confwarnings=false`, `connwarnings=false`, `detectnetblocking=false` (no warning toasts or overlays);
- `richpresence=false` (Windows and macOS builds announce "Moonlight" on Discord otherwise);
- `uidisplaymode=0` (Moonlight's own window keeps the 1×1 size Caskade gives it. Full screen, it showed its grey "Starting…" page and a button tip for about 1.5 s on a real desktop, 6 Nov 2026);
- `mdns=true`.

*Unverified:* the Flatpak's working directory (`flatpak run --cwd=` inside `~/.var/app/com.moonlight_stream.Moonlight/`).

### 5.2.1 Built (#261)

`caskade_core::streaming::StreamingClient`. Checked against the real moonlight-qt 6.2.0 on Linux on 2026-11-02:

- **Install:** `ensure` installs it the first time, as an Activity job titled "Getting streaming ready". Linux unpacks the AppImage (`extract = true`; no FUSE, which Ubuntu 22.04 and later lack by default) and runs its `AppRun`. That script picks a fallback VA-API when the system's doesn't work, then `exec`s Moonlight, so the process stays the same.
- **Portable state:** with `portable.dat` in the working directory, everything goes to `<cwd>/Moonlight Game Streaming Project/Moonlight.ini`: the `[General]` settings, the client certificate and key, the unique id, and `[hosts]` with each paired host's name, uuid, address, MAC and apps. Moonlight saves it only when it quits cleanly: SIGTERM, not SIGKILL.
- **Quiet settings:** `prepare_state` writes the six settings into `[General]` before every run and keeps every other line.
- **Flatpak:** the working directory is `~/.var/app/com.moonlight_stream.Moonlight/caskade`, reached with `flatpak run --cwd=` (*unverified*).
- **Environment:** a tool started from Caskade drops Caskade's own AppImage runtime (`APPDIR`, `APPIMAGE`, `ARGV0`, `OWD`, and any variable pointing into Caskade's mount; `managed::outside_own_appimage`). Moonlight's `AppRun` otherwise takes an inherited `APPDIR` as its own folder and fails.
- **The CLI and a real host:** `moonlight list <host> --csv` waited forever fetching box art for an app without an image, after it had written the host's apps into its settings. Caskade reads apps from its own client state and gives `list` a hard limit. `moonlight pair <host> --pin <code>` stays open on its "Pairing completed" dialog. In this Sunshine version `GET /api/pin` lists waiting requests (`id`, `name`, `address`), and `POST /api/pin` needs `pairing_id`, `pin` and `name`.

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

### 6.1.1 Built (#262)

`caskade_core::streaming::hosts::StreamHosts`, watched every 5 s while Caskade runs. Commands `streaming_hosts` and `streaming_add_host`.

- **mDNS** (`streaming::mdns`, with `hickory-proto`): one `_nvstream._tcp.local` PTR question from an ordinary port, so responders answer it directly (legacy unicast) and no port-5353 socket is needed. Answers' PTR, SRV and A records are put together. The instance name is the host's **OS host name**; checked against Sunshine v2026.914 through Avahi.
- **Hosts typed by address** live in `core.streaming.addresses`. **Paired hosts** come from Caskade's own client state (`[hosts]`: uuid, name, address, MAC, apps).
- **State:** every candidate is asked `GET http://<host>:47989/serverinfo`, which needs no pairing. The host's name is `hostname` (its `sunshine_name`), its id `uniqueid`, and it is busy when `state` says BUSY or `currentgame` isn't 0. One that doesn't answer is **asleep** when Moonlight knows its MAC, else **unreachable**.
- **Games:** a paired host's apps become uninstalled, unlaunchable releases under `stream:<host id>` (`LibraryStore::sync_stream_apps`). Each joins the library game with the same title key (case, composition and punctuation aside; hidden and kept-apart games excluded), or becomes a streaming-only game. An app the host stops offering goes, and its game goes with it when nothing else is left. The game page names the release "Stream from <device>" (`api/streaming.ts`).
- **App lists** are refreshed at most every 10 minutes per reachable paired host, with `moonlight list <host> --csv` stopped after 20 s by SIGTERM (so it saves), then read back from the client's state.

### 6.2 Pairing with a code

1. On the client, Pair with Living room picks a random four-digit code, shows it ("Enter 4831 on Living room") and runs `moonlight pair <host> --pin 4831`.
2. Sunshine `v2026.906` and later list waiting requests (`GET /api/pin`: id, client name, address). The host's Caskade shows "Bedroom PC wants to stream games from this device" with a code field, and submits it with `POST /api/pin {pairing_id, pin, name}`. Decline cancels it with `DELETE /api/pin`. Requests expire after five minutes.
3. Both ends list the pairing; the host can remove it (`POST /api/clients/unpair`), and the client forgets the host by deleting it from its state.

Only admins pair on the host. A client account needs no special permission.

#### Built (#263): no Moonlight window at all

Moonlight's `pair` opens a window (its AppImage has only Qt's `xcb` platform, so no offscreen mode), so Caskade pairs over the GameStream protocol itself (`streaming::pairing`), with the identity Moonlight keeps in Caskade's client state:

- **Identity:** Moonlight's certificate, key and unique id from `[General]`. When there is none yet, a `list` of an address nobody answers makes one; that run opens no window (only Qt's invisible selection owner), and SIGTERM saves it.
- **The exchange:** `getservercert` (a salt and the client certificate; answered once an admin entered the code on the host), `clientchallenge`, `serverchallengeresp`, `clientpairingsecret` and `pairchallenge` over HTTPS with the client certificate. AES-128-ECB keyed by `SHA-256(salt ‖ code)`; each side signs a secret with its key (RSA, SHA-256).
- **Outcomes:** a hash mismatch is a wrong code (the host is told `unpair`); a 400 is a declined request; no answer within five minutes is an expired one. Each stage gets a new connection, because the host closes idle ones.
- **Recording the host:** the host goes into Caskade's client state as Moonlight records it (`[hosts]`: uuid, name, address, `srvcert`). The real Moonlight then lists that host's apps, which it does only for a paired host.
- **On the host:** `GET /api/pin` lists waiting requests (`id`, `name`, `address`), `POST /api/pin {pairing_id, pin, name}` enters the code, `DELETE /api/pin {pairing_id}` declines, and `/api/clients/list` and `/api/clients/unpair {uuid}` list and remove devices.
- **Commands:**
  - `streaming_pair_with` (signed in) answers the code at once ("Enter 4831 on Den"), and `StreamingPairingFinished` follows with the outcome and Caskade's message.
  - `streaming_pair_requests`, `streaming_accept_pairing`, `streaming_decline_pairing`, `streaming_paired_devices` and `streaming_unpair` serve the host; accepting, declining and unpairing are admins only.
  - `streaming_forget_host` forgets a paired device on this one.
- Checked live on 2026-11-02 between the real Sunshine 2026.914 and moonlight-qt 6.2.0's identity: a wrong code, a declined request, then a pairing that the host lists and from which Moonlight lists the apps.

### 6.3 Pairing through sync

Once devices are paired through sync (#226) and exchange changes (#230), the client sends its code to the host over `caskade/sync/1` and the host submits it unprompted: paired devices stream with no code. Sync's view of other devices' installed copies (#232) then names exactly which game each app is (§8.2).

#### Built (#267)

- **Pairing requests carry the client's device name** (`Pairing.device_name`, `StreamingClient::pair_as`): Caskade's name for this device, so a host's prompt shows it, and a handover can find the request by it.
- **The client** (`streaming::linked::LinkedPairing`) starts the usual pairing (§6.2), then offers `{"type":"pair","host","request","code","name"}` to every device paired through sync at once. It opens a `caskade/sync/1` session with purpose `stream` (engineer 1's `Purpose::Stream`, admitted like `sync` and `copy`: paired devices with their grant, never forgotten ones).
- **The host's device** (`HostHandover`, `Devices::set_stream_handler`) answers `accepted`, `notHere` or `refused`:
  - `notHere` unless it is hosting (Ready) and its `/serverinfo` `uniqueid` is the host being paired with.
  - Otherwise it waits up to 10 s for the pending request with that name, enters the code (`accept_pairing`) and answers `accepted`.

  `streaming_pair_with` then answers `PairingStarted.automatic`, and Pair… shows "<device> is one of your devices, so it's pairing without a code" instead of the code. A device not paired through sync can't open the session, so it still needs the code.
- **Names, not titles.** A paired host on a device paired through sync (matched by address, `StreamHosts::set_devices`) names its apps by that device's own copies (#232). `LibraryStore::sync_stream_apps_from` joins each app to the game of that device's `peer:` copy with the app's title before any other game of that title.
- **Tests.**
  - `streaming::linked::tests`, two in-process devices on loopback paired through sync: the host enters the code; another host answers `notHere`; a device not paired through sync has nobody to hand the code to and is refused outright; the host answers only for itself and only while hosting.
  - `streaming::hosts::tests`: a synced device's copy wins over a title elsewhere.
  - `test/streamingSettings.test.tsx`: pairing with no code shown.
  - Live (`caskade-sources` `streaming::live_linux_sync_paired_devices_stream_with_no_code`, Xvfb): two devices paired through sync. The real host takes the code over sync, the client pairs and streams. A third, unpaired device can't hand a code over.

## 7. Updates and repair

- Both programs follow `core.updates` like managed emulators: the watch checks GitHub's `releases/latest` redirect without the API, an update shows in "Updates available" with Update and Skip, and nothing updates itself. Neither program has its own updater; Moonlight's check only shows a toolbar button that command-line launches hide.
- Updating the host stops it, swaps the files, re-applies Caskade's configuration and Linux rules if the release changed them, and starts it again; never during a stream. Updating the client never interrupts a stream.
- **Repair** re-runs setup: missing files, a configuration that drifted, rules removed by the OS, the service gone, credentials lost from the keychain (new ones, then pairing again).
- **Turning hosting off** stops the host and removes its program, service, firewall rules and Linux rules (one administrator prompt). Its configuration, credentials (keychain) and paired clients stay, so turning hosting back on reinstalls it and every paired device streams again with no new code. Removing a device unpairs it; a Forget all paired devices button clears them.

#### Built (#266)

- **Updates.** Settings → Streaming → Streaming updates lists each program by Caskade's name for it, with Update and Skip this version (`streaming_programs`, `streaming_update`, `streaming_skip`; admins). The "Updates available" notification links there.
  - **The host** (`StreamingHost::update`) refuses while a device streams: its own `/serverinfo` says busy. Otherwise it stops, `ManagedTools::update` swaps the files, the configuration is written again, the administrator step re-applies what's needed, and the host starts again.
    - On Linux, the configuration folder (state, paired devices, certificate, published games) isn't shipped by the release, so it stays.
    - On Windows, the new staging copy differs from the installed one (`elevation::windows::host_stale`, which ignores the host's own files), so the same one-prompt step copies it into the administrator-only folder.
  - **The client** (`StreamingClient::update`) refuses while a stream holds it. Its state folder stays.
- **The host's own files on Windows.** These are `config\sunshine_state.json` (whose `root` holds the paired devices and the host's id), `config\credentials\` (its certificate and key) and `config\apps.json` (`windows::RUNTIME`).
  - A new copy (an update, a repair) carries them over from the copy it replaces. The state keeps the new copy's sign-in and the old one's `root` (`merge_state`).
  - Switching hosting off sets them aside in `%ProgramFiles%\Caskade\Streaming\kept`, still administrator-only, before removing the host folder. Switching on again restores them from there and removes `kept`.
  - No program file, service or firewall rule remains.
- **Repair** (`StreamingHost::repair`, `streaming_repair`; the Repair button whenever hosting is on):
  - reinstalls when a shipped file is missing (`ManagedTools::intact`);
  - writes the configuration again;
  - sets the sign-in again, with new credentials when the keychain lost them;
  - re-applies whatever of the administrator step went (rules, the service, the firewall, a stale Windows copy) behind one prompt.

  Paired devices stay.
- **Forget all paired devices** (`HostApi::unpair_all`, `/api/clients/unpair-all`; `streaming_unpair_all`).
- **Tests.**
  - Host: an update refused during a stream, then keeping pairings, published games and settings with no prompt (`host::tests`). Repair fixing a missing program file, drifted configuration, removed rules, credentials lost from the keychain and a drifted sign-in. Off leaving no program, rule or record, and on again with the same pairing and sign-in.
  - Windows elevated side (`elevation::tests`): a new copy keeping pairings, identity and games with a new sign-in; off keeping them aside with no program file; on again restoring them; a changed or damaged copy copied again behind one prompt.
  - Windows for real (CI's `windows-elevation` job, `tests/windows_elevation.rs`, ignored, `CASKADE_TEST_NATIVE=1`): `windows::perform` with stand-in files makes and removes the real service, firewall rules and Program Files folder, and spares a neighbouring service, rule and app. The runner is an administrator with no UAC, so it proves the commands, not the prompt.
  - Client: an update refused during a stream, then keeping its state.
  - UI (`test/streamingSettings.test.tsx`): Update refused during a stream, Update and Skip, Repair, Forget all.
  - Live (`caskade-sources` `streaming::live_linux_pairing_survives_hosting_off_and_on`, Xvfb): the real host, paired natively, switched off (the program gone, nothing answering) and on, then the device paired before streams with no new code.

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

#### Built (#264): Play streams, Caskade checks first

- **The release.** Each host app is a release with `LaunchSpec::Stream { host, app }`: the host's id and the app's name there. `LibraryStore::sync_stream_apps` sets it on new releases and on releases from before #264. The webview counts a stream release as launchable, though nothing is installed. Users can't add stream actions, and an added action never replaces a stream release's own launch.
- **The launch.** `LaunchService::start` sends a stream release to `launch/stream.rs` before any action, runner or session:
  - The play gate has already asked for the client account's age limit.
  - The game's lease is taken, and one stream runs at a time.
  - The `StreamLauncher` (`caskade_core::streaming::play::Streams`) prepares the stream (below).
  - Moonlight starts in its own process group, with stderr read line by line.
  - Moonlight's exit code says nothing, so its lines decide. `Launch response:` (or `Resume response:`) means the host answered. Three quiet seconds after that, the stream counts as up. A `Qt Critical:`/`Qt Fatal:` line before then, an exit, or 60 s without an answer ends Moonlight. The launch then fails with Caskade's words.
  - Once up, the stream is followed like a session (`running_sessions`, `running_streams`, which `list_sessions` puts first, Stop, the guide menu), but nothing is stored. It emits `game.started` and `game.exited`, never `launch.after`, so no saves are captured and no playtime counts here. The host's own `caskade --launch` records the session.
- **The checks** (`Streams::prepare`), against the host itself (`streaming::gamestream::HostLink`):
  - **Pairing record.** The host must be in Caskade's client state, with its certificate.
  - **Reachability.** It must answer plain `/serverinfo` where it was last found or recorded. If it doesn't and its MAC is known, Caskade sends Wake-on-LAN magic packets (broadcast and its addresses, ports 7, 9, 47998–48000, 48010) and waits up to 45 s.
  - **HTTPS as the paired client.** Caskade uses Moonlight's certificate and pins the host's. The host must still know this device: `PairStatus`, and a refused certificate counts as forgotten.
  - **The game.** The host must still offer it (`/applist`, by name, case aside).
  - **Busy host.** If the host is playing another game, Play asks: `LaunchError::HostBusy` becomes `LaunchOutcome::HostBusy`. Answered with `LaunchChoice.stopHost`, Caskade sends `/cancel` and waits for the host to be free.
  - **Moonlight's arguments.** `stream <address> <app> --display-mode fullscreen --no-quit-after`, plus `core.streaming.quality` (resolution, fps, bitrate).
  - **TLS sessions.** Sunshine answers a resumed TLS 1.3 session with an internal-error alert, so the pinned client config turns resumption off.
- **Moonlight's lines, in Caskade's words:**
  - "Failed to find application" → "<device> no longer offers <game>."
  - "has not been paired" → "<device> no longer knows this device. Pair them again in Settings → Streaming."
  - "Failed to start the specified application" → "<device> couldn't start <game>."
  - Anything else → "Streaming from <device> failed."
- **Ending.**
  - The host's game ending ends the stream: Moonlight exits within 2 s.
  - End stream (the game page, the game menu, the guide menu) sends `/cancel` first; the stream then ends and Moonlight exits by itself. Then Moonlight gets SIGTERM to its group (so it saves its settings), then SIGKILL after 5 s; on Windows it is ended.
  - However a stream ends, the game stops on the host if it is still this stream's game. Moonlight's `--quit-after` does nothing on SIGTERM.
  - Caskade exiting kills Moonlight (`kill_on_drop`).
- **Moonlight's windows.**
  - Before the stream, Moonlight shows its main Qt window, 1280×600, for about 2 s. With `-qwindowgeometry 1x1+<x>+<y>` (Qt's own option), it shows as one pixel in the middle of Caskade's window instead. Negative X offsets count from the far edge, so a middle left of or above the main screen gets the size only.
  - `stream.starting` (`LaunchHooks::stream_starting`) keeps Caskade's window always on top until `game.started`, or `game.exited` for a stream that didn't start. Caskade's window then minimizes, or hides to the tray, in both modes, uncovering the stream's window beneath it (`game_focus::making_for_stream`).
  - Nothing shows after the stream.
- **UI.**
  - The game page's Play button is "Stream from <device>" for a stream release, and the release rows say "Stream". While streaming the page shows "Streaming" and "End stream".
  - The game menu names its Play item "Stream from <device>" when that is what Play starts. It adds an item per other device, and its Stop becomes "End stream".
  - The guide menu says "is streaming from another device." and offers End stream.
  - `StopHostDialog` asks "<device> is playing <game>. Stop it and stream <title>?".
  - Stream releases skip Play's update offer, the played-elsewhere warning and the saves check: those belong to the host.
- **Tests.**
  - `launch::stream_tests`: a fake client script and a fake `StreamLauncher` cover success, End stream, the host's game ending, an error before and while settling, an exit, a timeout, a busy host and a second stream. They check that no session is stored and no client process is left.
  - `streaming::play::tests`: a wiremock host behind a TLS front covers the arguments with quality, a forgotten device, a missing game, the busy question and `/cancel`, `finish` stopping only its own game, Wake-on-LAN to a local socket, and the stderr mapping.
  - `test/streamFlows.test.tsx`: the game page, the menu, a pad in console mode, the busy question and the guide menu.
  - `caskade-sources` `streaming::live_linux_streaming_plays_from_the_host` (ignored, Xvfb): the real host and client pair natively and stream through the pipeline. It checks that the host is busy, End stream frees it, and no client is left (no process in the state folder). Only the stream's window and the one-pixel start window are ever mapped. A missing game, a forgotten device and a host that's gone each fail in Caskade's words.

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
- **Decided 2026-11-02 (lead): the Windows host runs from an administrator-only folder.** A service running a launcher from the user's data folder would let anyone who can write that folder run code as SYSTEM. So the elevated step copies the host program (`sunshine.exe`, `tools\sunshinesvc.exe` and what they load) into `%ProgramFiles%\Caskade\Streaming\host`, which inherits Program Files' ACL (users read and execute only). The copy is checked against the SHA-256 of the files Caskade downloaded and checked; the service and firewall rules point only at that folder. Download and staging stay in `<data>`, unelevated. An update (#266) stages the new release the same way and applies it with the same one-prompt step: never silently, only when an update is installed. Turning hosting off removes the folder in the undo prompt. Security wins over prompt-free updates: a standard user must not gain SYSTEM through hosting. Sunshine's configuration lives in that folder too (`config\`), because `sunshine.conf` can name commands Sunshine runs elevated (`prep` commands): Caskade writes it in staging and the same step copies and checks it. Caskade changes apps afterwards only through the API.
- **Linux has no such risk.** The udev rules and module list change device permissions; they run nothing from a user-writable folder. Sunshine runs as the user, as Caskade's child, from `<data>/streaming/host`.
- **Still open: the web API's credentials.** Whoever holds them can add an app that runs elevated on Windows. Caskade keeps them in the signed-in OS user's keychain; on a machine shared by a standard user, that user can read them. Hosting is for an administrator to switch on (§8.5); a standard user on a Windows host is a known limit until Sunshine can turn elevated apps off.

### 8.5 Settings and UI

- **Settings → Streaming** (admins; namespace `core.streaming`): the switch "Let other devices stream games from this one", the host's state (Ready, Setting up, Needs attention with Repair), paired devices with Remove, and waiting pairing requests. Client side: known devices, Pair with…, Add by address, and stream quality (Automatic, or resolution, frame rate, bitrate) passed as `stream` options.
- The game page and Play button (§8.2), the pairing prompt as a notification and dialog on the host, and setup jobs in Activity.

#### Built (#265)

- **Settings → Streaming** (`views/settings/streaming/`) is shown to every account, like Devices. Hosting and accepting pairings are an admin's (`useDeviceAdmin`; the core refuses anyone else). Pairing this device with another is anyone's.
  - **Let other devices play:**
    - The switch "Let other devices stream games from this one".
    - The state: Off, Setting up…, Starting…, Ready, or Needs attention with the core's problem and Repair. Repair switches hosting on again, which redoes only what's missing.
    - What a dismissed or refused administrator prompt means.
    - Someone else's streaming (`existingFound`) asks for its user name and password.
    - While Ready: the devices waiting to pair, each with its code and a name, Pair or Decline (`components/streaming/PairRequestForm.tsx`), and the paired devices with Remove.
  - **Play from other devices:** every device found or added, with its state (Ready, Playing another game, Asleep: it wakes when you stream, Can't be reached), paired or not, and how many games it offers. Pair… opens `PairDialog`: the code to enter on that device, then Paired, a wrong code or an expired request (Try again), or declined. Paired devices have Forget. "Add a device by its address".
  - **Stream quality:** resolution, frame rate and bitrate, each Automatic by default (`core.streaming.quality`, #264).
- **The pairing prompt.** `components/streaming/StreamPairingHost.tsx`, in the shell, shows a request over any view while this device hosts and an admin is signed in, with the same form. It stays away while Settings → Streaming is open. A request expires after five minutes, so it is a prompt, not an entry in the notification feed.
- **Activity.** Hosting's download and setup show as "Setting up streaming from this device". The client's first install shows as "Getting streaming ready" (#261).
- **The game page and the game menu.** A streamed release says its device's state when it isn't ready: the Release picker ("Stream from Living room (Asleep: it wakes when you stream)"), the release row and the menu items' descriptions. Releases have no launch-time choices of their own (no actions, profiles or discs), so `LaunchChoiceDialog` and `PlayChoice` show nothing new.
- **No technology names.** No string a user sees names either program.
- **Pads.** The tab works with a pad through the shell's spatial navigation, and dialogs focus their first control.
- **The webview's API.** `StreamingApi` (`api/streaming.ts`) is the native core in the app. The browser preview gets `api/streamingFixtures.ts`, which tests drive: `finishPairing`, `request`.
- **Tests.** `test/streamingSettings.test.tsx`:
  - hosting on, a request paired and the device removed;
  - someone else's streaming used with its sign-in;
  - pairing with a wrong code, then the right one;
  - adding a device by address, and quality saved;
  - a standard account's view;
  - a pad turning hosting on and pairing in console mode;
  - a request prompting over the Library, answered with a pad.

  `test/streamFlows.test.tsx` checks a device's state on the game page.

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
| ST10 | Pairing through sync (§6.3), after Gitea #226 and Gitea #230. |

### Manual checks

Each item names its GitHub issue; record the date and result here.

VM runs on 6 Nov 2026 (Linux and Windows hosts, Linux and Windows clients) are recorded in `docs/manual-checks.md`, "What ran in virtual machines". Boxes below stay for real hardware.

- [ ] #21 Linux: switching hosting on shows one pkexec prompt, and cancelling it leaves `/etc/udev/rules.d` and `/etc/modules-load.d` unchanged. After allowing it, the rules and module list are there, `/dev/uinput` and `/dev/uhid` belong to group `input` with a seat ACL, and uhid is loaded. Switching hosting off removes exactly those, with one prompt.
- [ ] #21 Windows: one UAC prompt (Caskade, verified publisher); cancelling changes nothing (`sc query CaskadeStreaming` fails, `netsh advfirewall firewall show rule name="Caskade streaming (TCP)"` finds none). After allowing it, the service is demand-start, a standard user can `sc start` and `sc stop` it, and both firewall rules name the host's `sunshine.exe`. A host path with a space and an apostrophe works. Switching hosting off removes the service and both rules, with one prompt.
- [ ] #23 Linux, clean machine: switching hosting on shows one prompt, and Moonlight on another device pairs (§6.2) and streams a game. No `~/.config/sunshine`, desktop entry, tray icon or browser page appears. Closing Caskade to the tray keeps the stream available; Quit ends it; killing Caskade (`kill -9`) ends the host too.
- [ ] #23 Windows, clean machine: the same with one UAC prompt. Services shows only "Caskade streaming", and firewall rules only "Caskade streaming (TCP/UDP)". `%ProgramFiles%\Caskade\Streaming\host` grants users read and execute only. A standard user can't change the host's files.
- [ ] #27 per OS: the first Stream from a device installs the client under "Getting streaming ready", then streams. A Moonlight the user installed keeps its hosts and settings afterwards. No warning toast, overlay or Discord presence names Moonlight during or after the stream. Flatpak: `flatpak run --cwd=` keeps the state in `~/.var/app/com.moonlight_stream.Moonlight/caskade`.
- [ ] Gitea #262 (Gitea closed it and it has no GitHub issue, so this clears no box): a host on the LAN appears in `streaming_hosts` within seconds, and shows unreachable after it leaves the network. After pairing (#28), its games show as "Stream from <device>" on the matching library games.
- [ ] #24 per OS: with Sunshine installed the usual way, switching hosting on offers to use it, and the right password works, a wrong one doesn't. Caskade's games appear in its apps beside the user's; switching off removes only Caskade's.
- [ ] #25 per OS: a client lists the host's games with their covers, plus "Caskade". Starting one plays it on the host with a session there, and quitting the stream stops it. Installing, uninstalling, hiding or renaming a game changes the client's list after a refresh.
- [ ] #28 per OS pair: Pair with a device shows a code; entering it on the host's Caskade pairs with no window from either program. A wrong code says so and can be tried again; an unanswered request disappears from the host after five minutes.
- [ ] #29 per OS pair, the Steam Deck as a client included: Stream from <device> on the game page, the Play button and the game menu starts the stream with no Moonlight window in sight before or after (Cinnamon, GNOME, KDE, Windows, macOS, gamescope). The pad plays the game; guide brings up the quick menu and End stream returns to Caskade with the game stopped on the host. A host asleep wakes; one switched off, one that forgot this device and a game it no longer offers each show Caskade's error with no Moonlight process left. The host's playtime grows and the client's doesn't.
- [ ] #30 per OS: a new user switches hosting on, pairs a device and streams a game without leaving Caskade or seeing a technology name; the same with a pad in console mode on the TV, the request prompt included.
- [ ] #31 per OS: after switching hosting off, nothing of the host remains: Linux has no `/etc/udev/rules.d/60-caskade-streaming.rules` or `/etc/modules-load.d/60-caskade-streaming.conf`, and no program under `<data>/streaming/host`. Windows has no `CaskadeStreaming` service, no "Caskade streaming" firewall rule, and no program file under `%ProgramFiles%\Caskade\Streaming` (only `kept\`). Switching on again reinstalls it, and a device paired before streams with no new code. An update of either program from Settings → Streaming keeps pairings and settings. On Windows it applies with one UAC prompt into the administrator-only folder. Each repair case works on a real machine.
- [ ] #26: a 1080p client streaming from a 1440p Windows host gets a 1080p stream, and the host's mode comes back afterwards (HDR too, on an HDR screen). A pad on the client drives the game on a Linux host (X11 and Wayland, the portal's approval asked once) and on a Windows host, with rumble. A Windows host where ViGEmBus was already installed keeps it and works, and one without gets it in the same UAC prompt (`/exenoui /qn /norestart`; W3 in `docs/manual-checks.md` ran the installer silently on Windows Server, where it refuses, so installing it needs a client edition). A host with no screen, or whose pads don't work, says so in Settings → Streaming.
- [ ] #32: two of the household's devices, paired in Settings → Devices, pair for streaming with no code typed. A host on a synced device names each streamed game as that device's installed copy.
- [ ] #22 Windows: with Caskade running, `caskade.exe --launch <game id>` starts the game; `taskkill /PID <caller>` (WM_CLOSE, without `/F`), or ending a Sunshine stream, quits the game and ends its session, and the caller exits 5.

### Testing

- Tool definitions: asset patterns match the names recorded in this plan (table test).
- The Sunshine API against wiremock (apps, pins, clients, config, an expired request, a refused login), including the pinned certificate and a wrong one.
- Moonlight through a fake executable that prints its CLI's outputs (`list --csv`, `Qt Critical:` errors), for the launch and error paths. Pairing speaks the GameStream protocol itself, so its tests use a fake host playing the server's half.
- `caskade --launch` on Tauri's mock runtime: forwarding, `PlayGate`, staying alive until the session ends, and stopping the game when the caller's channel closes, which is what a signal does on POSIX. No test sends a signal or Windows' WM_CLOSE; the Windows check above covers WM_CLOSE.
- `live` tests (ignored) download the real releases into a temp folder; a manual checklist covers a real stream per OS pair, Wake-on-LAN, a Windows host's display mode and gamepads, and the Steam Deck as a client.

## 10. Risks and open questions

- **ViGEmBus is unmaintained.** If a Windows update stops it loading, Windows hosts stream keyboard and mouse only until a signed, compatible replacement exists (§4.5). Caskade checks that the driver answers when hosting starts and says so if it doesn't.
- **Linux host capture on Wayland:** the portal's one-time approval is the only prompt Caskade can't hide.
