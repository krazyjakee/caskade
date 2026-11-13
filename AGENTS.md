# AGENTS.md

Guidance for coding agents in this repository. `CLAUDE.md` imports this file; edit only this one.

Caskade is a cross-platform game library and launcher in Rust with a native Dioxus/Blitz UI by default. The legacy Tauri 2/React app is retained temporarily for comparison (`just legacy`, or `caskade --legacy` from an installed bundle). Read these before changing anything substantial:

- [docs/MISSION.md](docs/MISSION.md): what Caskade is for.
- [docs/PRODUCT-SPEC.md](docs/PRODUCT-SPEC.md): what it does and what is planned.
- [docs/TECH-SPEC.md](docs/TECH-SPEC.md): architecture, data, subsystems, errors, security, build and test.
- [docs/DESIGN-GUIDELINES.md](docs/DESIGN-GUIDELINES.md): look, input, components and wording.

Keep the specs true: a change that alters what Caskade does or how a subsystem works updates the product or tech spec in the same commit.

## Git workflow

- Commit all work to `master` in the main checkout. Don't create branches or git worktrees: several agents work in this checkout at once, and work kept off `master` drifts.
- Commit only the files you changed: `git add <paths>`, never `git add -A`, `git add .` or `git commit -a`. Other sessions' uncommitted changes share the working tree; leave them alone. Never `git stash`.
- `origin` is https://github.com/krazyjakee/caskade; issues, milestones and pull requests live there (`gh issue …`). Push with `git push origin master`; never force-push. Old commit messages cite archived Gitea issue numbers (the read-only `gitea` remote), not GitHub's.
- Format only the files you touched (`rustfmt <file>`, `biome format <file>`), never the whole tree.

## Commands

Cargo commands share one heavy lane: run them under `flock /tmp/caskade-heavy.lock …`. `just` recipes take the lock themselves; never wrap a `just` recipe in `flock`.

| Task                                             | Command                                                                                                                                                                                                                        |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Install                                          | `pnpm install`                                                                                                                                                                                                                 |
| Native app                                       | `just dev`; legacy with hot reload: `just legacy` (Vite on :1420, one at a time)                                                                                                                                               |
| UI in a browser, no core                         | `just web` (`VITE_CASKADE_FIXTURES=1` for fixtures)                                                                                                                                                                            |
| Build                                            | `flock /tmp/caskade-heavy.lock cargo build` (native is the default workspace member)                                                                                                                                           |
| Lint                                             | `flock /tmp/caskade-heavy.lock cargo clippy --workspace --all-targets --features caskade-desktop/dev-tools -- -D warnings`; Windows: add `--target x86_64-pc-windows-gnu -p caskade-core -p caskade-desktop -p caskade-native` |
| Test the app                                     | `just test-app` (core and adapter), `pnpm test` (Vitest), `just test-native` and `just test-native-window` (the native UI, [docs/native-testing.md](docs/native-testing.md))                                                   |
| One integration test file                        | `cargo test -p <crate> --test it <file>::` (each crate's `tests/*.rs` compile into one `it` binary; a new file needs `mod <file>;` in `tests/main.rs`)                                                                         |
| Installer, discovery, plugin host, theme         | `just test-installer`, `just test-discovery`, `cargo test -p caskade-plugin-host` (needs `rustup target add wasm32-wasip2`), `cargo test -p caskade-theme`                                                                     |
| TypeScript                                       | `pnpm typecheck` (under RTK: `rtk proxy pnpm typecheck`), `pnpm lint` (Biome)                                                                                                                                                  |
| Bindings                                         | `just bindings` after adding or changing a command; a test fails on stale bindings                                                                                                                                             |
| Translations                                     | `just i18n`, then commit the `.pot` and `.po` changes; `just i18n-check` gates CI                                                                                                                                              |
| Tokens and anatomy docs                          | `pnpm --filter @caskade/tokens build`, `pnpm --filter @caskade/ui docs`, then `CASKADE_WRITE_ANATOMY=1 cargo test -p caskade-native anatomy` (generated; never edit `docs/theming/tokens.md` or `anatomy.md` by hand)          |
| Plugin schema                                    | `CASKADE_WRITE_SCHEMA=1 cargo test -p caskade-plugin-host`                                                                                                                                                                     |
| App bundle                                       | `just build-app` (native by default, legacy included)                                                                                                                                                                          |
| Anything that starts the app outside a Rust test | `scripts/private-desktop.sh -- <command>` (its own Xvfb and D-Bus); `scripts/tray-race/run.sh <binary>` (tray quit race, #122); `scripts/tray-race/decoy.sh -- <command>` (checks a command leaves a desktop alone)            |

## Rules that tests or reviews enforce

- `caskade-core` and `caskade-app` (the application layer every app shell runs) have no Tauri dependency. Command logic goes in `crates/caskade-app/src/commands/<area>.rs` as plain async fns; the Tauri command in `apps/desktop/src-tauri/src/commands/<area>.rs` forwards in one line, keeping its doc comment and params (they generate the bindings), and is registered in `bindings.rs`. Doc comments on types that cross to the webview reach `bindings.ts` too: move them word for word. An app event reaches the webview through the adapter's Type-transparent wrapper in `events.rs` (`webview_events!`). 64-bit ids cross to the webview as strings.
- The webview never touches the OS: no `fs` or `shell` plugin, everything through commands. The active account comes from the adapter, never from the webview.
- Secrets (device key, store sign-ins, RomM and streaming credentials) live only in the OS keychain: never in the database, settings, logs, events, backups or the webview.
- A new migration starts with `-- older Caskade: compatible` or `-- older Caskade: incompatible` (see `db/compat.rs`); a test fails otherwise.
- A new table or setting needs its `sync::policy` classification (guard tests fail otherwise) and is backed up unless `caskade_backup::policy` excludes it.
- Game files move only over relay-free endpoints, never through a relay.
- Never `cargo update` iroh, iroh-base, iroh-relay, iroh-dns or noq: `Cargo.lock` pins them together.
- Automatic writers never overwrite a field the user set: go through `update_automatic_game` / `upsert_game`.
- Every error code and recovery hint is listed in `docs/TECH-SPEC.md` §7 and labelled in `apps/desktop/src/api/reply.ts` (a test checks both).
- UI: style through `--cs-*` token utilities only, never raw colours; give components `data-cs`/`data-part` attributes and add them to `packages/ui/src/anatomy.ts` (native RSX components also to `apps/native/src/anatomy.rs`, matching it); never select on classes; animations use the motion tokens. Follow the verbs and ellipsis rules in the design guidelines. Don't use Tauri's `data-tauri-drag-region`.
- Every visible string is translatable (Lingui in the webview, `tr!` in Rust).
- Tests never reach the internet (wiremock, fixtures, fakes); live tests are `#[ignore]`d.
- Nothing a test, check or measurement starts may reach the developer's desktop: no synthetic input, windows, tray icons or notifications on its display or its session bus, and never kill processes by name. Rust tests start the app on `caskade_test_desktop::PrivateDesktop` (its own Xvfb and D-Bus); scripts run it under `scripts/private-desktop.sh`, whose `--guard` refuses a login session's display or bus. `xvfb-run` alone isn't enough: the bus is still the desktop's, and a test's tray icon vanishing crashed Cinnamon (#122). Real-display runs are the user's (#120).
- Don't start threads, re-read whole tables on a timer, parse large files whole, or watch large trees recursively: see the tech spec's performance section.
- caskade-server (the IGDB metadata server) is closed source: never link to it.
