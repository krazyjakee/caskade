# List tasks
default:
    @just --list

# Install JS dependencies
install:
    pnpm install

# Build and run the native app
dev *args:
    just native {{args}}

# Run the native app (#87): the main window rendered by Blitz, without Tauri
native *args:
    flock /tmp/caskade-heavy.lock cargo build -p caskade-native
    flock /tmp/caskade-heavy.lock python3 scripts/7zz-sidecar.py --dev
    "$(flock /tmp/caskade-heavy.lock cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys;print(json.load(sys.stdin)["target_directory"])')/debug/caskade-native" {{args}}

# Test the native UI as CI does (docs/native-testing.md): its headless component and view tests, then its screenshots (vello_cpu) against the committed images and Chromium's
test-native *args:
    flock /tmp/caskade-heavy.lock cargo test -p caskade-native {{args}}
    flock /tmp/caskade-heavy.lock cargo test -p caskade-native --features shots --test shots --test text_pixels

# The native app's window tests: the smoke test, a pad end to end and the GPU backdrop, each on an Xvfb and session bus of its own (needs Xvfb, D-Bus and a Vulkan driver such as lavapipe)
test-native-window:
    flock /tmp/caskade-heavy.lock cargo test -p caskade-native --test smoke --test console_e2e --test backdrop -- --ignored

# Rewrite the native screenshots' committed images, after deliberately changing how a reference screen or theme looks
native-shots:
    CASKADE_WRITE_SHOTS=1 flock /tmp/caskade-heavy.lock cargo test -p caskade-native --features shots --test shots

# The native app's stylesheet (#90): Tailwind 4 over apps/native/src, after
# changing its classes or apps/native/styles; CI fails while it's stale
native-css:
    node apps/native/scripts/build-css.mjs

# Build the native app (no app bundle)
build:
    flock /tmp/caskade-heavy.lock cargo build -p caskade-native

# Debug build of the app with Linux bundles (deb, AppImage)
build-app:
    flock /tmp/caskade-heavy.lock cargo build -p caskade-native
    flock /tmp/caskade-heavy.lock python3 scripts/stage-bundle.py --profile debug
    flock /tmp/caskade-heavy.lock pnpm --dir apps/native tauri bundle --debug --no-sign --no-binary-patching

# Build the app's binaries and write Cargo's HTML timing report (no cache cleaning)
build-timings *args:
    flock /tmp/caskade-heavy.lock cargo build -p caskade-native --bins --timings {{args}}

# Scaffold a plugin: just plugin-new my-plugin [--id dev.me.hello] [--no-backend] …
plugin-new *args:
    node packages/create-plugin/index.mjs --sdk {{justfile_directory()}} {{args}}

# Build the starter add-on catalogue into <out> and check it (docs/plugins/publishing.md): just starter-catalogue [target/catalogue]
starter-catalogue out="target/catalogue":
    flock /tmp/caskade-heavy.lock node plugins/examples/build.mjs playtime-stats howlongtobeat gamemode
    flock /tmp/caskade-heavy.lock cargo run -q -p caskade-catalogue -- build scripts/addons/starter.json --out {{out}}
    flock /tmp/caskade-heavy.lock cargo run -q -p caskade-catalogue -- check {{out}}/catalogue.json

# Extract translatable text into locales/<domain>.pot (core, app, ui) and merge it into each locales/<lang>/<domain>.po; `just i18n --add de` starts a language
i18n *args:
    flock /tmp/caskade-heavy.lock cargo run -q -p caskade-i18n --features extract --bin caskade-i18n -- extract {{args}}

# Check the translations as CI does: templates and catalogs current, every PO file passes msgfmt --check, translations keep their placeholders and tags; prints each language's completion (needs GNU gettext)
i18n-check:
    flock /tmp/caskade-heavy.lock cargo run -q -p caskade-i18n --features extract --bin caskade-i18n -- extract --check

# Run the app tests: the core, the application layer and the native UI (Rust), then the JavaScript tooling's
test-app *args:
    flock /tmp/caskade-heavy.lock cargo test -p caskade-core -p caskade-achievements -p caskade-addons -p caskade-backup -p caskade-share -p caskade-streaming -p caskade-system-stats -p caskade-app -p caskade-ui-data -p caskade-ui-input -p caskade-ui-i18n -p caskade-ui-theme -p caskade-player -p caskade-native -p caskade-catalogue {{args}}
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
