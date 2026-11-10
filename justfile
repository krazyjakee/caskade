# List tasks
default:
    @just --list

# Install JS dependencies
install:
    pnpm install

# Run the desktop app (Tauri) with hot reload
dev:
    pnpm tauri dev

# Run only the UI in a browser with hot reload (http://localhost:1420)
web:
    pnpm dev

# Run the native app (#87): the main window rendered by Blitz, without Tauri
native *args:
    flock /tmp/caskade-heavy.lock cargo build -p caskade-native
    "$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys;print(json.load(sys.stdin)["target_directory"])')/debug/caskade-native" {{args}}

# The native app's stylesheet (#90): Tailwind 4 over apps/native/src, after
# changing its classes or the webview's styles; CI fails while it's stale
native-css:
    node apps/native/scripts/build-css.mjs

# Production build of the UI (no app bundle)
build:
    pnpm build

# Debug build of the desktop app with Linux bundles (deb, AppImage)
build-app:
    flock /tmp/caskade-heavy.lock pnpm tauri build --debug

# Build desktop binaries and write Cargo's HTML timing report (no cache cleaning)
build-timings *args:
    flock /tmp/caskade-heavy.lock cargo build -p caskade-desktop --bins --timings {{args}}

# Scaffold a plugin: just plugin-new my-plugin [--id dev.me.hello] [--no-backend] …
plugin-new *args:
    node packages/create-plugin/index.mjs --sdk {{justfile_directory()}} {{args}}

# Build the starter add-on catalogue into <out> and check it (docs/plugins/publishing.md): just starter-catalogue [target/catalogue]
starter-catalogue out="target/catalogue":
    flock /tmp/caskade-heavy.lock node plugins/examples/build.mjs playtime-stats howlongtobeat gamemode
    flock /tmp/caskade-heavy.lock cargo run -q -p caskade-desktop --bin caskade-catalogue -- build scripts/addons/starter.json --out {{out}}
    flock /tmp/caskade-heavy.lock cargo run -q -p caskade-desktop --bin caskade-catalogue -- check {{out}}/catalogue.json

# Regenerate the TypeScript bindings (apps/desktop/src/api/bindings.ts)
bindings:
    flock /tmp/caskade-heavy.lock cargo run -q -p caskade-desktop --bin export-bindings

# Extract translatable text into locales/<domain>.pot, merged into each locales/<lang>/<domain>.po: the Rust side's tr! calls (core), then the webview's (ui, app; needs GNU gettext)
i18n:
    flock /tmp/caskade-heavy.lock cargo run -q -p caskade-i18n --features extract --bin caskade-i18n -- extract
    pnpm i18n:extract

# Check the translations as CI does: templates current, every PO file passes msgfmt --check, translations keep their placeholders and tags; prints each language's completion (needs GNU gettext)
i18n-check:
    flock /tmp/caskade-heavy.lock cargo run -q -p caskade-i18n --features extract --bin caskade-i18n -- extract --check
    pnpm i18n:check

# Run the app tests: core and Tauri adapter (Rust), then the frontend (Vitest)
test-app *args:
    flock /tmp/caskade-heavy.lock cargo test -p caskade-core -p caskade-achievements -p caskade-addons -p caskade-backup -p caskade-share -p caskade-streaming -p caskade-system-stats -p caskade-app -p caskade-ui-data -p caskade-ui-input -p caskade-ui-theme -p caskade-player -p caskade-desktop -p caskade-native {{args}}
    pnpm test

# Type-check every package
typecheck:
    pnpm typecheck

# Lint and format-check (Biome)
lint:
    pnpm lint

# Apply Biome fixes and formatting
fmt:
    pnpm format

# Run the standalone installer library and CLI tests
test-installer *args:
    flock /tmp/caskade-heavy.lock cargo test -p caskade-installer --all-features {{args}}

# Run the caskade-discovery library and CLI tests
test-discovery *args:
    flock /tmp/caskade-heavy.lock cargo test -p caskade-discovery --all-features {{args}}

# Installer fixtures: just fixtures check | windows --prepare | linux-shell | … (xtask/src/lib.rs)
fixtures +args:
    flock /tmp/caskade-heavy.lock cargo build -q -p xtask
    cargo xtask fixtures {{args}}

# Build with the heavy lock, then fuzz under the low-priority single-core lane
fuzz target *args:
    python3 crates/caskade-installer/fuzz/run.py {{target}} {{args}}

# T9 scoreboard: the installer CLI's lifecycle over the fixture corpus, into target/scoreboard
corpus *args:
    flock /tmp/caskade-heavy.lock cargo build -p caskade-installer --all-features --bin caskade-install
    python3 xtask/scoreboard/run.py --binary "${CARGO_TARGET_DIR:-target}/debug/caskade-install" --output target/scoreboard {{args}}

# Explicit live resolver metadata refresh; assets are never downloaded or executed
test-resolvers-live:
    CASKADE_RESOLVERS_LIVE=1 flock /tmp/caskade-heavy.lock cargo test -p caskade-installer --all-features --locked --test it fetch_resolvers_live::refresh_public_github_response -- --ignored --exact
