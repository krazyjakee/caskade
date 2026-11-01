# Lifecycle scripts

Users can run their own commands around a game without writing a plugin (#179): before it launches, once its process has started, and after it exits. Scripts can be global, per emulator profile or per game. This page covers the model, ordering, failure behavior, how values reach a script, how scripts relate to plugin hooks, and the settings and Test UI (#180).

## Model

`caskade_core::scripts::LifecycleScript` is stored in the table `lifecycle_scripts` (migration 0019):

| Field | Meaning |
| --- | --- |
| `scope` | `global`, `profile` (an emulator profile id) or `game` (a game id). A game's scripts follow it into a merge and go when it goes. A profile's scripts go when the profile does. |
| `name`, `enabled` | A disabled script is kept but never runs. |
| `phase` | `before_launch`, `after_start` or `after_exit` |
| `command` | `direct`: a program (or a script file) and its arguments, started with no shell. `shell`: script text for `sh`, `bash` or `pwsh` everywhere, plus `powershell` and `cmd` on Windows only. A shell this OS doesn't have is refused when the script is saved. |
| `cwd` | The working folder. Unset, it's the launch's install folder when it has one. |
| `timeoutSecs` | 1–3600, 60 by default |

Each scope keeps its own order (`order_lifecycle_scripts`).

## Values reach a script as data

Every script gets `CASKADE_*` environment variables. A direct command can also use `{Placeholder}`s in its program, arguments and working folder:

| Placeholder | Variable | Value |
| --- | --- | --- |
| `{Phase}` | `CASKADE_PHASE` | `before_launch`, `after_start` or `after_exit` |
| `{GameId}`, `{GameTitle}`, `{EntryId}` | `CASKADE_GAME_ID`, … | The game and the release |
| `{ActionId}`, `{ActionName}` | | The play action launched (`docs/launch-actions.md`) |
| `{SessionId}` | | The play session |
| `{InstallDir}`, `{RomPath}` | | The install folder; the ROM or disc an emulator runs |
| `{PlatformId}`, `{EmulatorId}`, `{ProfileId}` | | For emulated games |
| `{Pid}` | `CASKADE_PID` | The game's process (after start) |
| `{Status}` | `CASKADE_STATUS` | After exit: `exited`, `crashed`, `interrupted` or `not_started` (a game that never started, or a start that failed), `failed` (a before-launch script failed) or `cancelled` |
| `{DurationMs}` | | The session's playtime (after exit) |
| `{Test}` | `CASKADE_TEST` | `1` when run as a test from the settings |

A placeholder expands inside its own argument and is never split, re-expanded or read by a shell. A title such as `$(rm -rf ~)` reaches the program as those characters. A shell script is never edited: it reads the variables, quoted as usual (`"$CASKADE_INSTALL_DIR"`). Unknown braces stay as they are.

## Order and failures

For one launch, the scripts are the enabled ones of the global scope, of the emulator profile the launch uses (emulated games only), and of the game.

| Phase | Order | A failure |
| --- | --- | --- |
| `before_launch` | global → profile → game | Stops the remaining scripts and the launch. The launch fails, naming the script and its exit code (`LaunchError::Refused`), and no play session is created. The after-exit scripts then run as cleanup with `{Status}` `failed`. |
| `after_start` | global → profile → game | Runs once the game's process is found, on its own task beside the game. Tracking never waits for it, and the game and its session carry on whatever happens. A failure becomes a notification (`script.failed`). |
| `after_exit` | game → profile → global | Each script runs even if an earlier one failed. The session is already recorded. A failure becomes a notification. |

Cleanup runs in reverse, so the innermost preparation is undone first.

Where scripts sit in the whole launch:

1. The play gate, the action choice, runner and emulator resolution.
2. Plugins' `launch.before` interceptors. They may rewrite the command or cancel the launch.
3. The emulator profile's own `before` command.
4. **`before_launch` scripts.**
5. The session is reserved and the game is spawned.
6. `game.started`, then **`after_start` scripts**.
7. After the game exits: `game.exited`, the profile's `after_exit` command, **`after_exit` scripts**, then `launch.after`.

**Each phase runs once per lifecycle.**

- After the before-launch scripts pass, a launch that then fails runs `after_exit` once with `{Status}` `not_started`: a reserved session, Wine prefix preparation or the spawn failing, or a game that never appears.
- The session's recovery state (`RecoveryState.scripts`) records the launch's context and whether `after_start` ran. When Caskade restarts and finds the game still running, it doesn't run `after_start` again, and `after_exit` runs when the game ends.
- If the game ended while Caskade was closed, `after_exit` runs with `{Status}` `interrupted`, on its own task, so startup doesn't wait for it.
- Stopping a game from Caskade ends it like any exit.
- Closing Caskade while a game runs leaves the session for recovery and runs nothing.

**What scripts don't run for.**

- Tool (utility) actions never run lifecycle scripts.
- Library imports, scans, metadata refreshes, restores and settings changes never run scripts. Only a launch does, or an explicit Test (see below).

## Limits

- **Output.** The last 64 KiB of stdout and of stderr are kept, as text without control characters (tabs and line breaks stay), and marked `truncated` when cut.
- **Time limit.** A script that runs past its time limit is stopped.
- **Cancellation.** `LaunchService::cancel_start` (the `launch_cancel` command) stops a launch while its before-launch scripts run. App shutdown stops after-start scripts.
- **Process groups.** On Linux and macOS a script runs in its own process group, and stopping it stops everything in the group. On Windows only the script's own process is stopped; programs it started in the background keep running.
- **Logging.** Only a script's id, phase and outcome are logged. Its arguments, environment and output are never logged.
- **Who sees results.** The results (`ScriptReport`) go out as the bus event `lifecycle.scripts.ran` and the webview event `LifecycleScriptsRan`. Plugins can't subscribe to that event.

## Shells by OS

| Shell | Linux | macOS | Windows |
| --- | --- | --- | --- |
| `sh` | yes | yes | if one is on `PATH` (Git for Windows, MSYS2) |
| `bash` | if installed | yes (3.2) | if one is on `PATH` |
| `pwsh` (PowerShell 7) | if installed | if installed | if installed |
| `powershell` (Windows PowerShell 5.1) | no | no | yes |
| `cmd` | no | no | yes |

If a script's shell or program isn't found, it doesn't start, and the result says which program was missing. `pwsh` on Linux and macOS isn't Windows PowerShell: Windows-only cmdlets and paths don't work there.

`cmd` reads its own command line rather than separate arguments, so a `cmd` script runs as `cmd /d /s /c "<script>"`: the text reaches it as typed, with quoted paths, quoted arguments and statements joined by `&` or `&&`. Like any `cmd /c` command it is one line; join statements rather than putting them on separate lines.

## Examples

These are direct commands unless marked.

- **Prepare** (before launch): `/usr/bin/rsync` with arguments `-a`, `{InstallDir}/../saves/`, `{InstallDir}/saves/`.
- **Cleanup** (after exit, `sh`): `pkill -f my-overlay || true`.
- **Clean up, Windows** (after exit, `powershell`): `Stop-Process -Name overlay -ErrorAction SilentlyContinue`.

## Configuring and testing (#180)

Each scope has the same editor (`components/scripts/ScriptsPanel.tsx`):

| Scope | Where |
| --- | --- |
| Global | Settings → Scripts, a tab only admins see |
| Emulator profile | The Profile scripts section under a saved profile's editor (Settings → Emulation) |
| Game | The game editor's Scripts tab |

- **The list.** It shows the scope's scripts in order, with the disabled ones marked Off. Add, Duplicate, Move up and down, and Delete (pressed twice) work on them. Each script saves on its own, and the game editor's tab is marked while a script is unsaved.
- **Copies start off.** Duplicate makes a copy whose "Runs with launches" is off, so copying never adds a running command unseen.
- **The form.** It covers the name, when the script runs, a shell script or a program with one field per argument, a working folder (Browse…, or drop a folder), the time limit, and what a failure in that phase does.
- **Test.** Test runs the script once, saved or not, through the launch's own `ScriptRunner` (`lifecycle_script_test`, `scripts::test`).
  - A game's script gets the game's values: its default release and that release's folder. Global and profile scripts get no game's values. `CASKADE_TEST` is `1` either way.
  - The result shows how it ended, the exit code, how long it took, the reason it failed or didn't start, stdout and stderr (the last 64 KiB of each, marked when cut), and the values it saw.
  - Cancel test stops it (`lifecycle_script_test_cancel`). Nothing runs unless Test is pressed.
- **Order at launch** (`lifecycle_scripts_effective`, `scripts::effective_order`). It lists the steps a launch takes in every phase, across all scopes. Steps from another scope are marked inherited, and disabled steps are struck through and marked Off.
- **Permissions.** A test needs the same permission as changing the scope.
- **Console mode.** The pad reaches every control. Text fields use the on-screen keyboard, and folders use the in-app folder browser.

## Permissions

| Change | Needs |
| --- | --- |
| A game's scripts | The Install permission: it changes what a game runs, like adding games by hand |
| An emulator profile's scripts | The Emulators settings section |
| Global scripts, which run with every game | An admin |
| Listing scripts | Nothing (it's a read) |
| Cancelling a starting launch | Being signed in |

## Why not plugin interceptors

Scripts run in the launch pipeline, at the same points as the emulator profile's own commands, rather than as `launch.before`/`launch.after` subscribers on the hook bus (#109/#125), for three reasons:

- They need the launch's context (release, action, ROM, install folder, the observed process), which the bus payloads deliberately don't carry to plugins.
- They must run exactly once per lifecycle across restarts, which needs the session's recovery state.
- Their output belongs to the user and shouldn't go to plugins.

Their order relative to the plugin hooks is fixed: see "Order and failures" above.

Code: `crates/caskade-core/src/scripts/`, `src/db/scripts.rs` and `src/launch/service/`, with the adapter's `src-tauri/src/commands/scripts.rs`.
