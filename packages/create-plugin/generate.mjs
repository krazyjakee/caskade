// Writes a new Caskade plugin (#130): a Rust backend (`caskade-plugin-sdk`)
// that handles a game event and, with `ui`, draws a view with Caskade's
// components (plugin API 0.2, #109). No dependencies: Node's own modules.
import { mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

/** The `caskade:plugin` API the templates target (`plugin.json` `api`). */

/** A file beside this script. Vitest's jsdom tests import it from an http
 * URL (`/@fs/<path>`) rather than a file URL. */
function besideThis(path) {
  const url = new URL(path, import.meta.url);
  return url.protocol === "file:"
    ? fileURLToPath(url)
    : decodeURIComponent(url.pathname.replace(/^\/@fs(?=\/)/, ""));
}
export const API = "^0.2";

/** Reverse-DNS, as the host checks it: lowercase dot-separated parts. */
export function validId(id) {
  const parts = id.split(".");
  return (
    id.length <= 128 &&
    parts.length >= 2 &&
    parts.every((part) => /^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/.test(part))
  );
}

/** `dev.example.my-plugin` → `my_plugin` (crate) / `MyPlugin` (type). */
function names(id) {
  const last = id.split(".").at(-1);
  const words = last.split("-").filter(Boolean);
  const crate = words.join("_");
  const type = words.map((word) => word[0].toUpperCase() + word.slice(1)).join("");
  return {
    last,
    crate: /^[0-9]/.test(crate) ? `plugin_${crate}` : crate,
    type: /^[0-9]/.test(type) ? `Plugin${type}` : type,
  };
}

const json = (value) => `${JSON.stringify(value, null, 2)}\n`;

function manifest({ id, name, ui }) {
  const contributes = {};
  // Desktop and console mode (a controller on the couch); drop "console"
  // if the view needs a mouse.
  if (ui) contributes.views = [{ id: "main", title: name, modes: ["desktop", "console"] }];
  contributes.hooks = ["game.exited"];
  return json({
    $schema: "https://caskade.dev/schemas/plugin.json",
    id,
    name,
    version: "0.1.0",
    api: API,
    description: `${name}: a Caskade plugin.`,
    backend: "dist/backend.wasm",
    // Ask for nothing until the plugin needs it; the user reviews these.
    permissions: {},
    // Start the backend when its view opens or a game exits, not at launch.
    activation: ui ? ["onView:main", "onEvent:game.exited"] : ["onEvent:game.exited"],
    contributes,
  });
}

function packageJson({ id }) {
  return json({
    name: names(id).last,
    version: "0.1.0",
    private: true,
    type: "module",
    scripts: {
      build: "node scripts/build-backend.mjs",
      dev: "caskade dev .",
      // locales/<plugin id>.pot from plugin.json and the sources (#219).
      "i18n:extract": "node scripts/extract-messages.mjs",
    },
  });
}

function backendFiles({ id, name, ui, sdk }) {
  const { crate, type } = names(id);
  const dependency = sdk
    ? `{ path = ${JSON.stringify(join(sdk, "crates/caskade-plugin-sdk"))} }`
    : `"0.2"`;
  return {
    "backend/Cargo.toml": `[package]
name = "${crate}"
version = "0.1.0"
edition = "2024"
publish = false

[lib]
# A WebAssembly component for Caskade's caskade:plugin@0.2.0 world.
crate-type = ["cdylib"]

[dependencies]
caskade-plugin-sdk = ${dependency}
serde_json = "1"

[profile.release]
opt-level = "s"
lto = true
strip = true

# Not part of any enclosing workspace.
[workspace]
`,
    "backend/src/lib.rs": ui
      ? `//! ${name}: a Caskade plugin. Its backend, a WebAssembly component that
//! Caskade runs in a sandbox, draws its view with Caskade's components and
//! hears what the user does with them.
use caskade_plugin_sdk::{
    Error, Level, Plugin, host,
    i18n::{plural, t, tf},
    ui,
};

struct ${type};

impl Plugin for ${type} {
    /// The view \`plugin.json\` declares. Text comes from locales/<lang>.po in
    /// the language Caskade is in; English is the message id, and
    /// \`npm run i18n:extract\` collects the messages.
    fn render(slot: &str, id: &str, context: &ui::Context) -> Result<Vec<ui::Node>, Error> {
        if (slot, id) != ("views", "main") {
            return Err(Error::new("no_ui", format!("no {slot} {id}")));
        }
        // What \`act\` returned last time.
        let clicks = context.state.get("clicks").and_then(|clicks| clicks.as_u64()).unwrap_or(0);
        Ok(vec![ui::section(
            ${JSON.stringify(name)},
            [
                ui::paragraph(tf("Hello from {name}!", &[("name", ${JSON.stringify(name)})])),
                // A focus stop for a pad, like every button: A presses it.
                ui::button(t("Click"), "click"),
                ui::note(plural("Clicked {count} time", "Clicked {count} times", clicks)),
            ],
        )])
    }

    /// The button's action: the new count, drawn again.
    fn act(
        _slot: &str,
        _id: &str,
        action: &str,
        _value: &serde_json::Value,
        context: &ui::Context,
    ) -> Result<ui::Outcome, Error> {
        match action {
            "click" => {
                let clicks = context.state.get("clicks").and_then(|clicks| clicks.as_u64()).unwrap_or(0);
                Ok(ui::Outcome {
                    state: Some(serde_json::json!({ "clicks": clicks + 1 })),
                    notice: None,
                })
            }
            _ => Err(Error::unknown_method(action)),
        }
    }

    /// The events \`contributes.hooks\` lists in plugin.json.
    fn on_event(name: &str, payload: &str) {
        if name == "game.exited" {
            host::log(Level::Info, &format!("a game exited: {payload}"));
        }
    }
}

caskade_plugin_sdk::export!(${type});
`
      : `//! ${name}: a Caskade plugin. Its backend, a WebAssembly component that
//! Caskade runs in a sandbox, handles a game event. Payloads cross as JSON
//! text.
use caskade_plugin_sdk::{Level, Plugin, host};

struct ${type};

impl Plugin for ${type} {
    /// The events \`contributes.hooks\` lists in plugin.json.
    fn on_event(name: &str, payload: &str) {
        if name == "game.exited" {
            host::log(Level::Info, &format!("a game exited: {payload}"));
        }
    }
}

caskade_plugin_sdk::export!(${type});
`,
    "scripts/build-backend.mjs": `// Builds backend/ for wasm32-wasip2 and copies the component to
// dist/backend.wasm (cargo's target folder may be anywhere).
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";

const output = execFileSync(
  "cargo",
  ["build", "--release", "--target", "wasm32-wasip2", "--manifest-path", "backend/Cargo.toml", "--message-format=json-render-diagnostics"],
  { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"], maxBuffer: 64 << 20 },
);
const wasm = output
  .split("\\n")
  .filter((line) => line.startsWith("{"))
  .map((line) => JSON.parse(line))
  .filter((message) => message.reason === "compiler-artifact" && message.target.name === "${crate}")
  .flatMap((message) => message.filenames)
  .find((file) => file.endsWith(".wasm"));
if (!wasm) throw new Error("cargo built no ${crate}.wasm; is the wasm32-wasip2 target installed?");
mkdirSync("dist", { recursive: true });
copyFileSync(wasm, "dist/backend.wasm");
console.log("dist/backend.wasm");
`,
  };
}

function readme({ id, name, ui }) {
  return `# ${name}

\`${id}\`: a Caskade plugin with a Rust backend${ui ? " that draws a view" : ""}.

## Build and try it

The backend needs Rust with the WebAssembly component target: \`rustup target add wasm32-wasip2\`.

\`\`\`sh
npm run build   # dist/backend.wasm
caskade dev .   # load this folder in place; changes reload while Caskade runs
\`\`\`

In a development build of Caskade, Settings → Plugins → **Load unpacked** does the same.

## Layout

- \`plugin.json\`: id, backend, permissions and everything the plugin contributes. Caskade reads it without running code.
- \`locales/\`: translations. \`npm run i18n:extract\` writes \`locales/${id}.pot\` from \`plugin.json\` and the backend's \`t()\`, \`tf()\` and \`plural()\`; \`npm run i18n:extract -- --add sv\` starts a Swedish \`locales/sv.po\`. Caskade shows the plugin's name, titles${ui ? ", settings and view" : " and settings"} in the user's language, and English for anything not translated.
- \`backend/src/lib.rs\`: the backend, a \`caskade_plugin_sdk::Plugin\`. It runs sandboxed: ask for \`network\`, \`fs\`, \`process\` or \`library\` in \`permissions\` when you need them.${ui ? " Its \\`render\\` draws the view with Caskade's components (\\`ui::heading\\`, \\`ui::button\\`, …), which work in console mode too, and \\`act\\` hears what the user does." : ""}
- \`scripts/build-backend.mjs\`: builds the component and copies it to \`dist/backend.wasm\`.

See \`docs/plugins/\` in the Caskade repository for the manifest, the components and the hook reference.
`;
}

/**
 * Writes the plugin into `dir`, which must not exist or be empty.
 * @param {{ id: string, name: string, ui: boolean, backend?: boolean, dir: string, sdk?: string }} options
 * `ui`: a view the backend draws; `backend` may only be true (0.1 had UI-only plugins).
 * `sdk`: a Caskade checkout to link the unpublished SDK packages from.
 * @returns {string[]} the files written, relative to `dir`
 */
export function generate(options) {
  const { id, name, ui, backend } = options;
  if (!validId(id))
    throw new Error(`"${id}" is not a plugin id: use reverse-DNS such as dev.example.hello`);
  if (!name?.trim() || name.length > 128) throw new Error("the name must be 1–128 characters");
  if (backend === false)
    throw new Error(
      "from plugin API 0.2 a plugin's UI is drawn by its backend: every plugin has one",
    );
  const dir = resolve(options.dir);
  let existing = [];
  try {
    existing = readdirSync(dir);
  } catch {}
  if (existing.length > 0) throw new Error(`${dir} is not empty`);
  const sdk = options.sdk && resolve(options.sdk);
  const settings = { id, name: name.trim(), ui, backend, sdk };
  const files = {
    "plugin.json": manifest(settings),
    "package.json": packageJson(settings),
    "README.md": readme(settings),
    ".gitignore": "dist/\nbackend/target/\n",
    // The same extractor as the example plugins' (#219).
    "scripts/extract-messages.mjs": readFileSync(
      besideThis("templates/extract-messages.mjs"),
      "utf8",
    ),
    ...backendFiles(settings),
  };
  for (const [path, text] of Object.entries(files)) {
    const file = join(dir, path);
    mkdirSync(dirname(file), { recursive: true });
    writeFileSync(file, text);
  }
  // Translations go here, as <lang>.po (#219).
  mkdirSync(join(dir, "locales"), { recursive: true });
  return Object.keys(files).map((path) => relative(dir, join(dir, path)));
}
