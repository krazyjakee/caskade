# create-caskade-plugin

Scaffolds a Caskade plugin (#130):

```sh
just plugin-new my-plugin                    # asks for the name, id and halves
just plugin-new my-plugin --id dev.me.hello --ui --no-backend
node packages/create-plugin/index.mjs my-plugin --yes --sdk .   # inside a checkout
```

Options: `--id <reverse-dns>`, `--name <name>`, `--ui`/`--no-ui`, `--backend`/`--no-backend`, `--yes` (defaults: both halves), `--sdk <caskade checkout>`.

The SDK packages aren't published yet, so pass `--sdk` with a Caskade checkout. It links `@caskade/sdk` and `@caskade/vite-plugin` from `packages/`, and `caskade-plugin-sdk` from `crates/`.

The three templates:

| Halves       | Files                                                                                                                  |
| ------------ | ---------------------------------------------------------------------------------------------------------------------- |
| UI only      | `plugin.json`, `package.json`, `vite.config.ts`, `tsconfig.json`, `src/index.tsx`, `src/style.css`                     |
| Backend only | `plugin.json`, `package.json` (build scripts), `backend/Cargo.toml`, `backend/src/lib.rs`, `scripts/build-backend.mjs` |
| Both         | everything above. The view asks the backend `hello` through `useBackendQuery`                                          |

Every template also has a `README.md`, a `.gitignore`, an empty `locales/` and `scripts/extract-messages.mjs` (`npm run i18n:extract`; #219). The script writes `locales/<plugin id>.pot` from `plugin.json` (name, description, titles, settings schema), the UI's `t()`, `plural()` and `<Trans>`, and a backend's `tr("…")`, and merges it into each `locales/<lang>.po`; `--add <lang>` starts a catalog and `--check` fails when one is out of date. It is `templates/extract-messages.mjs`, copied as is. `plugin.json` asks for no permissions, and the backend starts only on its activation events. `npm run build` writes `dist/ui.js` and/or `dist/backend.wasm`. `caskade dev .` loads the folder in place and reloads it on change.
