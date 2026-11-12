# create-caskade-plugin

Scaffolds a Caskade plugin (#130):

```sh
just plugin-new my-plugin                    # asks for the name, id and whether it has a view
just plugin-new my-plugin --id dev.me.hello --no-ui
node packages/create-plugin/index.mjs my-plugin --yes --sdk .   # inside a checkout
```

Options: `--id <reverse-dns>`, `--name <name>`, `--ui`/`--no-ui` (default: a view), `--yes`, `--sdk <caskade checkout>`.

Plugins target plugin API 0.2 (#109): every plugin has a Rust backend, which also draws its UI with Caskade's components. The SDK crate isn't published yet, so pass `--sdk` with a Caskade checkout: `caskade-plugin-sdk` is linked from `crates/`.

The two templates:

| Template    | Files                                                                                                                                                                                                            |
| ----------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| With a view | `plugin.json`, `package.json` (build scripts), `backend/Cargo.toml`, `backend/src/lib.rs` (`render` and `act` draw a view with a button and a count, `on_event` logs games exiting), `scripts/build-backend.mjs` |
| No view     | the same, with a backend that only handles `game.exited`                                                                                                                                                         |

Every template also has a `README.md`, a `.gitignore`, an empty `locales/` and `scripts/extract-messages.mjs` (`npm run i18n:extract`; #219). The script writes `locales/<plugin id>.pot` from `plugin.json` (name, description, titles, settings schema) and the backend's `t()`, `tf()`, `tc()` and `plural()`, and merges it into each `locales/<lang>.po`; `--add <lang>` starts a catalog and `--check` fails when one is out of date. It is `templates/extract-messages.mjs`, copied as is. `plugin.json` asks for no permissions, and the backend starts only on its activation events. `npm run build` writes `dist/backend.wasm`. `caskade dev .` loads the folder in place and reloads it on change.
