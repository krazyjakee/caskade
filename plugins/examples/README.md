# Example plugins

These are the dogfood plugins. CI builds and tests them.

Every example targets plugin API 0.2 (#109): its backend draws its UI with Caskade's components, so it works in the webview and the native app, and in console mode.

| Plugin                            | Shows                                                                                                                  |
| --------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| [playtime-stats](playtime-stats/) | A view drawn by its backend: a tile, a chart or a table through a segmented control and its state, a `game.exited` hook, `library-query` sorted by playtime |
| [howlongtobeat](howlongtobeat/)   | A drawn `game.details.tabs` tab with the host-given game title, outbound HTTP to an allowlisted host, plugin KV, settings |
| [gamemode](gamemode/)             | A `launch.before` interceptor with the `process` permission, read-only folder grants, settings; no UI                  |
| [caskade-pack](caskade-pack/)     | `install-handler` and `url-resolver` providers (#55), a `game.actions` entry answered by `act`, translated into Swedish (`locales/sv.po`, #219) |
| [crashy](crashy/)                 | Test-only (#134): every contribution and hook fails, to prove isolation                                                |

Build them into each plugin's `dist/`:

```sh
node plugins/examples/build.mjs gamemode howlongtobeat playtime-stats caskade-pack crashy
```

Backends are built with cargo for `wasm32-wasip2` (`rustup target add wasm32-wasip2`).

To try one, load the folder in place with `caskade dev plugins/examples/<name>` (a development build only: `dev` must be the first argument, and a dev plugin gets its permissions without review), or install it from Settings → Plugins → Install from folder.

Tests:
- `crates/caskade-plugin-host/tests/examples.rs`: each plugin installs from its folder with exactly its declared permissions. GameMode wraps a fixture launch in a fake `gamemoderun`; HowLongToBeat queries a mock server, caches the result and draws its tab; Playtime stats draws the most played games, the table and the last session; Caskade Pack's game action answers in Swedish; Crashy's UI fails alone.
- `apps/native/src/plugins/tests.rs` and `apps/desktop/src/plugins/examples.test.tsx`: drawn views, tabs, game actions, Home sections and placeholders in the native app and the webview.
