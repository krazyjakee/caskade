# AGENTS.md

Guidance for coding agents in this repository. `CLAUDE.md` imports this file; edit only this one.

Caskade is a cross-platform game library and launcher in Rust (Tauri 2) with a React UI. Read these before changing anything substantial:

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

| Task | Command |
| --- | --- |
| Install | `pnpm install` |
| App with hot reload | `just dev` (Vite on :1420, one at a time) |
| UI in a browser, no core | `just web` (`VITE_CASKADE_FIXTURES=1` for fixtures) |
| Build | `flock /tmp/caskade-heavy.lock cargo build -p caskade-core -p caskade-desktop` |
| Lint | `flock /tmp/caskade-heavy.lock cargo clippy --workspace --all-targets -- -D warnings`; Windows: add `--target x86_64-pc-windows-gnu -p caskade-core -p caskade-desktop` |
| Test the app | `just test-app` (core and adapter), `pnpm test` (Vitest) |
| One integration test file | `cargo test -p <crate> --test it <file>::` (each crate's `tests/*.rs` compile into one `it` binary; a new file needs `mod <file>;` in `tests/main.rs`) |
| Installer, discovery, plugin host, theme | `just test-installer`, `just test-discovery`, `cargo test -p caskade-plugin-host` (needs `rustup target add wasm32-wasip2`), `cargo test -p caskade-theme` |
| TypeScript | `pnpm typecheck` (under RTK: `rtk proxy pnpm typecheck`), `pnpm lint` (Biome) |
| Bindings | `just bindings` after adding or changing a command; a test fails on stale bindings |
| Translations | `just i18n`, then commit the `.pot` and `.po` changes; `just i18n-check` gates CI |
| Tokens and anatomy docs | `pnpm --filter @caskade/tokens build`, `pnpm --filter @caskade/ui docs` (generated; never edit `docs/theming/tokens.md` or `anatomy.md` by hand) |
| Plugin schema | `CASKADE_WRITE_SCHEMA=1 cargo test -p caskade-plugin-host` |
| App bundle | `just build-app` |

## Rules that tests or reviews enforce

- `caskade-core` has no Tauri dependency. Commands go in `apps/desktop/src-tauri/src/commands/<area>.rs` and are registered in `bindings.rs`. 64-bit ids cross to the webview as strings.
- The webview never touches the OS: no `fs` or `shell` plugin, everything through commands. The active account comes from the adapter, never from the webview.
- Secrets (device key, store sign-ins, RomM and streaming credentials) live only in the OS keychain: never in the database, settings, logs, events, backups or the webview.
- A new table or setting needs its `sync::policy` classification (guard tests fail otherwise) and is backed up unless `backup::policy` excludes it.
- Game files move only over relay-free endpoints, never through a relay.
- Never `cargo update` iroh, iroh-base, iroh-relay, iroh-dns or noq: `Cargo.lock` pins them together.
- Automatic writers never overwrite a field the user set: go through `update_automatic_game` / `upsert_game`.
- Every error code and recovery hint is listed in `docs/TECH-SPEC.md` §7 and labelled in `apps/desktop/src/api/reply.ts` (a test checks both).
- UI: style through `--cs-*` token utilities only, never raw colours; give components `data-cs`/`data-part` attributes and add them to `packages/ui/src/anatomy.ts`; never select on classes; animations use the motion tokens. Follow the verbs and ellipsis rules in the design guidelines. Don't use Tauri's `data-tauri-drag-region`.
- Every visible string is translatable (Lingui in the webview, `tr!` in Rust).
- Tests never reach the internet (wiremock, fixtures, fakes); live tests are `#[ignore]`d.
- Never send synthetic input to the real desktop or kill processes by name: live UI checks run under Xvfb with isolated profiles.
- Don't start threads, re-read whole tables on a timer, parse large files whole, or watch large trees recursively: see the tech spec's performance section.
- caskade-server (the IGDB metadata server) is closed source: never link to it.
