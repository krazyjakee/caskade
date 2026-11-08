# Caskade

A game library and launcher for Windows, Linux and macOS, written in Rust with a Tauri 2 shell and a React UI.

Every game you own, on every device in your home, in one library: store and launcher games, emulated games with emulators set up for you, web games, fan ports, and games streamed from another PC in the house. Devices pair and keep the library, saves and playtime in step directly, with no account and no server. It has a desktop mode, a fullscreen console mode for gamepads, themes and WebAssembly plugins.

- [Mission](docs/MISSION.md)
- [Product spec](docs/PRODUCT-SPEC.md): what it does today and what is planned
- [Tech spec](docs/TECH-SPEC.md): how it is built
- [Design guidelines](docs/DESIGN-GUIDELINES.md)

## Status

Version 0.1.x is published as [GitHub releases](https://github.com/krazyjakee/caskade/releases), and installed copies update themselves. Linux is the reference platform. Windows builds and is partly verified on real systems; the app hasn't been run on macOS yet. Details are in the product spec's Status section.

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

The crates, apps and packages are described in the [tech spec](docs/TECH-SPEC.md#3-repository).

The [documentation index](docs/README.md) links to the specs, the plugin and theme contracts, and the guides.
