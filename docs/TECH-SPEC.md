# Tech spec

How Caskade is built today. Last reviewed 2026-11-08, at version 0.1.3.

What it does is in the [product spec](PRODUCT-SPEC.md). Public contracts for extension authors live beside this document: [plugins](plugins/README.md) (with `plugin.schema.json` and `wit/caskade-plugin.wit`) and [theming](theming/README.md) (with the generated [token](theming/tokens.md) and [anatomy](theming/anatomy.md) references).

## 1. Principles

1. **The core has no Tauri dependency.** `crates/caskade-core` is a plain Rust library: domain model, database, services, hook bus. `apps/desktop/src-tauri` is a thin adapter of commands, events, windows and URI schemes. The core is tested without a window.
2. **The webview never touches the OS.** Files, processes and the network go through typed commands to the core, which checks permissions. The webview gets no `fs` or `shell` plugin, and its CSP allows connections only to IPC, the app's own schemes and a few image hosts.
3. **Built-ins and plugins share code paths.** Each extension point is one provider trait. Built-in sources implement it natively; WebAssembly plugins are adapted to it.
4. **Local first.** Everything that can work offline does. Network answers are cached, a failed refresh keeps the last good answer, and caches are only evicted while online.
5. **Declared, not implied.** Every table and setting is classified for sync and for backups, every plugin capability is declared and granted, every user-set field is recorded as user-set. Guard tests fail when something new is unclassified.
6. **Nothing unasked.** Downloads, installs, elevation and destructive actions wait for confirmation. Background work is bounded and never prompts.

## 2. Stack

| Area | Choice |
| --- | --- |
| Language | Rust 2024 edition, MSRV 1.94; TypeScript 7 (`strict`) |
| App shell | Tauri 2.12 (WebView2, WKWebView, WebKitGTK). Plain tao + wry for the game and video windows. |
| Async | tokio: one 4-worker runtime (`caskade_app::runtime`), which Tauri shares. |
| Database | SQLite through `sqlx` 0.9 (bundled SQLite, in-repo migrations), FTS5 for search |
| IPC types | `specta` + `tauri-specta` (pinned RC). Generated bindings are committed. |
| HTTP | `reqwest` with rustls |
| Process tracking | `sysinfo` plus per-OS process identity |
| Plugins | wasmtime 50 (pinned), Component Model, WASI 0.2, guests built for `wasm32-wasip2` |
| Device sync | iroh 1.0.3 and iroh-blobs 0.103.1 (pinned) |
| Torrents | librqbit 9 (upload disabled) |
| Web game capture | hudsucker MITM proxy |
| Gamepads | `gilrs` (XInput backend on Windows) |
| UI | React 19, React Aria Components, TanStack Router, Query and Virtual, Tailwind CSS 4 over `--cs-*` tokens, Lingui, lucide-react, Vite 8 |
| Translations | gettext PO catalogs (`ui`, `app`, `core` domains) |
| Lint and format | rustfmt, Clippy with `-D warnings`, Biome |
| Tests | `cargo test` / nextest, insta, wiremock, Vitest with Testing Library, Storybook as a component workshop |
| Tasks | `just`, pnpm workspaces |

## 3. Repository

| Path | Contents |
| --- | --- |
| `crates/caskade-core` | Domain model, library store and the services in §6 that the rest of the core depends on. Feature `net` adds everything that needs `reqwest` or iroh. |
| `crates/caskade-achievements` | Achievements (Steam, GOG, Epic and RetroAchievements providers, the cached sets) and the notification feed. Feature `net` for the providers. |
| `crates/caskade-addons` | The add-on catalogue: the signed catalogue, compatibility, add-on packages and their journal, the author tools. Feature `net`. |
| `crates/caskade-backup` | App-data backups: verifiable archives of the library, settings and accounts, schedules, staged restore, backups held on other devices (`net`), and `policy` (what a snapshot leaves out). |
| `crates/caskade-share` | One-off shares of a game or a smaller item (save, web game, ROM, collection) through a link that can be stopped and may expire. Needs `net`. |
| `crates/caskade-streaming` | Game streaming: the managed Sunshine host and Moonlight client, pairing, host discovery, publishing, handover to a linked device, and the administrator steps (`elevation`) the host needs. Feature `net`. |
| `crates/caskade-system-stats` | Live system statistics for Settings → Statistics. |
| `crates/caskade-discovery` | Standalone: finds installed games from launchers, stores and the OS. Features per OS, `deep`, `watch`, `sqlite`, `cli`. |
| `crates/caskade-installer` | Standalone: fetches, probes, plans, installs, uninstalls, moves and reconciles payloads (installers, packages, archives, discs, torrents). Dev CLI. |
| `crates/caskade-sources` | Built-in library sources over discovery, emulator and platform definitions (`emulation/*.toml`), managed tool definitions (`tools/*.toml`). |
| `crates/caskade-sync` | iroh endpoint, device identity, pairing, the `caskade/sync/1` protocol, blob transfer, share links, swarm downloads. |
| `crates/caskade-plugin-host` | Manifests, discovery, the wasmtime runtime and capability enforcement. |
| `crates/caskade-plugin-sdk` | Rust SDK for plugin backends. |
| `crates/caskade-component-cache` | Cache of compiled plugin components. |
| `crates/caskade-theme` | Validates and compiles theme folders against the token manifest and anatomy catalogue. |
| `crates/caskade-input` | Gamepad input normalised to actions. |
| `crates/caskade-jobs` | The process-wide resource queue. |
| `crates/caskade-i18n` | Runtime translation for the Rust side. |
| `crates/caskade-malloc`, `caskade-webview2`, `caskade-xinput` | The only crates with `unsafe`: glibc allocator tuning, WebView2 COM calls, XInput's guide button. |
| `crates/caskade-emulation-fixtures` | Generated emulator and ROM fixtures for tests. |
| `crates/caskade-app` | The application layer every app shell runs, with no Tauri or other UI toolkit: building and starting every service (`start`: `open` for the library and what the window needs first, `start` for the rest), the async runtime, background supervision, command errors, permission checks, typed UI events, every command as a plain function over the services it uses (`commands/<area>.rs`), first-run setup, startup recovery, the installed-games discovery controller (startup scan, schedule and watch), `caskade --launch` (`launch_cli`: the caller, its channel and the running app's server), dev plugins (`dev_plugins`), making way for games (`game_focus`, behind a `GameWindow` the shell implements), deep-link and torrent-link handling, navigation targets. |
| `apps/desktop/src-tauri` | The Tauri adapter over `caskade-app`: one-line `#[tauri::command]` forwarders and the bindings, the webview events, windows, schemes, tray, the self-updater's plugin glue, deep links, the `--launch` caller's hidden window on Windows, elevation entry. |
| `crates/caskade-player` | The game and video windows (`caskade --web-game`, `--video`): tao and wry in a process of their own, which both app shells start; the webview recovery policy (#3) every Caskade webview follows, and WebKitGTK's renderer choice (`webkit`). |
| `apps/native` | `caskade-native` (#87): the main window rendered by Blitz (Dioxus Native) over `caskade-app`, without Tauri: one winit loop owns the main thread, the services start beside it (`caskade_app::start`), the window's place and size are kept in `native-window.json`, games make way through `GameWindow`, and web games and videos open through `caskade-player` in processes of their own. It runs its own winit handler around Dioxus Native's (`launch.rs`), so a close can hide the window to the tray: tray-icon and muda, on a GTK thread of their own on Linux (with the Cinnamon hide-before-exit ordering) and on the main thread elsewhere, with the tray's state shared with the Tauri app (`caskade_app::tray`). One instance per user (`caskade_app::single_instance`, a second endpoint on the `--launch` channel's code): a second start hands its arguments over and ends, and both starts' `caskade://` links, torrent files and `dev` folders go through `caskade_app::deep_link::open`. A copy that wasn't installed takes `caskade://` links only when no app has them (`caskade_app::link_handler`: an XDG desktop entry on Linux, the URL protocol under HKCU on Windows; the bundle declares them on macOS), or with `--register-links`. Dialogs are GTK's on the tray's GTK thread on Linux and rfd's elsewhere; a library that won't open offers a restore, or an update when a newer Caskade wrote it. Self-update (`caskade_app::self_update`) reads the same `latest.json` and checks the same minisign key and signed version as the Tauri app, then replaces the AppImage, installs the deb (one polkit prompt), runs the NSIS installer passively, or swaps the macOS app; development builds say they don't update. On macOS the menu bar has Caskade, Edit and Window menus, with Caskade's own Quit. Caskade's context menus are the UI's (#93); text fields are Blitz's own inputs, with input methods (composition events, the IME placed at the caret, the field's purpose), the clipboard, selection by mouse, keyboard and long press, and select all, copy, cut and paste for a field's menu (`Input::field_command`; BLITZ-5, #84). A panic in the window's UI restarts Caskade (a process gets one event loop) within the webviews' restart budget (`native-restarts`), then stops; a window restored off every screen comes back to the top left. Blitz's renderer is chosen at build time (vello_hybrid, on whatever adapter wgpu finds, Mesa's lavapipe included); with no usable adapter Caskade says so and stops instead of restarting, and a CPU renderer at run time is BLITZ-31 (#110). Xvfb smoke tests (`tests/smoke.rs`) check the window, a web game, a video, a clean quit, the crash restarts and a second start handing its link over; `CASKADE_NATIVE_SMOKE_GENERAL` sets `core.general` for a manual run of the startup settings. On Windows `build.rs` embeds the app icon and a manifest asking for Common Controls 6 (rfd's dialogs need it, or the exe doesn't start) and per-monitor DPI awareness. Ships beside the Tauri app until parity (BLITZ-33). Blitz, anyrender and Stylo build from `vendor/blitz` (#121): the BLITZ-0 pins with Caskade's patch series (damage-limited repaint of animation frames, step-aware animation ticks, `prefers-reduced-motion`, gradient fixes; #82; for screen readers, the AT-SPI backend on Linux, ARIA states and properties, names from content and labels, bounds, and actions routed to DOM events; #83), as standalone crates the root `Cargo.toml` patches in, regenerated and bumped through `vendor/blitz/README.md`; Dioxus is pinned with them (#113). Screen readers read the window through Blitz's AccessKit bridge (AT-SPI, UI Automation, NSAccessibility); `tests/accessibility.rs` snapshots the tree of demo elements and drives screen-reader actions (`docs/spikes/blitz-4.md`). Without Blitz's `net` feature (its reqwest would bring aws-lc back). |
| `crates/caskade-ui-data` | The native UI's data layer (#89), free of any UI toolkit: library reads and changes over `caskade-app`, which app events stale which data, the in-memory fixture library, the recovery hints' labels. |
| `crates/caskade-ui-input` | The native UI's input rules (#92), free of any UI toolkit: spatial focus over layout boxes, the keyboard map, command bindings, the action dispatcher and the on-screen keyboard's model, ported from the webview's `src/input`. |
| `crates/caskade-ui-theme` | The native UI's look (#90), free of any UI toolkit: Appearance settings as the root's `data-*` attributes and inline tokens, the user accent kept readable on the theme's surfaces (`accent.ts`, number for number), the user CSS check (`userCss.ts`, rule for rule), with the webview's test cases, and the anatomy catalogue's entries and generators, which write `anatomy.md` and `anatomy.json` as `docs.ts` does (CI checks they write today's files from `anatomy.ts`'s data). Native components list their entries in `apps/native/src/anatomy.rs`; tests check each against the webview's section and that every `data-cs` the native sources render is catalogued. |
| `apps/desktop/src` | The React app. |
| `apps/storybook` | Storybook for `@caskade/ui`, also a theme playground. |
| `apps/website` | Static English/Swedish landing page for GitHub Pages; dependency-free Node build, shared branding and tokens, self-hosted fonts. See its [guide](../apps/website/README.md). |
| `packages/ui`, `packages/tokens` | `@caskade/ui` (themeable components and the anatomy catalogue), `@caskade/tokens` (DTCG tokens compiled to CSS). |
| `packages/sdk`, `packages/vite-plugin`, `packages/create-plugin`, `packages/i18n` | Plugin UI SDK, build preset, scaffolder, translation tooling. |
| `themes/`, `plugins/examples/` | Built-in and catalogue themes, example plugins (dogfooded in tests). |
| `wit/` | The backend plugin contract, `caskade:plugin@0.1.0`. |
| `locales/` | PO templates and catalogs. |
| `xtask/`, `scripts/` | Fixture builders, the installer corpus score, CI scope, legal notices, catalogue builds. |
| `vendor/` | Vendored and patched third-party crates, each with a `PATCHES.md`: inno and innospect (resource limits), hudsucker and librqbit-sha1-wrapper (rustls and SHA-1 on ring, so no build compiles aws-lc). |

## 4. Runtime architecture

### 4.1 Processes and windows

One `caskade` binary has several entry points, decided in `run()` before logging and Tauri start:

| Invocation | What runs |
| --- | --- |
| `caskade` | The app: main window (React), tray icon, core services. A second start shows the running window (single instance). |
| `caskade --console` | The app, starting in console mode. |
| `caskade --launch <game> [--entry] [--action]` | A caller that connects to the running app over a per-user channel (a 0600 Unix socket or a local-only named pipe), starting it detached if needed, and lives as long as the game. Exit codes: 0 exited, 1 crashed, 2 usage, 3 refused, 4 failed, 5 stopped. |
| `caskade --web-game <url>` / `--bundle <dir> [--capture]` | The game window: tao + wry with no IPC, optionally behind the capture proxy. |
| `caskade --video <id>` | The video window for YouTube embeds, closed when Caskade exits. |
| `caskade --elevated <steps> …` | Windows administrator steps, started with `runas`. |
| `caskade --restore <backup>` | Stages a restore for the next start. |
| `caskade <magnet or .torrent>` / `caskade://…` | Turned into an install request (deep links, OS handlers). |

### 4.2 Webview and core

- **Commands**: each command's logic is a plain function in `caskade_app::commands::<area>`, taking the services it uses (as references) and its arguments, and returning `Result<T, CommandError>`; the types it takes and returns live there too. The adapter's `#[tauri::command]` in `src-tauri/src/commands/<area>.rs` keeps the signature, `State` parameters and doc comment the bindings are generated from, and forwards in one line; only what needs Tauri stays there (dialogs, windows, `AppHandle` state that may not be up yet, adapter-only events). Commands are registered in `bindings.rs`. `just bindings` regenerates `apps/desktop/src/api/bindings.ts`, and a test fails when the committed file is stale. 64-bit times and sizes cross as numbers, 64-bit ids as strings.
- **Events**: the app layer's typed events (`caskade_app::events::Event`, including the commands' own, with the bus bridge that coalesces library changes) go out through the shell's `Emit` sink (`Events::send`, or `Events::emit` when a failed delivery must be acted on). The adapter (`events.rs`, `webview_events!`) sends each under a tauri-specta event: a `#[serde(transparent)]` wrapper with the app type's name whose `specta::Type` is the app type's own, so the bindings name the app layer's type and nothing else. They invalidate TanStack Query caches.
- **Each API area** in the webview (`src/api/*.ts`) is an interface with a native implementation and fixtures, so the browser preview (`just web`) and tests run without the core.
- **The native UI's data** (#89, `crates/caskade-ui-data`, hooks in `apps/native/src/data`): views call `caskade_ui_data::Library`, which goes through the app layer's commands with the core's `ErrorReport`s (codes, hints, references; the hint labels are the webview's, checked against `reply.ts`); library pages hold at most 100 summaries. No IPC, JSON or query cache: `use_query(key, load)` is a Dioxus resource whose load runs on the app's runtime and runs again only when an app event makes its key stale (`Changed::from_event`: a game's update stales that game, the pages and the totals; a session stales its game and the sessions; another account, or missed events, everything). Fixtures mode (`CASKADE_FIXTURES=1` in debug builds, `CASKADE_FIXTURE_COUNT`) runs the real core and app layer over an in-memory library seeded with the webview's `fixtures.ts` games, so both UIs can be compared on the same data; a test keeps the two in step. The root wraps the views in `data::DataRoot`, which provides the source once the services have started (or the fixture library has opened); in debug builds `CASKADE_DATA_PROBE=1` adds a game, favourites it and logs what a subscribed view shows, for a check of the running app under Xvfb.
- **The native UI's input** (#92, `crates/caskade-ui-input`): the webview's input rules (`apps/desktop/src/input`) ported rule for rule over a `FocusTree` (elements, attributes and layout boxes), so a pad moves the same way in both UIs: `data-focus-section` memory, the topmost dialog or popover as the scope, settings rows as one stop, the same key map (off in desktop mode until a pad is used), innermost-wins command bindings, and the on-screen keyboard's layouts (English, AZERTY, QWERTZ, Swedish). `Dispatcher` takes pad actions and keys and acts through a `Ui` trait; the webview's `input.test.tsx` and `keyboard.test.tsx` cases run as the crate's tests over a hand-built tree with the same boxes. In the native app (`apps/native/src/input`), `InputRoot` (`data-input-root`, around the views; an internal hook, not a theming surface) holds focus when no control does, so keys reach its keyboard map; pads are read by `input::start_pads` once the services are up, through the app layer's `commands::input::sink` (actions only while the window has focus, `guide` always, raising the window), and arrive as `InputActionEvent`s on the shell's events. Each action is handled on Dioxus's executor in two steps: the bindings for it are found with Blitz's document borrowed and run with nothing borrowed (they're app code, registered with `use_input_action`); the default then runs over the document, and what it asks of app code (click and focus listeners, going back, the on-screen keyboard) runs last. An overlay that opens takes focus on its `data-focus-default` control, and gives it back when it closes; views report their location with `use_focus_location` so focus comes back to where a view was left. A printable key the keyboard map leaves alone, typed outside a field, a menu or a dialog, starts the shell's search (`keys::starts_typing`, the webview's `startsTyping`; `Input::on_type`). The guide quick menu (`input/guide.rs`, the webview's `GuideMenu`) is part of the root: guide over a running game offers going back to it (the window minimizes), stopping it after a confirm (`commands::guide::guide_stop_game`), or its details (`Input::on_details`, the router's). Blitz gaps worked around there: `:focus-visible` never matches, so the focused element gets `data-focus-visible` when keys or a pad moved focus; a click focuses only fields and clears focus anywhere else, so the root puts focus on the clicked control once Blitz's own click has run; app code can't send a key through the page's listeners, so native controls (a dialog's B, a combobox's arrows) take actions through bindings rather than key presses; a scroll made from code sends no scroll event, so `Input::scrolled` counts the input layer's. A headless test drives a real `DioxusDocument` (a sidebar, a grid, a dialog and a field) with key events and with pad actions on the shell's events. The console e2e harness drives the native binary too (`input/e2e.rs`, debug builds and the `e2e` feature): `CASKADE_E2E_SCRIPT` runs the Tauri harness's script language with a synthetic pad (`InputService::without_pads`), the input root answers its probes (focus, sections, text, real key and pointer events), and `tests/console_e2e.rs` runs it under Xvfb; the webview's full console script runs there once the native views exist (#96).
- **The native UI's components** (#93, `apps/native/src/ui`): `@caskade/ui` and react-aria's behaviour in RSX, on the input layer and `caskade_ui_i18n`. Each renders the webview's anatomy and the state and interaction attributes themes select (`data-hovered`, `data-pressed`, `data-focused`, `data-selected`, `data-disabled`, …; the input layer adds `data-focus-visible`), declares its anatomy entry beside its code (checked against the webview's catalogue), and behaves as the WAI-ARIA APG has it: Blitz gives keys no default actions but Tab, so components take Enter and Space themselves (buttons both, links Enter, switches and checkboxes Space) and stop them there; a pad's A arrives as the input layer's click. Labels name controls through `aria-labelledby`, as AccessKit doesn't name a switch, checkbox, menu item or option from its content. Overlays (menus, a select's list, dialogs) are fixed boxes placed from their trigger's box (below, or above when below doesn't fit) with `data-placement` or a dialog role, so the input layer keeps focus inside them; each puts focus on its start (`data-focus-default`, else the first enabled control) when it mounts and gives it back to its trigger when it goes (Blitz leaves focus on the input root when the focused element goes, which the input layer can't tell from a choice), and closes on Escape, a pad's Back or a click outside. Lists move by arrows, Home, End and typeahead in desktop mode; a pad moves spatially. Blitz has no portals: overlays render where their component is, with `position: fixed`. Tests render each component headless inside the input root and press it with the mouse, the keyboard and a pad, and read it as a screen reader does: `src/a11y_snapshot.rs` writes the AccessKit tree as text, one format for the components' tests and `tests/accessibility.rs`. dioxus-primitives isn't used (Dioxus 0.7, overlays through JavaScript, a focus model of its own).
- **The native library grid** (#94, `apps/native/src/library`): `VirtualLibrary` mounts only the rows on screen (one grid row or six list rows each side) and the focused game's row, and loads only the 100-game pages those rows need, keeping two each side (`library/window.rs`); rows are fixed-height (312 px cards, 44 px list rows under a 32 px header) and columns follow the width as the webview's card layout. LT/RT page through the games, or letter by letter when sorted by title, and the alphabet bar jumps; the library's events reload the pages shown. Covers are the 320 px thumbnails and list icons the 96 px ones, never the original, on `caskade-asset://` from the artwork cache. After its own scrolls and the input layer's it reads the scroller's offset from the document (Blitz sends no scroll event for those). Tests scroll 10,000 fixture games top to bottom in a headless document and bound the mounted cards; `apps/native/scripts/library-scroll.sh` runs the scroll benchmark (release, `bench` feature, fixtures mode) on a private desktop. Layout rules for RSX views: [native-layout.md](native-layout.md).
- **Schemes**: `caskade-asset://` serves cached artwork and thumbnails, `caskade-theme://` compiled theme stylesheets and assets, `caskade-plugin://` plugin UI modules.
- **Permissions**: every command is checked against the active local account's role and permissions before it runs. The rules are `caskade_app::permissions`; the adapter's invoke gate (`permissions.rs`) calls them for each command. The account comes from the app layer, never from the webview.

### 4.3 Hook bus and the launch pipeline

The bus carries events, interceptors and providers between the core, built-ins and plugins. The launch pipeline is its main user:

1. Check the play gate (signed-in account, age limit, already running, one stream at a time).
2. Pick the release and action (a default, or ask), apply the action's target.
3. Offer a game update (a live check only when the background one is overdue, at most 5 s), check saves on paired devices (at most 4 s; a device that reads offline after a failed sync isn't waited for), warn if playing elsewhere.
4. Run before-launch lifecycle scripts and `launch.before` interceptors (which may rewrite or cancel).
5. Spawn, or open the URI, web window or stream. Processes Caskade starts drop its own AppImage environment.
6. Identify the real game process by install folder and executable (and alias path), track it with the chosen tracking mode.
7. `game.started`, after-start scripts in the background.
8. Wait for exit, persist the `PlaySession`, `game.exited`, after-exit scripts, `launch.after` interceptors, save versioning, achievement refresh.

Sessions survive restarts: recovery state records running processes by `ProcessIdentity` (on Linux, start ticks plus `boot_id`), so a reused PID is never touched and clock steps change nothing.

## 5. Data

### 5.1 Domain model

| Entity | Notes |
| --- | --- |
| **Game** | What the user sees: titles, description, genres, tags, dates, platforms, age ratings, favourite, hidden, user overrides (`overrides_json`). |
| **Entry (release)** | One per source: `source_id` + `external_id`, install state and location, size, `LaunchSpec`. Several entries make one game. |
| **Install location** | `Managed { root_id, rel_dir }` for games Caskade installed, never an absolute path, so games and roots can move; `External(path)` for games another launcher owns. |
| **StorageRoot** | A managed install folder with an id also written to `<root>/.caskade/root.json`, so a root at a new path is recognised. |
| **LaunchSpec** | `Uri`, `Executable` (relative to the install for managed games), `Emulator` (platform default, pinned profile or ask, plus ROMs), `Web`, `Stream { host, app }`, `Plugin`. |
| **Action** | A named play or utility action with its own target and tracking mode. |
| **Platform, Emulator, Profile, Scanner** | Definitions are data (TOML); installed emulators, profiles, defaults and scanners are user configuration. |
| **PlaySession** | Start, end, duration, entry, and the device it was played on (`device_id`, this device's endpoint id from `sync.device` when it starts here). Remote playtime from stores is kept apart, per release, provider and account. |
| **Collection** | Manual or smart, owned by a local account. |
| **Artwork** | Kind, source, cached path, thumbnails; user choices override automatic sources. |
| **Account** | Local accounts: role, permissions, age limit, PIN hash (argon2id). |
| **Store account, entitlement** | Launcher and store accounts and what each owns, apart from what is installed. |
| **Device, change log, save version, share** | Paired devices and grants, the sync change log with hybrid logical clocks and tombstones, content-addressed save versions, one-off shares. |

### 5.2 Storage

- **Library**: `library.sqlite` in the data folder, migrated by numbered `sqlx` migrations (`crates/caskade-core/migrations/`, 37 so far, each with a down migration).
- **Transactions** that write start with `BEGIN IMMEDIATE` (`LibraryStore::begin_write`): a deferred one that reads first fails at once with "database is locked" when another connection is writing. Applied migrations never change, comments included; `db/migration_checksums.txt` freezes them and each new migration adds its line.
- **Older builds** open a library a newer one migrated when they can (`db/compat.rs`). From migration 38 on, each migration's first line is `-- older Caskade: compatible` (new tables, columns that may be empty or have a default, looser checks, data fixes) or `-- older Caskade: incompatible` (dropped or renamed tables or columns, a column that must be filled in, a stricter check, a changed meaning); a test fails on a migration without one. After migrating, the library records its newest incompatible migration in `PRAGMA user_version`. A build without some applied migrations opens the library as it is when it has the recorded one, and otherwise fails with `VersionMissing`, reported as a newer library (`unavailable`, hint `updateCaskade`). The startup dialog then offers Update Caskade, which installs the newest release through the updater and restarts, instead of Restore from backup….
- **Caches** (`<cache>/`): artwork and thumbnails, IGDB answers (`igdb/games.sqlite`), Hasheous dumps and indexes, the Ludusavi manifest and indexes, Decomp Buddy plans, the add-on catalogue, compiled plugins, peer downloads. All disposable and rebuilt on demand.
- **Data** (`<data>/`): managed emulators and tools, save blobs, staged restores, elevation records, streaming state, themes, plugins and plugin data.
- **Settings**: namespaced sections (`core.general`, `core.appearance`, `core.sync`, `core.streaming`, …) registered with a schema version, a JSON Schema from `schemars` and validated defaults. The same mechanism renders plugin settings.
- **Secrets** live in the OS keychain (Secret Service, Keychain, Credential Manager): RomM credentials, store account sign-ins (`dev.caskade.store-accounts`), the device key (`dev.caskade.sync`), streaming credentials (`dev.caskade.streaming`). Never in the database, settings, logs, events, backups or the webview.
- **Overrides**: a field the user set is recorded with its automatic value. Automatic writers go through `update_automatic_game` or `upsert_game`, which write beneath the user's values.

### 5.3 Sync and backup classification

- Every table and setting is **synced**, **optional** (off by default) or **local** in `sync::policy`. Paths, folders, emulators, sign-ins and caches are local. Guard tests and the `sync_policy` fixtures fail on anything unclassified.
- Every table and setting is backed up unless `caskade_backup::policy` excludes it (credentials, caches, work in progress). Files outside the database need a backup `Component`.

## 6. Subsystems

### Library and discovery

- `caskade-discovery` reads launcher files, registries, plists, `.desktop` files, Start Menu shortcuts, uninstall entries and Wine prefixes; a classifier keeps out non-games; copies found by several sources merge. Watching uses `notify`, never recursively over large trees.
- `LibraryStore` imports scans, merges games that are one game (`same_games`: same IGDB id, or title within five years), keeps separated games apart (`kept_apart`) and serves queries, counts and FTS5 search.

### Metadata and artwork

- `Igdb` asks caskade-server (closed source, on Railway) by IGDB id and caches every answer, including not-found. Concurrent lookups share one request; failures pause all requests for a minute or `Retry-After` (up to 6 h). Cached answers carry a shape version so new fields are fetched once.
- `Metadata` matches each game (RomM ROM, Hasheous, then the server's `/match` for store ids) and applies fields only where the game's value is empty or was written by the last apply. Runs retry with capped, jittered backoff.
- Fetched metadata and automatic artwork stay on each device, so a device fills in the games sync brings itself. A game from another device has no entries here until it's installed; it's matched by the IGDB id its devices matched it to (`LibraryStore::synced_igdb_id`, source `device`) when nothing here finds one and it has no store ids of its own here, else by the store ids in the identities its devices sent (`LibraryStore::synced_store_ids`); a hand-set match that arrives is applied like one made here. Sync wakes matching when games or hand-set matches arrive (`SyncService::on_games_arrived` → `Metadata::wake`, after a 5-second settle, one run for a burst, not during a failure's backoff); runs go 50 games at a time and only while online lookups are allowed.
- `Hasheous` downloads the dumps the library needs and answers hash and title lookups offline.
- The artwork cache downloads once, makes thumbnails and serves them over `caskade-asset://`. Page-only images download when the page opens.

### Installs, storage and updates

- `caskade-installer` classifies payloads, plans per host, runs installers silently where it can (and interactively where it must), records what it installed, and rolls back. Its failure model (kind, stage, file, security note, recovery actions) drives Activity's recovery UI.
- `InstallService` owns the job queues, maps finished installs into the library and owns `InstallRequests`, the confirmation queue that every deep link, drop, paste and torrent link goes through.
- Storage roots are reconciled by id; moves are transactional and resume.
- `updates` follows managed installs whose source an `UpdateProvider` knows (GitHub releases, without the API). Updates install beside the old folder, carry the user's files over and roll back on failure; interrupted ones are finished or undone at start.

### Emulation and managed tools

- Definitions in `caskade-sources/emulation/` describe detection, install sources per OS, setup steps, profiles, display flags, BIOS and save rules. `platforms.toml` maps platforms to IGDB, Hasheous and RomM ids and lists recommended emulators. Each definition's sources are recorded in `emulation/SOURCES.md`.
- `managed` holds the shared steps: resolve (GitHub without the API, Dolphin's endpoint, a URL), download, unpack and swap carrying the user's files, Flatpak. `EmulatorSetup` and `ManagedTools` build on it. Removal deletes only what `.caskade/shipped.json` lists.
- ROM scanners identify by hash; RomM is both a library source and a `BiosProvider`.
- RomM server definitions sync between devices; their passwords and API tokens stay in each device's keychain. A server with no secret saved on this device for its address, or whose server refused the one it has (401/403), is `sign_in_needed` in `ServerStatus`: the panel offers Sign in (or Enter API token) instead of Sync now, and `romm::watch` doesn't sync it until a new secret is saved. Sync now still tries.

### Launching extras

- **Launch actions** (`game_actions`, `entry_action_settings`) and **lifecycle scripts** (`lifecycle_scripts`, run by `ScriptRunner` with bounded output, a time limit, cancellation and process-group kill) plug into the pipeline in §4.3.
- **Proton builds**: `HostCaps::detect` lists builds in Steam's `compatibilitytools.d` folders (native, Flatpak, system) and `steamapps/common/Proton*`, ranked by family (UMU-Proton, GE-Proton, numbered Steam Proton, Experimental, others, betas, Hotfix), then by version compared as numbers. At each launch `InstallerRunnerResolver` sets `PROTONPATH` to the best one the installer's compatibility runner setting allows: `UMU-Proton` when an umu build leads (so umu keeps it current), never a Hotfix, and `UMU-Proton` (umu downloads it) when none is installed. An action's `proton` (and the built-in action's `builtIn.proton`) names a build by its folder; `launch::actions::apply` puts it in the entry's runner hint, and the resolver uses it when this device has it, otherwise the automatic choice. Managed entries never store a build's path.
- **Web games** play in the game window. Offline bundles (Linux) are captured through the window's MITM proxy and saved with `WebGames::save_offline`.
- **Ports** come from Decomp Buddy's `plans.json` (fetched daily with an ETag) and install with `InstallService::install_attached`.

### Accounts, store accounts and achievements

- `AccountService`: roles, permissions, age limits, PIN hashes with a throttle that survives restarts.
- `StoreAccounts`: connect flows (browser callback with a `state` check, device poll, pasted code, key), token renewal, complete-snapshot imports (partial pages never mark products lost), reconciliation of owned and installed copies, remote playtime counted once per store-identity family.
- `Achievements`: per-account caches, single-flight fetches, last good answer kept, unlocks announced only after a baseline. Providers: Steam, GOG (via the game's Galaxy client), Epic (GraphQL per sandbox), RetroAchievements (by verified hash).

### Devices, sync, saves and shares

- **Transport**: iroh endpoints; devices found over mDNS (service `caskade`), n0 lookup and relays unless `core.sync.neverRelay`. Game files only move over endpoints with no relay transport (`DirectDialer`, `SwarmDialer`, `FileServer`).
- **Protocol**: `caskade/sync/1`, one byte-identical refusal for anything unauthorised, line-framed JSON with read bounds.
- **Pairing**: code pairing with a nonce commitment, or single-use 10-minute `caskade://pair` links accepted by an admin. Grants never reach the webview.
- **Library sync**: changes captured by diffing snapshots against `sync_fields` (only after the library was written), last writer wins per field by hybrid logical clock, tombstones for deletions, games matched across devices by identity (`sync_game_ids`). A session travels as one record plus its device as a separate `device` field, so it keeps the device that played it when forwarded, and Home's game list names that device from `paired_devices` when it isn't this one.
- **Saves**: `Saves` resolves locations per release (Ludusavi manifest by Steam id, GOG id, title, aliases; emulator `[saves]` rules; the user's choices), maps Windows paths into a Wine or Proton prefix, and versions files content-addressed in `<data>/saves/blobs/`.
- **Presence, copies, nearby downloads, backups on devices, emulator offers and config** ride on the same exchange as notes on the blob rounds.
- **Copies** (`Transfers`): the sender offers an installed release's folder as an iroh-blobs collection, refusing a launcher's install and a web game with no offline copy. The receiver installs the folder as it is (`prepare_portable`: a copy is an installed game, never planned by its files), places an emulated release's ROM files as a game of its platform (`transfer::rom`), and makes a web game's offline bundle that game's offline copy (`transfer::web`, shared with shares).
- **Shares**: `caskade://receive` links with their own grant and expiry, admitted only while the share runs, revoked mid-transfer when stopped. Recipients fetch from the swarm of the sharer and other recipients.

### Streaming

- Sunshine (host) and Moonlight (client) are separate GPLv3 programs installed through managed tools, configured by Caskade and never shown.
- **Host**: config in `<data>/streaming/host/`, random web credentials in the keychain, a child process under `setpriv --pdeathsig` (Linux) or an on-demand service (Windows), crash restarts with backoff, the local API pinned to its certificate by bytes. `Publisher` keeps one app per playable game, running `caskade --launch <id>` with Caskade's own program (`caskade_core::program`: the AppImage file from one, not its temporary mount). A user's own Sunshine is detected and used instead of installing one.
- **Client**: Moonlight's own state directory with quiet settings; Caskade pairs over the GameStream protocol itself (no Moonlight window), finds hosts over mDNS `_nvstream._tcp`, wakes them with Wake-on-LAN, and starts streams fullscreen centred on Caskade's window.
- **Elevation**: `Elevation::apply` asks once (pkexec on Linux, `runas` on Windows) for the missing changes in a named plan, records what appeared, and `undo` removes only those. ViGEmBus is installed on Windows hosts in the same prompt.

### Plugins and themes

- **Plugins**: a manifest (`plugin.json`, JSON Schema in `docs/plugins/`), an optional wasm backend against `wit/caskade-plugin.wit` with preopened dirs, allowlisted hosts and per-call memory and time limits, and an optional UI half loaded with `import()` from `caskade-plugin://` behind an import map that shares the app's React, SDK and UI. Every contribution has an error boundary; repeated backend failures disable the plugin. Backends are Rust-only for API 0.1.
- **Add-on catalogue**: signed with minisign (the updater's key), cached offline, compatibility worked out per device; packages are size- and SHA-256-checked, unpacked safely and swapped in with a journal that rolls back.
- **Themes**: `theme.json`, DTCG `tokens.json`, `theme.css`, `assets/`, no JavaScript. `caskade-theme` validates them against the token manifest and anatomy catalogue and compiles them; switching swaps one stylesheet with no reload. Cascade layers: `cs.reset, cs.tokens, cs.base, cs.components, cs.plugins, cs.theme, cs.user`.
- **Themes in the native app** (#90, `apps/native/src/theme.rs`): `assets/app.css` is the webview's Tailwind entry (`apps/desktop/src/styles/index.css`: layers, `@caskade/tokens`, base, Tailwind 4) compiled over `apps/native/src` instead of `packages/ui` by `scripts/build-css.mjs` (`just native-css`; CI fails while it's stale). The theme's compiled stylesheet comes from `ThemeService` as it reaches the webview, and the user's CSS, checked as the webview checks it, goes in `cs.user`: each is one `<style>` in `<head>` whose text Blitz parses as CSS, not HTML. The document's base URL is `caskade-theme://localhost/`, so a theme's `/<id>/<asset>` URLs (fonts, images) resolve on the theme scheme, which the app's net provider serves from `ThemeService::asset`; `data:` URLs are Blitz's, game artwork's thumbnails come from the artwork cache on `caskade-asset://` (`library::fetch_art`, as the Tauri app's scheme), and nothing else is fetched. `ThemeService` starts in `caskade_app::start::open`, so built-in, installed and dev themes, the user's CSS and Appearance (`caskade-ui-theme`) are in the document before the first frame; a plugin's theme applies once the plugin host has started. Changes to `core.appearance`, dev theme reloads and the OS's light or dark scheme restyle the running window. Inter, Oxanium and JetBrains Mono are bundled (latin) and registered under the tokens' family names (`Inter Variable`, …), so a theme naming another font still wins. The OS's reduced motion (GTK's animations switch, Windows' Animation effects, macOS's Reduce motion) goes to Blitz's viewport, so `@media (prefers-reduced-motion)` in the tokens and in themes applies as in the webview. Screenshots (`--shots`, the `shots` feature) render every theme on the reference screens with vello_cpu through this same code; `tests/shots.rs` compares them with committed images, and the same pages exported as HTML are compared with WebKitGTK's rendering (`scripts/webkit-shots.py`, `compare-shots.py`; the differences are in [theming/blitz.md](theming/blitz.md)). The app's stylesheet leaves out the screenshots' reference screens (`src/shots`), which get their own (`src/shots/shots.css`).

### Input and console mode

- `caskade-input` normalises pads to actions (`nav.*`, `confirm`, `back`, `tab.*`, `guide`, …), emitted to the webview only while a Caskade window has focus, except `guide`. On Windows a thread reads XInput's guide button.
- The webview's spatial focus engine (`input/spatial.ts`) moves focus within `data-focus-section` areas; views declare a default stop and bind buttons with `useInputAction`.
- The native UI runs the same rules (`caskade-ui-input`) over Blitz's layout boxes; views bind buttons with `use_input_action` (see *The native UI's input* above).
- `main_window::raise` handles window managers that ignore plain focus requests (Muffin, Mutter) on Linux.

### App services

- **Background work**: `caskade-jobs` is one FIFO resource queue for CPU and disk stages across services. Scans use at most two workers; the 7-Zip sidecar two decoder threads.
- **Notifications**: a feed in the library database with versioned read state; new games batch hourly.
- **Backups**: `VACUUM INTO` snapshots plus optional components in a zip with a manifest of SHA-256s, written as `.partial`, verified, then renamed. Restores are staged and applied before the library opens, keeping the old profile until the restored one opens.
- **App updates**: `tauri-plugin-updater` against `latest.json` on the newest GitHub release, minisign-verified. Debs install through `pkexec apt-get`.
- **Webview recovery**: a page whose process exits, or hangs for 20 seconds, reloads; failures that keep coming restart the app, with caps counted across processes. WebView2 reports its process failures on Windows, WebKitGTK its web process exiting or going unresponsive on Linux.
- **Tray**: AppIndicator loaded at run time on Linux; hides before exit and keeps answering the bus for 500 ms to spare Cinnamon's watcher.
- **Startup** (`login_item`, #114): `core.general.launchOnLogin` registers a login item that starts Caskade with `--login`: an XDG autostart entry (`$XDG_CONFIG_HOME/autostart/caskade.desktop`) on Linux, naming Caskade's own program (`caskade_core::program`, shared with the magnet and `.torrent` handler: the AppImage only when the executable runs inside `$APPDIR`, since a program started from another AppImage inherits its `APPIMAGE`), the Background portal (`RequestBackground` through `gdbus`) inside a Flatpak sandbox (`/.flatpak-info`), the current user's `Run` value `Caskade` through `reg.exe` on Windows, and a launch agent (`~/Library/LaunchAgents/dev.caskade.app.login.plist`, `open -a` on the bundle) on macOS. `login_item::follow` registers or removes it when the setting changes and, while it's on, rewrites at start an item naming another path; with it off, a start only removes a leftover file and never runs `reg.exe` or the portal. A `--login` start with `startMinimized` on leaves the main window hidden when the tray icon is up (`window_state::starts_in_tray`); the tray, a second start and the Dock show it. `startView` is read by the webview before its first render (`shell/startView.ts`) and applies only to a load on `/` outside console mode; `last` is the app page remembered in `localStorage` (`caskade.lastView`) on every navigation.
- **System statistics**: sampled once a second on a blocking thread, only while a watcher holds a lease.
### Localization

- **gettext throughout.** The English text is the message id, with a `msgctxt` only where the same English means different things. Domains: `core` (the Rust side: core, adapter, tray), `ui` (`@caskade/ui`, shared with plugins and Storybook) and `app` (the rest of the webview). Templates are committed as `locales/<domain>.pot`, translations as `locales/<lang>/<domain>.po`. Adding a language's PO files ships it.
- **Rust**: `crates/caskade-i18n` is a pure-Rust gettext runtime (no libintl, so cross builds and bundles need no C gettext). `caskade_core::i18n` provides `tr!`, `trn!`, `trp!` and `trnp!` with named placeholders, and `msg!`-style macros for text stored to render later (notifications keep messages, not rendered text). Catalogs are compiled into the binary. The core owns the language: `set_locale` takes a tag or `system` and falls back through less specific tags to English.
- **Webview**: Lingui 6 with PO catalogs, macros transformed natively, catalogs compiled per language into lazy chunks at build time. `main.tsx` takes the language from the core and follows its `LocaleChanged` event. The pseudo-locale `en-XA` (longer, accented) exists in dev, tests and debug builds to find unmarked or overflowing text.
- **Formatting**: numbers, sizes, durations and dates go through `api/presentation.ts`, never bare `toLocaleString()`; in the native UI through `caskade_ui_i18n::format` over `caskade_i18n::intl`, whose tables (US and UK English, Swedish; others fall back to US English until their row is added) match `Intl`'s output, checked by `intl_oracle.rs`, which `crates/caskade-i18n/scripts/intl-oracle.mjs` generates from Node's `Intl`. Titles sort with an ICU collation per language, search folds case and accents, and the on-screen keyboard has a layout per language.
- **Native UI** (#91): it shares the webview's `app` and `ui` domains (the same English, contexts and translations), so the PO files carry over when the webview goes. `crates/caskade-ui-i18n` bundles `locales/<lang>/app.po` and `ui.po` (only this leaf crate embeds them, so the core doesn't rebuild when the webview's text changes) and provides `t!`, `tn!`, `tp!` and `tnp!` (with `note = "…"` for translators), which look up `app` then `ui` and render Lingui's syntax (`caskade_i18n::message`: `{name}`, `#`, nested `plural`/`select`, ICU quoting); the native app's `trans!` turns numbered tags (`<0>…</0>`, `caskade_i18n::rich`) into the elements it's given, as Lingui's `<Trans>` does. The core's `tr!` family stays in `core` (errors, the tray, dialogs). Every lookup reads the native app's `i18n::LANGUAGE` signal, so a component that showed text renders again when `i18n::use_language` (at the root) sees the core's language change: no restart. Debug builds take `en-XA` from the Language setting or `CASKADE_LANG` and pseudo-localize each text piece exactly as Lingui does (`caskade_i18n::pseudo`). Extraction: `just i18n` lists the native UI's messages in `locales/native.json`, and `packages/i18n`'s extraction adds them to Lingui's catalogs before writing `app.pot`/`ui.pot` (a message the webview has gains the `.rs` reference; others go to `ui` if it has them, else `app`), so the webview's entries keep their order and Lingui stays the only writer of those templates until BLITZ-33. Tests: every Swedish translation of a message the native UI uses resolves through its macros, and no `rsx!` text node or shown attribute (`aria-label`, `title`, `placeholder`, `alt`, …) is a literal with words (`extract::untranslated_rsx`, with an allowlist for the app's name).
- **Plugins and themes** ship their own `locales/<lang>.po` in a domain named after their id.
- **Tooling**: `just i18n` extracts and merges (needs GNU gettext); `just i18n-check` (in CI) fails on a stale template, a PO file `msgfmt --check` rejects, or a translation that drops or changes a placeholder or tag. The translator's guide is [translating.md](translating.md).
- **Not yet translated**: native dialogs the Rust side opens itself, the macOS app menu, and bundle metadata (`.desktop` entries, installer languages).

## 7. Errors and recovery

### 7.1 The error model

`caskade_core::error::ErrorReport` is what crosses to the webview when a command fails: a stable `code`, a one-sentence `summary`, a redacted `detail`, `hints` (most useful first), `transient` and an eight-hex-character `reference` logged with the detail. `ErrorReport::from_error` walks the cause chain and picks the code; text Caskade wrote becomes the summary and an OS or database message the detail. Redaction (`error::sanitize`) strips URL credentials, queries and fragments, tokens, passwords, secrets and bearer values from every summary and detail.

Codes: `invalid`, `forbidden`, `conflict`, `cancelled`, `refused`, `missingFile`, `accessDenied`, `diskFull`, `storageOffline`, `busy`, `overloaded`, `offline`, `server`, `auth`, `integrity`, `corrupt`, `database`, `unavailable`, `internal`, `other`.

`busy` is a file in use, or the database locked by a writer past the busy timeout (SQLite's "database is locked"). `overloaded` is Caskade's own work keeping all four of the library's connections busy past sqlx's 30-second wait (a pool timeout): nothing outside Caskade holds it. Both are transient. The library logs each statement that runs a second or more with its SQL (`sqlx::query`, "slow statement") and each wait of two seconds or more for a connection (`sqlx::pool::acquire`), which name what held the connections.

Hints: `retry`, `retryLater`, `checkConnection`, `reconnectStorage`, `freeSpace`, `checkPermissions`, `checkCredentials`, `signIn`, `reload`, `restart`, `updateCaskade`.

A test fails when a code or hint is missing from this list or lacks a label in `apps/desktop/src/api/reply.ts`. In the adapter, commands return `Result<T, CommandError>` and map with `.map_err(report)`; in the webview, `unwrap` rejects with a `CommandFailure` and `FailureNotice` / `ErrorNotice` show it. Installs and updates keep the installer's richer `Failure`; an input the installer has no way to install is the `unsupported` kind, its `Reason` kept as the detail and said in words with Locate the installer… as the way on.

### 7.2 Recovery policy

| Policy | When | Examples |
| --- | --- | --- |
| Automatic | Safe, bounded, inside what the user started | Retry transient failures with backoff and `Retry-After`, resume verified downloads, rebuild caches, recreate app-owned folders, skip one unreadable record, fall back to the default theme |
| Confirm | Crosses an authorization or has an uncertain effect | Locate a moved source, replace user-modified files, reset a damaged settings section (keeping a backup), delete user data |
| Manual | Only the user can fix it | Credentials, reconnecting a drive, freeing space, a quarantine, data from a newer Caskade |

Recovery code is bounded (fixed attempts or capped, jittered backoff, one layer owns the retry), respects cancellation, reconciles before retrying, never reports success before verifying, keeps the original error, and never bypasses integrity checks, consent or permissions.

## 8. Security

- **Backend plugins are sandboxed** to what the manifest declares and the user granted. A first install shows the permission review; an update that asks for more waits for consent.
- **UI plugins are trusted code** in the app's origin, contained by the CSP and an app-level IPC surface with no raw filesystem or shell commands. A malicious UI plugin can call any app-level command; signing and review through the catalogue are the current answer.
- **Secrets** stay in the keychain (§5.2), and no command returns a token.
- **Devices** authenticate by ed25519 keys; unauthorised peers get one identical refusal; game files never cross a relay.
- **Downloads** are checked by SHA-256 or signature wherever the source offers one; the app updater and the catalogue are minisign-signed.
- **Elevation** runs only fixed, named steps behind one OS prompt, with an undo.
- **Local accounts** are a household lock, not data protection.
- Vulnerability reporting is in [SECURITY.md](../SECURITY.md).

## 9. Performance and resources

- Big lists are virtualised; artwork is thumbnailed and served from cache, never decoded at full size in the grid.
- On Linux, glibc's malloc arenas are capped at 2 and free memory is trimmed every minute (`caskade-malloc`). A `MALLOC_ARENA_MAX` in the environment wins.
- **Budget**: with 8,400 games the main process holds under 400 MB of anonymous memory and swap after an hour idle (check L8 in [manual-checks.md](manual-checks.md)). `scripts/idle-bench/` measures every process the app runs (with the webview, its WebKit processes too) against a seeded 8,400-game library in a sandbox, so builds can be compared; results are kept in the Native UI milestone's benchmark issue. Work repeated on a timer reuses what it worked out while its inputs are unchanged, as discovery's watch plan does for Wine prefixes.
- Library queries never compare every row of one table with every row of another per game: Ready to Install's peer copies are matched to the copies other devices announced by `(device, entry)` against the announced set read once per statement (`db::sync_copies::copyable_here_sql`). The quadratic `LIKE` join it replaced held connections for seconds per count on synced libraries and starved the pool (#77).
- Background work reacts to writes (`LibraryStore::writes`) instead of rereading whole tables on a timer. Large files (the Ludusavi manifest) are parsed incrementally. Threads start only for features in use: copy stores open on first use, the plugin epoch thread starts with the first plugin.
- On NVIDIA's proprietary driver under X11, WebKitGTK paints on the CPU; the app restarts itself with the DMABUF renderer over shared memory (`caskade_player::webkit`; the native app does this only for its game and video windows, as Blitz's main window doesn't use WebKit), unless a `WEBKIT_*` renderer variable or `LIBGL_ALWAYS_SOFTWARE` (a software-rendering session) is set. If the main window's page then crashes or hangs, the driver and WebKit library are recorded in the cache (`webkit-gpu-failed`) and the app restarts on WebKit's own renderer; it tries the GPU again once either changes. Where WebKit still paints on the CPU, `<html data-paint="cpu">` pauses ambient animation.

## 10. Build, test and release

- **Commands**: `just dev` (app with hot reload), `just web` (UI only), `just test-app`, `just bindings`, `just build-app`, `just i18n`, `pnpm typecheck`, `pnpm lint`, `pnpm test`. Cargo commands share `/tmp/caskade-heavy.lock` through `flock`.
- **Build speed**: features that little else depends on live in their own crates over `caskade-core` (achievements, add-ons, backup, share, streaming, system statistics), so an edit there rebuilds that crate and the adapter but not the core. rustls runs on ring only: aws-lc's C build is kept out by two vendored patches, and a test fails if it returns to `Cargo.lock`. The desktop crate emits only an `rlib`; dev builds use `debug = "line-tables-only"` and no dependency debug info; each crate's integration tests compile into one `it` binary (`tests/main.rs`).
- **Tests** never reach the internet: wiremock for services, recorded fixtures, fake launchers and processes, several cores in one process for sync, a TLS-fronted fake for the streaming host. Live tests against real services and programs are `#[ignore]`d. The console end-to-end test drives the built app with a scripted pad under Xvfb.
- **Tests that start the app** never reach the developer's desktop (#122): Rust tests (the native smoke and e2e, the Tauri e2e) start it on `caskade_test_desktop::PrivateDesktop`, an Xvfb display (`-displayfd`) and a `dbus-daemon` without service folders of the test's own, with a cleared environment; scripts (screenshots, measurements, the prototype's) run under `scripts/private-desktop.sh`. Both guards refuse a display or bus a login session uses (`loginctl`, `/run/user/<uid>/bus`, the user manager's environment). The app puts no tray icon up in probe and measurement runs (`CASKADE_DATA_PROBE`, `CASKADE_NATIVE_SMOKE_MOTION`, `CASKADE_NATIVE_NO_TRAY=1`), and `--shots` starts none of the shell. `scripts/tray-race/` quits each app through its tray menu the moment its icon appears, 20 times, against Cinnamon's real `xapp-sn-watcher` on a private bus (the watcher must survive with no `NoReply`), and `decoy.sh` runs any command against a stand-in session watched by `scripts/desktop-watch.sh`, which only observes a session: its registered tray icons, root windows and `Notify` calls.
- **CI** (`.github/workflows/ci.yml`): pull requests run only what their changes reach (`scripts/ci-scope.py`); master runs everything. Jobs: frontend, Rust (Linux), Windows cross-Clippy, Windows launch channel and elevation (native), console end-to-end, installer and discovery on Linux, Windows and macOS. `across-networks.yml`, run by hand, tests a share between a runner and a PC on another network.
- **Release** (`.github/workflows/release.yml`, [RELEASING.md](RELEASING.md)): a version tag builds signed bundles for every platform into a draft GitHub release with the updater feed.
- **Website** (`.github/workflows/pages.yml`): `node apps/website/build.mjs` renders English and Swedish HTML to `apps/website/dist`, validates translation parity and copies local assets. `python3 apps/website/check.py` checks the generated pages and links offline. Pull requests build and check; site, logo, token CSS or workflow changes on `master` deploy through GitHub Actions to project Pages. The site has no first-party runtime JavaScript; a native popover provides the flag-and-name language dropdown. Download buttons link directly to the latest stable release's AppImage, Debian package, Windows setup and Apple silicon / Intel disk images, selected from GitHub release metadata with missing, ambiguous and unexpected asset URLs rejected. The Pages workflow fetches current metadata before building and redeploys from `master` when a release is published; local builds use a committed metadata snapshot and stay offline. A responsive, lazy-loaded YouTube privacy-enhanced iframe below the introduction loads third-party content for the video, without autoplay, with translated labels and a direct YouTube link; other assets remain local. Relative asset and language URLs support the `/caskade/` project path. Website copy uses its own JSON catalogues, separate from the app's gettext domains. Localised desktop and console screenshots are captured from the real browser-preview components with a sample library by the manual `apps/website/capture.mjs` script, which downloads public cover artwork and isolates the fixture API; it never reads a native user profile. CSS crops link to the complete captures, and normal builds and CI use the committed images offline.
