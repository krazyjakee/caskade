# Platform support and deferred checks

What the Playnite milestone (#194) runs on, how each part was verified,
and what is left for native hardware or real accounts. Record each
deferred check's result with the commit you tested.

How things were verified:

- **Run**: tests ran on Linux (x86_64, X11; live UI checks under Xvfb with
  isolated profiles).
- **Lint**: `cargo clippy -D warnings` passes for `x86_64-pc-windows-gnu`
  (`-p caskade-core -p caskade-desktop --all-targets`). Nothing ran on
  Windows.
- **macOS**: the app can't be cross-linted (bundled SQLite, `ring` and
  Tauri's `objc2` helpers need the Apple SDK), so macOS-only code paths
  have not been compiled. OS-independent code is the same code that runs
  on Linux.
- **Fakes**: wiremock services, recorded fixtures, fake launchers and a
  scripted pad. Live tests against real services are `#[ignore]`d and need
  the user's accounts.

## Support

| Area | Linux | Windows | macOS | Verified with |
| --- | --- | --- | --- | --- |
| Store accounts: connect, sign-ins, sync health (#164, #165) | Run | Lint | Not compiled | wiremock providers, in-memory secrets, IPC tests that check no answer carries a token, Vitest panels |
| Steam, GOG and Epic libraries (#166–#168) | Run | Lint | Not compiled | wiremock with recorded fixtures; `tests/account_libraries.rs` connects all three through the app's commands |
| Play time from stores (#169) | Run | Lint | Same code | db tests, plus a launch through `LaunchService` in `tests/account_libraries.rs` |
| Owned games: Ready to Install, install through the launcher (#170) | Run (fake launcher) | Lint | Not compiled | fake opener, discovery over a fixture machine, pad flow in `src/test/storeBackupFlows.test.tsx` |
| Backups, restore, schedule (#191–#193) | Run | Lint | Not compiled | core tests, `tests/backup_roundtrip.rs` (an isolated profile restored through startup), Vitest and pad flows |
| Game edits and overrides (#171) | Run | Lint | Same code | core tests and `tests/milestone_editing.rs` (edit, bulk edit, IGDB refresh from a fake server, rescan, restart) |
| Game editor, selection and bulk edit (#172, #173) | Run | — | — | Vitest with mouse, Shift+click and a fake pad, in both modes; a WebKitGTK live check |
| Manual games (#174) | Run | Lint | Not compiled | core tests, `.lnk`/`.url`/`.desktop` parser fixtures run on Linux |
| Artwork: choose, import, reset (#175) | Run | Lint | Not compiled | core tests, Vitest with `fakeDrops` |
| Refresh details (#176) | Run | Lint | Same code | wiremock IGDB and Hasheous |
| Add-on catalogue: client, packages, install and rollback (#181–#183) | Run | Lint | Not compiled | wiremock; a live run of the app under Xvfb from a signed local catalogue (install, update, a failed update rolled back, a permission increase waiting for consent) |
| Catalogue tooling and starter collection (#184) | Run | — | — | reproducible builds checked; a clean-profile install of the built starter |
| Launch actions (#177, #178) | Run (fake processes) | Lint | Not compiled | core and adapter tests |
| Lifecycle scripts (#179, #180) | Run | Lint | Not compiled | core tests with an injected executor and real `sh`/`bash` |
| Achievements (#185–#190) | Run | Lint | Same code | wiremock for Steam, GOG, Epic and RetroAchievements; the adapters are OS-independent |
| Achievements on game pages (#190) | Run | — | — | Vitest by fake pad in both modes; a WebKitGTK live check with the keyboard |
| Keyboard, mouse and pad flows | Run | — | — | Vitest harness (`src/test/`: pad, keyboard and settings flows), `console_e2e` under Xvfb with a scripted pad |

## Differences between systems

- **Store sign-ins** go to the OS keychain under `dev.caskade.store-accounts`:
  Secret Service on Linux, Credential Manager on Windows, Keychain on macOS.
  Without one, a sign-in lasts until Caskade closes, and Settings says so.
- **Installing owned games** opens the store's launcher: `steam://install/<id>`,
  `goggalaxy://openGameView/<id>` or `com.epicgames.launcher://apps/…?action=install`.
  GOG Galaxy and the Epic Games Launcher have no Linux builds, so on Linux
  GOG and Epic games say their launcher isn't installed. Heroic is not an
  install target, though copies Heroic installs are found and joined.
- **Lifecycle scripts**: `powershell` and `cmd` are Windows-only, and `pwsh`
  runs wherever it is installed. On Windows, cancelling or timing out stops
  only the script's own process, not its children (no Job object). On Unix
  the whole process group is stopped.
- **Manual games**: `.lnk` and `.url` are Windows shortcuts and `.desktop`
  files are Linux's; URIs work everywhere. A Windows program chosen on
  another system is marked `windows` and runs through a compatibility tool.
- **Add-ons**: catalogue entries list the systems they run on. GameMode is
  Linux-only, and its install elsewhere is refused with the reason.
  HowLongToBeat isn't offered while its site refuses requests (403).
  Catalogue and signature checks are the same everywhere.
- **Webviews**: the UI was checked live only in WebKitGTK. Two of its fixes
  (plugin stylesheets loaded with a `<link>`, 19fc6c8; shell roots that
  focus can't scroll, f15e224) work around WebKitGTK behaviour. On Windows
  Tauri serves custom schemes as `http://<scheme>.localhost`. The plugin
  `<link>` uses the address the core gives, so it should work there too,
  but that is unverified.
- **Launch action tracking** by process name or folder relies on `sysinfo`,
  whose process data differs per OS.

## Deferred: native hardware and real accounts

Only the user can do these.

Accounts and stores:

- Run the ignored live tests with real accounts:
  - `CASKADE_STEAM_PROFILE` and `CASKADE_STEAM_KEY` for Steam.
  - `CASKADE_GOG_CODE` for GOG: the address the login page ends on.
  - `CASKADE_EPIC_CODE` for Epic: the `authorizationCode` JSON.

  Run one store at a time:
  `cargo test -p caskade-core --lib store_accounts::steam::tests::live -- --ignored --nocapture`
  (`gog` and `epic` likewise).
- Connect each store in the app, including the browser sign-in returning
  through `caskade://store-auth`, on Windows, macOS and Linux.
- Check that each sign-in is in the OS keychain and in no file under the
  profile.
- Use Ready to Install's Install with the real Steam client (and on Windows
  or macOS, GOG Galaxy and the Epic Games Launcher). Let the install
  finish, then check that the copy joins the owned game and that Play
  records time.
- The GOG and Epic endpoints are unofficial: check that they still answer.
- Run the achievements live tests with real accounts, including
  RetroAchievements. Disc images can't be hashed, so link those by hand.

Backups:

- Back up to a removable drive and unplug it. The next automatic backup
  says the folder isn't there and retries.
- Restore on Windows and macOS, both from Settings → Backup and with
  `caskade --restore <backup>`.
- Restore a backup made on another OS. Missing storage locations and
  games' folders are named before anything changes.

Library editing and the UI:

- Look over the game editor, selection and bulk edit, achievements and the
  shell in WebView2 (Windows) and WKWebView (macOS). Check that plugin
  stylesheets load and that the shell can't be scrolled out of the window.
- Add a real `.lnk` and `.exe` as manual games on Windows, and a `.app`
  bundle on macOS.
- Use the native file picker and drop artwork on Windows and macOS.
- Refresh details against the real metadata server. Only the ignored
  `igdb::tests::live` reaches it.

Add-ons:

- Install, update and roll back add-ons on Windows and macOS. An update
  swaps folders by renaming them. On Windows, open files could block that
  rename (untested); the journal would then roll it back at the next
  start.
- Use the published catalogue origin, after the publish gate.
- Run `caskade-catalogue` on Windows and macOS.

Scripts and actions:

- Run lifecycle scripts with `powershell`, `cmd` and `pwsh` on Windows.
- Run launch actions with process-name and folder tracking on Windows and
  macOS.

Console and pads: see the [console hardware checklist](console-native-checklist.md)
(Steam Deck game mode, Steam Input on and off, the guide button,
suspend and resume).
