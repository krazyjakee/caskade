# Caskade plugins

How a plugin is laid out, what `plugin.json` declares, and the backend
contract. How the plugin host fits into Caskade, and its security model,
are in the [tech spec](../TECH-SPEC.md).

## Folder layout

A plugin is a folder with a `plugin.json` and up to two halves:

```
dev.example.howlongtobeat/
├── plugin.json
├── dist/
│   ├── ui.js            # UI half: an ES module (#127)
│   └── backend.wasm     # backend half: a WebAssembly component
└── locales/             # optional: <lang>.po translations (#219)
```

Caskade looks for plugin folders in `<app data>/plugins/`. Each folder is
validated on its own, so one broken plugin never hides the others. Reading
the folder runs no plugin code. A plugin starts only after the user enables
it, on its first use.

## `plugin.json`

The JSON Schema is [`plugin.schema.json`](plugin.schema.json). Point
`"$schema"` at it for editor completion. It is generated from the host's
types (`cargo test -p caskade-plugin-host` checks it is current;
`CASKADE_WRITE_SCHEMA=1` rewrites it).

| Key           | Meaning                                                                                                                 |
| ------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `id`          | Reverse-DNS, lowercase: `dev.example.howlongtobeat`.                                                                     |
| `name`        | Shown in Settings → Plugins.                                                                                            |
| `version`     | Semver.                                                                                                                 |
| `api`         | Semver range of the `caskade:plugin` API it targets, e.g. `^0.2`. The current and the previous 0.x minor are accepted: `^0.1` plugins still load while 0.1 is deprecated (see [UI](#ui-02)). |
| `ui`          | 0.1 only, deprecated: path of the JavaScript UI module inside the folder. From 0.2 the backend draws the UI, and a `ui` key is an error. |
| `backend`     | Path of the backend component inside the folder.                                                                        |
| `permissions` | `network` (host or `host:port`), `fs` (`folder` or `folder:ro`), `process` (bool), `library` (`none`, `read`, `write`). |
| `activation`  | `onStartup`, `onRequest`, `onView:<id>`, `onEvent:<name>`, `onCommand:<id>`.                                            |
| `contributes` | `slots`, `views`, `hooks`, `providers`, `commands`, `settings` (a JSON Schema), `emulation` (data files; validated but not loaded yet), `themes` (theme folders, see [theming](../theming/README.md#theme-packages-139)). |

Paths must stay inside the plugin folder; `..`, absolute paths and symlinks
that lead out are rejected. Unknown keys are errors. Every error names the
key, e.g. `permissions.library: unknown variant`.

Hook points (`contributes.hooks`) are the bus events (`app.ready`,
`library.game.added`, `game.exited`, `install.queued`, `install.downloaded`,
`install.completed`, `install.failed`, `uninstall.completed`, `game.moved`,
`achievements.changed` (a game's achievements were fetched: its `game_id`
only; unlock announcements, which name accounts and achievements, aren't
offered), …) and the interceptor points `launch.before`, `launch.after`,
`library.import`, `metadata.merge`, `install.plan`, `install.after` and
`install.moved`. `metadata.merge` is reserved: manifests may name it, but
nothing calls it yet. `install.progress` is for the UI only. UI
contribution points (`contributes.slots[].slot`) are `views`,
`home.sections`, `game.details.tabs`, `game.details.panels`,
`game.card.badges`, `game.card.overlay`, `game.actions`, `topbar.items`,
`sidebar.items`, `settings.pages`, `commands` and `notifications`.

## Backend contract

[`wit/caskade-plugin.wit`](../../wit/caskade-plugin.wit) defines the
package `caskade:plugin@0.2.0`. A backend is a WebAssembly component
targeting the `plugin` world:

- **imports `host`:** `log`, `kv-get`/`kv-set`/`kv-delete` (the plugin's own
  store; keys up to 256 bytes, values up to 64 KiB, 4 MiB per plugin),
  `emit` (publishes `plugin.event` on the bus), `settings-get`,
  `install-input-read` (an install handler's input, during its calls),
  `locale` (the language Caskade shows text in, since 0.1.1), and `message`
  and `plural` (text from the plugin's own catalog in that language, since
  0.2.0).
- **exports `hooks`:** `on-event` and `intercept` (continue with a rewritten
  payload, or cancel with a reason).
- **exports `requests`:** `handle-request`, for calls to the backend by name.
- **exports `ui`** (optional, the `ui-plugin` world; since 0.2.0): `render`
  and `act`, which draw the plugin's slots and views ([UI](#ui-02)).

**Versions.** 0.1.1 added `host.locale` (#219). 0.2.0 added the `ui` export
and `host.message`/`host.plural` (#109), and moved plugins' UI from
JavaScript halves to the backend. The host links components built for any
0.1.x or 0.2.x: the 0.1 world, frozen at 0.1.1, stays linked beside 0.2
while 0.1 is deprecated, so 0.1 backends load unchanged. A backend that
needs something newer declares it in `api` (`^0.2`), so an older Caskade
refuses it in the manifest instead of failing to link it.

Payloads cross as JSON text.

**Providers.** A plugin that lists a provider in `contributes.providers`
exports the matching interface. Its world includes `plugin` and adds those
exports (`all-providers` exports all five). The host finds them on the
component and adapts them to the same core traits as the built-ins:

| Provider            | Answer (JSON)                                                    | Needs             |
| ------------------- | ---------------------------------------------------------------- | ----------------- |
| `library-source`    | `scan` → a list of discovered games, imported by library scans   | `library: write`  |
| `metadata-provider` | `lookup(game, fields)` → `{description, genres, releaseDate, artwork: [{kind, url}]}`. The host implements it (`PluginHost::apply_metadata`), but the app doesn't call it yet. | `network` |
| `runner`            | `prepare({runnerId, payload})` → a launch command for `LaunchSpec::Plugin` | `process` |
| `install-handler`   | `claims(input)` → bool; `plan(input)` → `{steps, confidence}`     |                   |
| `url-resolver`      | `resolve(url)` → `[{url, headers, fileName, size, sha256}]`; asked only for granted hosts | `network` |

A library source's games are tagged `plugin:<id>`, whatever source they
claim.

**Installing (#55).** An install handler teaches the installer a format.
It is asked about every input after download and probing, with
`{artifactId, name, size, format, facts}`, and reads the bytes with
`host.install-input-read(offset, length)` (at most 1 MiB per read, only
during `claims` and `plan`). It never sees the input as a file and needs
no `fs` permission. Its plan uses the installer's
[`Step` model](../../crates/caskade-installer/src/types/step.rs):
`extract`, `copy`, `copy_range` (bytes of the input to a file),
`write_file`, `set_executable` and `symlink`. Steps that run programs or
read other files are refused. The plan is ranked with the built-in
handlers', and the installer writes the files into the game folder.

A URL resolver turns a page on a granted host into downloads; the
installer fetches them. `headers` (e.g. an API key) are sent to the
download's origin only, not across redirects to other hosts. Each download,
and every redirect it takes, must be on a granted host and reach a public
address: a loopback, private or link-local address only when `network`
names that address itself (`192.168.1.20`, `[::1]`, or `localhost` for
loopback), not through a name that resolves to it. Downloads match an
entry's host and ignore its port.

`install.plan` sees the ranked plans before anything runs. It may reorder,
drop or select candidates, or cancel to stop the install; it can't change
a step. `install.after` (needs `process` or an `fs` grant) and
`install.moved` run after an install or a move; a cancel there becomes a
warning on the job.

[`plugins/examples/caskade-pack`](../../plugins/examples/caskade-pack) is a
complete example: it installs `.cpk` packs and finds them on download
pages, with only a `network` permission.

Each call runs with limits. Memory is capped at 64 MiB per linear memory,
with at most 2 memories, 8 tables of 100,000 elements and 16 core instances
per component instance, and a call has 2 s of time (epoch interruption). A backend with a `network`
permission has 30 s per call instead, since its calls wait on remote
servers. If a call traps, times out or hits a limit, only that call fails,
and the next call starts a fresh instance.

**Compiled-component cache (#136).** Compiling a backend takes about 20 ms
for a Rust plugin, and much longer for large components. The app keeps the
compiled artifact in `<cache>/plugin-components/` and loads it on later
starts instead of compiling again. Loading compiled code skips wasmtime's
validation, so nothing unverified is loaded:

- An entry's name is a SHA-256 over the component bytes, the wasmtime
  version and the engine's compatibility hash. A changed plugin, a wasmtime
  update or other engine settings use another entry.
- Each entry carries an HMAC-SHA256 over its name and artifact. The key is
  random per install and kept in the app database, not in the cache
  folder, so a file written by anything else, or copied from another entry,
  fails verification.
- Any failure (missing key, bad MAC, short or unreadable file, an artifact
  wasmtime refuses) means compile again and overwrite the entry.

The one `unsafe` call this needs (`Component::deserialize`) lives in
`crates/caskade-component-cache`, which denies `unsafe_code` everywhere
else. The rest of the workspace keeps `unsafe_code = "forbid"`.

## Translations (#219)

A plugin can show in the user's language. It ships gettext catalogs in
`locales/<lang>.po`, named by the language's BCP 47 tag (`sv.po`,
`pt-BR.po`), in a domain named after the plugin's id. The English text is
the message id, as in Caskade itself
([tech spec, Localization](../TECH-SPEC.md#localization)); `msgctxt` tells apart the
same English with different meanings, and `{name}` placeholders are
filled in after the lookup. The language is Caskade's: the core resolves
the Language setting, and `pt-BR` falls back to `pt`. Anything missing,
empty or fuzzy shows its English.

- **Manifest text.** The host looks up the name, the description, slot,
  view and command titles (`game.actions` included) and the settings
  schema's `title`s and `description`s in the plugin's catalog. Settings →
  Plugins, the plugin's settings page (`settings_sections`) and
  `plugins_ui` get them translated; the manifest itself, ids and the
  stored values stay as written.
- **UI half.** `ctx.i18n` and, inside contributions, `useTranslation()`
  have `t(message, values?, { context? })` and
  `plural(count, one, other, values?)`; `<Trans>` takes plain text as its
  child, or `message` with React elements in `values`:

  ```tsx
  import { definePlugin, Trans, useTranslation } from "@caskade/sdk";

  function Panel() {
    const { t, plural } = useTranslation();
    return (
      <section aria-label={t("Game packs")}>
        <Trans message="Drop a pack on {library}" values={{ library: <b>{t("the library")}</b> }} />
        {plural(count, "One pack", "{n} packs")}
      </section>
    );
  }

  export default definePlugin({
    activate(ctx) {
      ctx.slots.register("game.actions", { id: "repack", title: ctx.i18n.t("Repack"), run });
    },
  });
  ```

  The app fetches the catalog (`plugin_messages`) before `activate`. When
  the language changes (`LocaleChanged`, the `locale.changed` plugin
  event), it activates every plugin again with the new catalog, so titles
  registered in `activate` and rendered text both follow.
- **Backend.** From 0.2, `host.message` and `host.plural` look text up in
  the plugin's catalog in the language in use (Rust SDK: `i18n::t`,
  `i18n::tc` with a context, `i18n::tf` with `{name}` values and
  `i18n::plural`), for what `render` draws and for the backend's own
  messages. `host.locale()` answers the language itself.
- **Extraction.** `create-caskade-plugin` gives every plugin an empty
  `locales/` and `scripts/extract-messages.mjs` (`npm run i18n:extract`).
  It writes `locales/<plugin id>.pot` from `plugin.json`, the UI's `t()`,
  `plural()` and `<Trans>`, and a backend's `tr("…")`, then merges it into
  each catalog without losing translations; `--add <lang>` starts one,
  `--check` fails when one is out of date.
- **Problems.** A catalog that can't be read, isn't named after a
  language, or has a translation with a placeholder its English lacks is
  one of the plugin's `warnings` (`PluginSummary.warnings`, keyed
  `locales/<file>`). The plugin still loads and runs, in English. A
  plugin without catalogs is English, with no warnings.

`plugins/examples/caskade-pack` is the worked example, in Swedish: its
name, settings and game action from `locales/sv.po`, and its install
handler's errors through `host.locale`.

## Permissions

A backend gets only what its manifest declares and the user granted.
Enabling a plugin, or installing it from a folder after reviewing it,
grants what it declares. If an update asks for more, or adds a UI half
the grant didn't cover, the plugin stays off (`needs_consent`) until it's
enabled again.

- **`network`:** outbound `wasi:http` only (no sockets). Each request's
  host must be in the list: an exact host, or `*.domain` for its
  subdomains. Ports are 80 and 443 unless an entry names one
  (`example.com:8443`, `127.0.0.1:8384`, `[::1]:8080`); an entry with a
  port allows only that port. The host resolves the name itself and
  connects only to public addresses: a loopback, private, link-local,
  shared (CGNAT) or unspecified address only when an entry names that
  address itself, with the port (`localhost` names loopback), never
  through a name that resolves to it (DNS rebinding). A request to a host
  or port outside the grant fails with `HTTP-request-denied`, one whose
  name resolves only to addresses the grant doesn't allow with
  `destination-IP-prohibited`. Redirects are new requests, so they are
  checked too. Only the host and port of a refused request are logged.
- **`fs`:** each entry (`$HOME/ROMs:ro`, `~/ROMs`, an absolute folder
  such as `C:\Games`) is visible to the backend at its own path,
  read-only with `:ro` and read/write without a suffix or with `:rw`; a
  drive letter's colon is not a suffix. Nothing else is visible, except
  the plugin's private data folder at `/data`.
- **`library`:** `read` allows `library-query`; `write` also allows
  `library-update`.
- **`process`:** only plugins with `process: true` may change what runs:
  a launch command's `program`, `args`, `env`, `cwd` or `emulation` in
  `launch.before`, and an imported game's `install_dir`, `launch`,
  `runtime`, `uninstall` or `tracking` in `library.import`
  (`capabilities::launch_rewrite_allowed` and `import_rewrite_allowed`,
  applied by the hook adapter).

## Backend languages

Plugin API 0.2 supports **Rust** backends only, as 0.1 did. **JavaScript/TypeScript
backends are not supported**: from 0.2 the backend draws the UI too, so a
plugin with UI is written in Rust. A jco-built TypeScript backend can pass the host's
functional checks, but it can't make HTTP calls from the 0.1 exports (they
are synchronous, and `fetch()` never settles). It is also about 70× larger,
about 30× slower per call and needs 12 MiB per instance. The full measurements
are in archived Gitea issue 132.

### Rust backend: `caskade-plugin-sdk`

[`crates/caskade-plugin-sdk`](../../crates/caskade-plugin-sdk) carries the
bindings of the `caskade:plugin` WIT package (its version is the WIT
version). A backend implements one `Plugin` trait, whose methods all have
defaults, and calls `export!`:

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
caskade-plugin-sdk = { path = "…/crates/caskade-plugin-sdk" }
serde_json = "1"
```

```rust
use caskade_plugin_sdk::{Error, Intercept, Plugin, host, json, payloads};

struct Stats;

impl Plugin for Stats {
    // Events listed in `contributes.hooks`.
    fn on_event(name: &str, payload: &str) {
        // Event payloads arrive as `{"version": 1, "data": …}`.
        if name == "game.exited"
            && let Ok(session) = json::event::<payloads::GameSession>(payload)
        {
            host::kv_set("last", session.game_id.as_bytes());
        }
    }
    // Interceptor points listed in `contributes.hooks`.
    fn intercept(point: &str, payload: &str) -> Result<Intercept, Error> {
        Ok(Intercept::Continue(payload.to_owned()))
    }
    // Calls from the UI half: `ctx.backend.call(method, payload)`.
    fn request(method: &str, payload: &str) -> Result<String, Error> {
        match method {
            "settings" => json::to_string(&host::settings::<serde_json::Value>()?),
            _ => Err(Error::unknown_method(method)),
        }
    }
}

caskade_plugin_sdk::export!(Stats);
```

Providers are trait methods too (`library_scan`, `metadata_lookup`,
`runner_prepare`, `install_claims`/`install_plan`, `resolve_url`); the
host calls only the ones in `contributes.providers`. `host` has the host
functions plus JSON helpers (`settings`, `kv_json`, `set_kv_json`), `json`
parses and writes payloads (`json::event` unwraps an `on-event` payload;
`intercept` payloads aren't wrapped), and `payloads` has the shapes the core
sends.
`render` and `act` draw the plugin's UI ([UI](#ui-02)), and `i18n` has
`t`, `tc`, `tf` and `plural` over the plugin's own catalog.
`cargo build --release --target wasm32-wasip2` produces the component. The
host's test guest (`crates/caskade-plugin-host/tests/guest`) is a complete
example.

## UI (0.2)

From API 0.2 (#109) a plugin's UI is drawn by its backend: `render` returns
the slot or view as a tree of Caskade's own components, as JSON, and Caskade
draws it with its components. The plugin gets the theme, focus, pad input,
console mode and screen readers without doing anything, and no plugin code
runs outside its sandbox. The node types are
[`crates/caskade-plugin-ui`](../../crates/caskade-plugin-ui), which the
Rust SDK re-exports as `ui`, with builders (`ui::heading`, `ui::button`,
…).

- **What draws.** Only the slots and views `plugin.json` declares. Caskade
  calls `render(slot, id, context)` when one shows: `slot` is the slot
  (`views` for a view), `id` the contribution's, and `context` is
  `{"mode": "desktop" | "console", "gameId"?: "…", "state": …}`. Game slots
  get the game. Titles come from `plugin.json`, translated by the plugin's
  catalog.
- **What the user does.** A `button` or `segmented` control names an
  `action`. Using it calls `act(slot, id, action, value, context)`, which
  returns `{"state"?: …, "notice"?: "…"}`. Caskade keeps `state` for that
  contribution and passes it to the next `render`, shows `notice` as the
  plugin's notification, and draws the contribution again. A `game.actions`
  or `commands` entry calls `act` with the action `run`.
- **Checks.** A tree is at most 256 KiB, 12 levels deep and 2000 nodes,
  with texts up to 4000 characters and action ids of 1 to 64 letters,
  digits, `-`, `_`, `.` or `:`. Anything else (an unknown node or field, a
  ragged table, a heading level other than 2 or 3) is refused, and the
  plugin's placeholder shows instead, as when `render` fails.
- **Translations.** `i18n::t("Most played")` and
  `i18n::plural("{count} game", "{count} games", n)` read the plugin's
  `locales/<lang>.po` in the language Caskade is in ([Translations](#translations-219)).

| Node (`type`) | Fields | Draws |
| ------------- | ------ | ----- |
| `text`        | `text` | Plain text. |
| `heading`     | `text`, `level` (2 or 3, default 2) | A heading. |
| `paragraph`   | `text` | A paragraph. |
| `note`        | `text` | Secondary text, muted. |
| `alert`       | `text` | Something that went wrong, announced. |
| `stack`       | `children`, `direction` (`column` or `row`), `gap` (`none`, `small`, `medium`, `large`) | Children in a column or a row. |
| `section`     | `label`, `children` | A labelled part that pad focus moves between. |
| `stats`       | `items` (`label`, `value`), `label`? | Labelled values. |
| `tile`        | `value`, `label` | One large value. |
| `bars`        | `label`, `items` (`label`, `value`, `text`) | A bar chart; `text` is each value for reading. |
| `table`       | `caption`, `columns`, `rows`, `rowHeaders` | A table. |
| `list`        | `items`, `ordered` | A list of nodes. |
| `badge`       | `text`, `tone` (`neutral`, `accent`, `success`, `warning`, `danger`) | A short status label. |
| `progress`    | `label`, `value`, `max` | A progress bar. |
| `button`      | `label`, `action`, `value`?, `variant` (`secondary`, `primary`) | Calls `act`. |
| `segmented`   | `label`, `action`, `value`, `options` (`value`, `label`) | One of up to 8 options; calls `act` with the chosen value. |

```rust
use caskade_plugin_sdk::{Error, Plugin, i18n::t, ui};

struct Stats;

impl Plugin for Stats {
    fn render(slot: &str, id: &str, context: &ui::Context) -> Result<Vec<ui::Node>, Error> {
        let view = context.state["view"].as_str().unwrap_or("chart").to_owned();
        Ok(vec![
            ui::heading(t("Most played")),
            ui::segmented(t("Show as"), "view", view, [("chart", t("Chart")), ("table", t("Table"))]),
        ])
    }
    fn act(
        slot: &str,
        id: &str,
        action: &str,
        value: &serde_json::Value,
        context: &ui::Context,
    ) -> Result<ui::Outcome, Error> {
        Ok(ui::Outcome { state: Some(serde_json::json!({"view": value})), notice: None })
    }
}
```

The manifest asks for `"api": "^0.2"`, names a `backend` and no `ui`
module. Slots and views need the backend that draws them.

### Console mode

Console mode (#148) is the same app on a TV with a controller: a fullscreen
shell, bigger type and a button-hint bar. A plugin's UI joins it on the
same terms as the built-in views (#152).

- **Where things show.** A view or slot contribution declares `modes`:
  `["desktop"]`, `["console"]` or both. Without `modes`, views show in both;
  slots on the game page and Home (`game.details.tabs`,
  `game.details.panels`, `game.actions`, `home.sections`) show in both, and
  every other slot only on the desktop. A console view gets an entry in the
  console sidebar. A link into a view the current mode hides shows "not
  available in console mode".

  ```json
  "views": [{ "id": "stats", "title": "Playtime stats", "modes": ["desktop", "console"] }]
  ```

- **Focus and buttons.** Buttons and segmented controls are focus stops,
  and a `section` is a focus group that brings focus back to where it last
  was. A (confirm) presses the focused control, B goes back, and in a view
  LB and RB move a segmented control between its options. `context.mode`
  says which mode a tree is for.
- **Crashes.** A contribution that fails to draw is replaced by its
  placeholder, and focus that was inside it stays on the placeholder, so a
  pad can move on.

### Migrating from 0.1

0.1 plugins are deprecated. They still load: their backends link to the
0.1 world, and their JavaScript UI halves run in the webview app until the
native app replaces it (BLITZ-33). The native app doesn't run JavaScript,
so it draws no 0.1 UI, and Settings → Plugins says the plugin's UI needs an
update. To move a plugin to 0.2:

1. Set `"api": "^0.2"` and remove `ui` from `plugin.json`. Keep the
   `slots` and `views` entries; their titles now come only from the
   manifest.
2. Depend on `caskade-plugin-sdk` 0.2, and move each registered component
   into `render`: `match (slot, id)` and return its tree. State a
   component kept in React (`useState`) goes in the `state` that `act`
   returns. Data a component fetched with `useBackendQuery` is read in
   `render` itself.
3. Move `game.actions` and `commands` callbacks into `act` with the action
   `run`; `ctx.notify(message)` becomes the outcome's `notice`.
4. Move the UI half's texts to `i18n::t` (the catalog stays the same) and
   delete `src/`, the Vite config and the `@caskade/sdk` dependency.

## UI half (0.1, deprecated)

This is how 0.1 plugins drew their UI. It still works in the webview app
until BLITZ-33; new plugins use [UI (0.2)](#ui-02).

A UI half is one ES module whose default export is `definePlugin(...)`.
Inside Caskade, `react`, `react-dom`, `react/jsx-runtime`, `@caskade/sdk`
and `@caskade/ui` resolve through the app's import map to the app's own
instances. So a plugin bundles none of them, and it shares the app's React
tree, query client and components.

- [`packages/sdk`](../../packages/sdk) (`@caskade/sdk`, versioned with the
  WIT package): `definePlugin`, the `PluginContext`
  (`slots.register`, `events.on`, `backend.call`, `settings`, `notify`,
  `i18n`), the hooks `usePlugin`, `useGame`, `useBackendQuery`,
  `useSettings` and `useTranslation`, `<Trans>` (see
  [Translations](#translations-219)),
  and for console mode `useMode`, `useInputAction`, `useFocusable` and
  `FocusSection` (see [Console mode](#console-mode)).
  The event payload types (`PluginEvents`) are generated from the core's
  bindings (`pnpm --filter @caskade/sdk core-types`).
- [`packages/ui`](../../packages/ui) (`@caskade/ui`): the shell's form
  primitives (`Button`, `Switch`, `Select`, `TextField`, `Slider`,
  `SettingsSection`, …), `GameArt`, `GameCard`, `Hero`, `Shelf`,
  `StatusBadge` and the anatomy catalogue
  ([`docs/theming/anatomy.md`](../theming/anatomy.md)). They come from the
  app at runtime, so they follow the active theme.
- [`packages/vite-plugin`](../../packages/vite-plugin)
  (`@caskade/vite-plugin`) builds `src/index.tsx` into `dist/ui.js` (and
  `dist/ui.css`, in the `cs.plugins` cascade layer) with the shared modules
  left external:

```ts
// vite.config.ts
import { caskadePlugin } from "@caskade/vite-plugin";
import { defineConfig } from "vite";

export default defineConfig({ plugins: [caskadePlugin()] });
```

```tsx
// src/index.tsx
import { definePlugin, useBackendQuery } from "@caskade/sdk";
import { SettingsSection } from "@caskade/ui";

function Hours({ gameId }: { gameId: string }) {
  const hours = useBackendQuery<{ main: number }>("hours", { gameId });
  return <SettingsSection title="How long to beat">{hours.data?.main ?? "…"} h</SettingsSection>;
}

export default definePlugin({
  activate(ctx) {
    ctx.slots.register("game.details.tabs", { id: "hltb", title: "How long", component: Hours });
  },
});
```

`plugin.json` then points `ui` at `dist/ui.js`; the app loads `dist/ui.css`
from beside it when it exists.

### Console mode (0.1)

0.1 UI halves joined console mode through the SDK: `useMode`,
`useInputAction(action, handler, { label, owner })`, `useFocusable` and
`FocusSection`, with the actions `confirm`, `back`, `details`, `search`,
`tab.prev`/`tab.next`, `page.prev`/`page.next`, `menu`, `view`, `guide` and
the four `nav.*` directions. 0.2 trees get all of it from Caskade's own
components.
