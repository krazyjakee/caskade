# Launch actions

A release (a `GameEntry`) can be started in more than one way: its own launch, plus named actions the user adds, such as a modded start, a mod manager or a configuration tool (#177). This page covers the model, how a launch picks an action, tracking, tools and where the code lives. The editor and the Play choice are #178. Lifecycle scripts (#179) are in [lifecycle-scripts.md](lifecycle-scripts.md); tools never run them.

## Model

- **Built-in action.** A release's own `launch` (`LaunchSpec`) is its built-in play action, with id `source:<entry id>` and the name "Play". Nothing was migrated: every release from before actions launches as it did, with the same entry and session ids.
  - Its source keeps the target current (rediscovery, automatic updates), so the editor can't change it. The user duplicates it into a new action instead.
  - The name and tracking can be changed. They're stored as `EntryActionSettings.builtIn`.
  - The exception is a manual release (`source_id` `manual`, `model::MANUAL_SOURCE`, #174). Its target is the user's, and saving the built-in action writes `entry.launch`.
- **Added actions** (`model::GameAction`, table `game_actions`, migration 0019) belong to one release and have:
  - a name;
  - a kind: `play` or `utility`;
  - a target: any `LaunchSpec`, so executable, URI, Flatpak, emulator, plugin or web;
  - tracking (`ActionTracking`);
  - a position.
- **Relocatable paths.** For a managed install, a relative program, working folder or tracking folder is relative to the install folder. It is stored relative, so it moves with the install. An absolute program, such as a tool outside the install, stays absolute.
- **Choice settings** (`EntryActionSettings`, table `entry_action_settings`):
  - `defaultAction`: a play action's id, or none for the built-in action.
  - `ask`: ask which play action to start.
  - These are separate from the game's default release (`Game.default_entry`). Play picks the release first, then the release's action.
- **What edits survive.**
  - A rescan keeps the entry's id, so its actions and settings stay.
  - When an import retires an entry into another one of the game's entries, its actions and settings move with its sessions.
  - Deleting an entry deletes its actions.
  - Deleting an action, or turning it into a tool, makes the release play its built-in action again.

## Choosing at launch

`LaunchService::launch_with(entry, choice)` picks the action before it reserves anything, so a missing or invalid action never leaves a session open:

1. `choice.action`, when given. A deleted action or a tool is refused with the reason.
2. With `ask` on and more than one play action, the launch returns `NeedsChoice` with `LaunchPrompt.actions` (`ActionOption`: id, name, whether it's the default). The webview launches again with `choice.action`. An emulator action can then ask for its profile or disc as before, so the answer carries `action` along.
3. The default action. If it's gone, the built-in action plays. If there's no built-in action, the first play action plays.

The chosen target stands in for the entry's `launch` (`launch::actions::apply`), and the launch continues through the usual pipeline: managed path resolution, the Windows runner, the emulator profile, the web player, `launch.before` hooks and tracking. A native program used as a tool for a Windows game runs without Wine; programs ending in `.exe`, `.bat`, `.cmd`, `.com`, `.lnk` or `.msi` still go through the game's runner. The session's recovery state records the action id (`RecoveryState.action`).

## Tracking

`ActionTracking.mode` decides which processes count as the game:

| Mode | Counts | The started process |
| --- | --- | --- |
| `automatic` | The release's usual tracking: install folder, discovery hints, Steam app id | As before |
| `original_process` | Only the process Caskade started | Is the game |
| `process_tree` | That process and everything it starts | Is the game |
| `folder` | Processes started from `folder` (relative to a managed install) or the install folder | Is the game only if it runs from there |
| `process_name` | Processes named in `names` (`game.exe`) | Isn't the game unless named |

A mod manager that starts the game should use `process_name` or `folder`. The session then starts when the game appears and ends when it exits, not when the manager does.

`launch::validate_tracking` refuses combinations that can't work and names the mode to use instead:

- a URI with `original_process` or `process_tree`, because the system's handler exits at once;
- a Flatpak with `folder`, because it runs inside its sandbox;
- any mode other than `automatic` for emulator or web targets, which follow the profile or the game window;
- `original_process` for a Windows program on Linux or macOS, because the started process is Wine or Proton, not the game;
- `process_name` without names;
- `folder` without a folder on a release that has none.

## Tools

A utility action runs with `LaunchService::run_tool(entry, action)`:

- It creates no play session and no playtime, and doesn't change last played.
- It runs no plugin `launch.before`/`launch.after` hooks.
- It isn't tracked once it starts.
- The play gate (age limits) still applies.
- It doesn't take the game's lease, so it can run while the game does.

## Validation

`launch::validate_target(entry, target)` checks a target before it is saved:

- a URI must use a launcher scheme;
- a Flatpak app id must be valid;
- an absolute program must exist;
- a relative program needs an install folder;
- an absolute working folder must exist;
- an emulator target needs ROMs.

The relative program of a managed install is resolved at launch. A missing one fails there with its name, before any session is reserved.

## In the app (#178)

- **Editor.** The game editor's Actions tab (`components/gameEditor/ActionsTab.tsx`) lists each release's actions: its own launch first, then the ones added.
  - The tab can add a play action or a tool, duplicate, move up and down, rename and delete (with a second press to confirm), and choose what Play starts or Ask each time.
  - Each action saves on its own. Unsaved changes mark the tab, and the editor asks before closing.
- **Form** (`components/gameActions/ActionForm.tsx`).
  - It shows what an action starts (a program, a launcher link or a Flatpak app), with one field per argument, a working folder, environment variables and, for play actions, which programs count as the game.
  - Path fields take Browse… (the native dialog on the desktop, the in-app folder browser in console mode, where the on-screen keyboard fills text fields) or a file dropped on them.
  - A found release's own launch shows its target read-only and says to duplicate it. An emulator, web or plugin target is described rather than edited.
  - The core's reason for a refused save shows under the form, and the draft stays.
- **Play.** When a release asks, Play's question (`LaunchChoiceDialog`) lists its play actions with the default chosen. The answer goes with any later profile or disc question.
- **Game page.** It shows "Play starts" (the default or Ask each time) and the release's tools under Play (`components/gameActions/PlayChoice.tsx`).
- **Game menu.** It offers "Play with …" for each play action when there are several, and a Tools section. A tool runs through the menu host (`ToolRequest`), which asks for an admin's PIN when the age limit needs one and says why a tool didn't start.

## Permissions and commands

| Command | Access |
| --- | --- |
| `game_actions` | Open (read) |
| `game_action_save`, `game_action_delete`, `game_actions_order`, `game_action_settings_save` | Install permission: changing what a game runs is adding games by hand, in effect |
| `game_tool_run` | Signed in, like Play |

`game_action_save` with an empty `entryId` adds the action to the game's manual release. If the game has no release yet (a title-only record), it creates one, and a play action created that way becomes that release's built-in action.

Code: `crates/caskade-core/src/model/action.rs`, `src/db/actions.rs`, `src/launch/actions.rs` and `src/launch/service/`, with the adapter's `src-tauri/src/commands/actions.rs`.
