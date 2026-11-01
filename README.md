# Caskade

A game library and launcher for Windows, Linux and macOS. It is written in Rust, with a Tauri 2 shell and a React UI. It has a desktop mode and a fullscreen console mode for gamepads.

## What it does

- **Finds installed games:** Steam, Heroic, Legendary, Lutris, Epic, GOG, EA, Ubisoft, Battle.net, Xbox, itch, Amazon, Riot, Rockstar, Bottles, Faugus, Minigalaxy, Flatpak and Snap. It also finds games through the Start Menu, uninstall entries, `.desktop` files, app bundles and Wine prefixes. Copies found by more than one source are merged.
- **Emulation:** detects installed emulators, scans ROM folders into the library, and launches ROMs through per-emulator profiles. RomM servers work as a library source.
- **Installs:** installs a file, folder or URL into a storage root. That covers Windows installers (run under Proton on Linux), Linux and macOS packages, archives and portable games. A `caskade://install?url=…` link queues an install, and nothing is downloaded until you confirm it.
- **Launches games:** tracks the real game process for playtime, including games a launcher starts in a child process.
- **Themes:** themes are token sets plus CSS over documented component parts. `themes/default` and `themes/paper` are built in, and you can add your own CSS.
- **Plugins:** WebAssembly backends with permissions the user grants, plus optional UI halves.

## Status and limitations

- This is a prototype. Nothing is packaged or published yet.
- CI tests the desktop app on Linux only. Windows gets a cross-compiled Clippy check, and the app has not been checked on macOS. The installer and discovery crates are tested on Linux, Windows and macOS.
- The Store view shows placeholder data.
- Plugin backends are Rust only. Two plugin features are reserved and don't run yet: metadata providers and plugin-supplied emulator definitions. See [docs/plugins](docs/plugins/README.md).

## Build and run

You need:

- Rust 1.94 or newer
- Node with pnpm 9
- `just`
- Python 3, which fetches the pinned 7-Zip sidecar
- On Linux, Tauri's WebKitGTK build dependencies and `libudev-dev`. App bundles also need `libayatana-appindicator3-dev`: the Tauri CLI finds the tray library through pkg-config and bundles it
- For the plugin host tests: `rustup target add wasm32-wasip2`

```sh
pnpm install
just dev        # desktop app with hot reload
just web        # UI only, in a browser, without the Rust core
just test-app   # core and adapter tests, then Vitest
```

`just --list` shows the other recipes: `test-installer`, `test-discovery`, `bindings`, `build-app` and `plugin-new`. The Cargo recipes serialize builds through `flock` (util-linux), so they assume Linux.

## Layout

| Path                                                                                              | Contents                                                                                                                       |
| ------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| `crates/caskade-core`                                                                             | Library store (SQLite), launch pipeline, play sessions, settings, install and storage services, hook bus. No Tauri dependency. |
| `crates/caskade-installer`, `caskade-discovery`                                                   | Standalone libraries (with dev CLIs) for installing payloads and finding installed games                                       |
| `crates/caskade-sources`, `caskade-plugin-host`, `caskade-theme`, `caskade-input`, `caskade-jobs` | Library sources, the wasmtime plugin host, the theme compiler, gamepad input, the background job queue                         |
| `crates/caskade-component-cache`, `caskade-plugin-sdk`, `caskade-emulation-fixtures`              | Compiled-plugin cache, the Rust SDK for plugin backends, generated emulator and ROM test fixtures                              |
| `apps/desktop`                                                                                    | The Tauri adapter (`src-tauri`) and the React app (`src`)                                                                      |
| `apps/storybook`                                                                                  | Storybook for `@caskade/ui`, also a theme playground                                                                           |
| `packages/`                                                                                       | `@caskade/ui` components, `@caskade/tokens` design tokens, the plugin SDK and scaffolder                                       |
| `themes/`, `plugins/examples/`                                                                    | Built-in themes and example plugins                                                                                            |
| `xtask/`                                                                                          | Installer fixture builders, the corpus scoreboard and emulation fixture checks (`cargo xtask`)                                 |

The [documentation index](docs/README.md) links to architecture, plugin and theme contracts, crate guides, and verification checklists.
