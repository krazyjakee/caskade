// Writes a new Caskade plugin (#130): UI half, backend half
// (Rust, `caskade-plugin-sdk`) or both. No dependencies: Node's own modules.
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
export const API = "^0.1";

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

function manifest({ id, name, ui, backend }) {
  const contributes = {};
  // Desktop and console mode (a controller on the couch); drop "console"
  // if the view needs a mouse.
  if (ui) contributes.views = [{ id: "main", title: name, modes: ["desktop", "console"] }];
  if (backend) contributes.hooks = ["game.exited"];
  return json({
    $schema: "https://caskade.dev/schemas/plugin.json",
    id,
    name,
    version: "0.1.0",
    api: API,
    description: `${name}: a Caskade plugin.`,
    ...(ui && { ui: "dist/ui.js" }),
    ...(backend && { backend: "dist/backend.wasm" }),
    // Ask for nothing until the plugin needs it; the user reviews these.
    permissions: {},
    ...(backend && {
      // Start the backend when its view opens or a game exits, not at launch.
      activation: ui ? ["onView:main", "onEvent:game.exited"] : ["onEvent:game.exited"],
    }),
    contributes,
  });
}

function packageJson({ id, ui, backend, sdk }) {
  const scripts = {};
  if (ui) scripts["build:ui"] = "vite build";
  if (backend) scripts["build:backend"] = "node scripts/build-backend.mjs";
  scripts.build = [ui && "npm run build:ui", backend && "npm run build:backend"]
    .filter(Boolean)
    .join(" && ");
  scripts.dev = "caskade dev .";
  // locales/<plugin id>.pot from plugin.json and the sources (#219).
  scripts["i18n:extract"] = "node scripts/extract-messages.mjs";
  // The packages aren't published yet: `--sdk <caskade checkout>` links them.
  const linked = (folder, version) => (sdk ? `link:${join(sdk, folder)}` : version);
  return json({
    name: names(id).last,
    version: "0.1.0",
    private: true,
    type: "module",
    scripts,
    ...(ui && {
      devDependencies: {
        "@caskade/sdk": linked("packages/sdk", "^0.1.1"),
        "@caskade/vite-plugin": linked("packages/vite-plugin", "^0.1.0"),
        "@types/react": "^19.3.0",
        react: "^19.3.1",
        typescript: "^7.0.2",
        vite: "^8.3.5",
      },
    }),
  });
}

function uiFiles({ id, name, backend }) {
  const scope = names(id).last;
  const message = backend
    ? `{hello.data?.message ?? t("Asking the backend…")}`
    : `{t("Hello from {name}!", { name: ${JSON.stringify(name)} })}`;
  return {
    "vite.config.ts": `import { caskadePlugin } from "@caskade/vite-plugin";
import { defineConfig } from "vite";

// Builds src/index.tsx into dist/ui.js (and dist/ui.css). React and the
// Caskade packages come from the app at runtime, so they aren't bundled.
export default defineConfig({ plugins: [caskadePlugin()] });
`,
    "tsconfig.json": json({
      compilerOptions: {
        target: "ES2022",
        lib: ["ES2023", "DOM"],
        module: "ESNext",
        moduleResolution: "bundler",
        jsx: "react-jsx",
        strict: true,
        noEmit: true,
        isolatedModules: true,
        verbatimModuleSyntax: true,
        skipLibCheck: true,
      },
      include: ["src"],
    }),
    "src/index.tsx": `import { definePlugin, FocusSection, useInputAction, useTranslation${backend ? ", useBackendQuery" : ""} } from "@caskade/sdk";
import { useState } from "react";
import "./style.css";

function Main() {
  // Text from locales/<lang>.po in the language Caskade is in; English is
  // the message id. \`npm run i18n:extract\` collects the messages.
  const { t } = useTranslation();
  const [clicks, setClicks] = useState(0);
  // In console mode a pad's buttons are actions: the button below is already
  // a focus stop, and X (details) counts too. The label shows in the hint bar.
  useInputAction("details", () => setClicks((count) => count + 1), { label: t("Click") });${
    backend
      ? `
  // The backend half's \`request\` answers this (backend/src/lib.rs).
  const hello = useBackendQuery<{ message: string }>("hello", { name: "Caskade" });`
      : ""
  }
  return (
    <FocusSection id="${scope}" className="${scope}">
      <p>${message}</p>
      <button type="button" onClick={() => setClicks(clicks + 1)}>
        {t("Clicked {count} times", { count: clicks })}
      </button>
    </FocusSection>
  );
}

export default definePlugin({
  activate(ctx) {
    // Every contribution is declared in plugin.json too.
    ctx.slots.register("views", { id: "main", title: ctx.i18n.t(${JSON.stringify(name)}), component: Main });
  },
});
`,
    "src/style.css": `/* Loaded into the cs.plugins cascade layer; prefix rules with your class. */
.${scope} {
  display: grid;
  gap: 0.75rem;
  justify-items: start;
}
`,
  };
}

function backendFiles({ id, name, sdk }) {
  const { crate, type } = names(id);
  const dependency = sdk
    ? `{ path = ${JSON.stringify(join(sdk, "crates/caskade-plugin-sdk"))} }`
    : `"0.1"`;
  return {
    "backend/Cargo.toml": `[package]
name = "${crate}"
version = "0.1.0"
edition = "2024"
publish = false

[lib]
# A WebAssembly component for Caskade's caskade:plugin@0.1.1 world.
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
    "backend/src/lib.rs": `//! The backend half of ${name}: a WebAssembly component that Caskade runs
//! in a sandbox. Payloads cross as JSON text.
use caskade_plugin_sdk::{Error, Level, Plugin, host, json};

struct ${type};

impl Plugin for ${type} {
    /// The events \`contributes.hooks\` lists in plugin.json.
    fn on_event(name: &str, payload: &str) {
        if name == "game.exited" {
            host::log(Level::Info, &format!("a game exited: {payload}"));
        }
    }

    /// \`ctx.backend.call(method, payload)\` from the UI half.
    fn request(method: &str, payload: &str) -> Result<String, Error> {
        match method {
            "hello" => {
                let input: serde_json::Value = json::from_str(payload)?;
                let who = input.get("name").and_then(|name| name.as_str()).unwrap_or("Caskade");
                json::to_string(&serde_json::json!({ "message": format!("Hello, {who}!") }))
            }
            _ => Err(Error::unknown_method(method)),
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

function readme({ id, name, ui, backend }) {
  const halves = [ui && "a UI half (React)", backend && "a backend half (Rust → WebAssembly)"]
    .filter(Boolean)
    .join(" and ");
  return `# ${name}

\`${id}\`: a Caskade plugin with ${halves}.

## Build and try it

${ui ? "Install the UI tooling once: `pnpm install` (or npm).\n\n" : ""}${
  backend
    ? "The backend needs Rust with the WebAssembly component target: `rustup target add wasm32-wasip2`.\n\n"
    : ""
}\`\`\`sh
npm run build   # ${[ui && "dist/ui.js", backend && "dist/backend.wasm"].filter(Boolean).join(", ")}
caskade dev .   # load this folder in place; changes reload while Caskade runs
\`\`\`

In a development build of Caskade, Settings → Plugins → **Load unpacked** does the same.

## Layout

- \`plugin.json\`: id, halves, permissions and everything the plugin contributes. Caskade reads it without running code.
- \`locales/\`: translations. \`npm run i18n:extract\` writes \`locales/${id}.pot\` from \`plugin.json\`${ui ? " and the UI's `t()` and `<Trans>`" : ""}; \`npm run i18n:extract -- --add sv\` starts a Swedish \`locales/sv.po\`. Caskade shows the plugin's name, titles, settings${ui ? " and UI" : ""} in the user's language, and English for anything not translated.
${
  ui
    ? "- `src/index.tsx`: the UI half. `definePlugin()` registers contributions; `@caskade/sdk` has the context and hooks. React and the SDK come from the app. The view works in console mode too (`modes` in `plugin.json`): its controls are focus stops for a pad, and `useInputAction` gives the pad's buttons jobs.\n- `vite.config.ts`: `@caskade/vite-plugin` builds `dist/ui.js` and puts `src/style.css` into the `cs.plugins` layer.\n"
    : ""
}${
  backend
    ? "- `backend/src/lib.rs`: the backend half, a `caskade_plugin_sdk::Plugin`. It runs sandboxed: ask for `network`, `fs`, `process` or `library` in `permissions` when you need them.\n- `scripts/build-backend.mjs`: builds the component and copies it to `dist/backend.wasm`.\n"
    : ""
}
See \`docs/plugins/\` in the Caskade repository for the manifest and hook reference.
`;
}

/**
 * Writes the plugin into `dir`, which must not exist or be empty.
 * @param {{ id: string, name: string, ui: boolean, backend: boolean, dir: string, sdk?: string }} options
 * `sdk`: a Caskade checkout to link the unpublished SDK packages from.
 * @returns {string[]} the files written, relative to `dir`
 */
export function generate(options) {
  const { id, name, ui, backend } = options;
  if (!validId(id))
    throw new Error(`"${id}" is not a plugin id: use reverse-DNS such as dev.example.hello`);
  if (!name?.trim() || name.length > 128) throw new Error("the name must be 1–128 characters");
  if (!ui && !backend) throw new Error("a plugin needs a UI half, a backend half or both");
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
    ".gitignore": "dist/\nnode_modules/\nbackend/target/\n",
    // The same extractor as the example plugins' (#219).
    "scripts/extract-messages.mjs": readFileSync(
      besideThis("templates/extract-messages.mjs"),
      "utf8",
    ),
    ...(ui && uiFiles(settings)),
    ...(backend && backendFiles(settings)),
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
