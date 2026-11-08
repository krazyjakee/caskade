# Example plugins

These are the dogfood plugins. CI builds and tests them.

| Plugin                            | Halves       | Shows                                                                                                   |
| --------------------------------- | ------------ | ------------------------------------------------------------------------------------------------------- |
| [playtime-stats](playtime-stats/) | UI           | A sidebar view with a playtime chart, a `game.exited` listener, `useGames` and `ctx.library`            |
| [howlongtobeat](howlongtobeat/)   | UI + backend | A `game.details.tabs` tab, `useBackendQuery`, outbound HTTP to an allowlisted host, plugin KV, settings |
| [gamemode](gamemode/)             | backend      | A `launch.before` interceptor with the `process` permission, read-only folder grants, settings          |
| [caskade-pack](caskade-pack/)     | UI + backend | `install-handler` and `url-resolver` providers (#55); translated into Swedish (`locales/sv.po`, #219)   |
| [crashy](crashy/)                 | UI + backend | Test-only (#134): every contribution and hook fails, to prove isolation                                 |

Build them into each plugin's `dist/`:

```sh
node plugins/examples/build.mjs gamemode howlongtobeat playtime-stats
```

UI halves are built with the app's Vite and `@caskade/vite-plugin`. Backends are built with cargo for `wasm32-wasip2` (`rustup target add wasm32-wasip2`).

To try one, load the folder in place with `caskade dev plugins/examples/<name>` (a development build only: `dev` must be the first argument, and a dev plugin gets its permissions without review), or install it from Settings → Plugins → Install from folder.

Tests:
- `crates/caskade-plugin-host/tests/examples.rs`: each plugin installs from its folder with exactly its declared permissions. GameMode wraps a fixture launch in a fake `gamemoderun`, and HowLongToBeat queries a mock server and caches the result.
- `apps/desktop/src/plugins/examples.test.tsx`: the UI halves render.
