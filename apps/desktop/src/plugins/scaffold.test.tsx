// #130: the create-plugin templates build with @caskade/vite-plugin and
// render through the app's loader (the Rust side builds and loads backends,
// crates/caskade-plugin-host/tests/scaffold.rs).
import { caskadePlugin } from "@caskade/vite-plugin";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { build, type Rolldown } from "vite";
import { afterEach, expect, it } from "vitest";
import { generate } from "../../../../packages/create-plugin/generate.mjs";
import {
  joinPath as join,
  listFolder,
  readText,
  removeFolder,
  tempFolder,
} from "../../../../packages/create-plugin/testing.mjs";
import { PluginLoader } from "./loader";
import { PluginSlot } from "./PluginSlot";
import { PluginRegistry, PluginRegistryContext } from "./registry";
import { exposeSharedModules, sharedModules } from "./shared";
import { appExports, importMap, shimSource } from "./sharedExports";
import type { PluginUiApi, UiPlugin } from "./types";

const folders: string[] = [];
afterEach(() => {
  cleanup();
  for (const folder of folders.splice(0)) removeFolder(folder);
});

const dataUrl = (source: string) =>
  `data:text/javascript;base64,${btoa(unescape(encodeURIComponent(source)))}`;

function scaffold(ui: boolean, backend: boolean) {
  const dir = tempFolder();
  folders.push(dir);
  const files = generate({ id: "dev.example.hello", name: "Hello", ui, backend, dir });
  return { dir, files };
}

/** Builds the template's UI half as its vite.config.ts does. */
async function buildUi(dir: string) {
  expect(readText(dir, "vite.config.ts")).toContain("defineConfig({ plugins: [caskadePlugin()] })");
  const output = (await build({
    root: dir,
    configFile: false,
    logLevel: "silent",
    plugins: [caskadePlugin()],
    build: { write: false },
  })) as Rolldown.RolldownOutput | Rolldown.RolldownOutput[];
  const files = (Array.isArray(output) ? output : [output]).flatMap((result) => result.output);
  const ui = files.find((file) => file.fileName === "ui.js");
  if (ui?.type !== "chunk") throw new Error("no ui.js");
  expect(files.map((file) => file.fileName).sort()).toEqual(["ui.css", "ui.js"]);
  return ui.code;
}

/** Loads built code through the import-map shims and renders its view. */
async function show(code: string, api: PluginUiApi) {
  exposeSharedModules();
  const shims = Object.fromEntries(
    Object.keys(importMap().imports).map((specifier) => [
      specifier,
      dataUrl(
        shimSource(specifier, appExports[specifier] ?? Object.keys(sharedModules[specifier] ?? {})),
      ),
    ]),
  );
  const mapped = code.replace(/from "([^"]+)"/g, (whole, specifier: string) =>
    shims[specifier] ? `from "${shims[specifier]}"` : whole,
  );
  const registry = new PluginRegistry();
  await new PluginLoader(api, registry, () => import(/* @vite-ignore */ dataUrl(mapped))).loadAll();
  expect(registry.listNotices()).toEqual([]);
  const { QueryClient, QueryClientProvider } = await import("@tanstack/react-query");
  render(
    <QueryClientProvider client={new QueryClient()}>
      <PluginRegistryContext.Provider value={registry}>
        <PluginSlot slot="views">{({ contribution: { component: View } }) => <View />}</PluginSlot>
      </PluginRegistryContext.Provider>
    </QueryClientProvider>,
  );
}

function api(plugin: UiPlugin, requests: unknown[]): PluginUiApi {
  return {
    list: async () => [plugin],
    installed: async () => [],
    request: async (_id, method, payload) => {
      requests.push({ method, payload });
      return { message: "Hello, Caskade!" };
    },
    subscribe: async () => () => {},
    settings: {
      get: async () => ({ revision: 0, value: {} }),
      set: async (_id, value, revision) => ({ revision: revision + 1, value }),
    },
  };
}

const plugin: UiPlugin = {
  id: "dev.example.hello",
  name: "Hello",
  version: "0.1.0",
  entry: "caskade-plugin://dev.example.hello/dist/ui.js",
  contributes: [{ slot: "views", id: "main", title: "Hello" }],
};

it("the UI template builds and renders its view with the app's React", async () => {
  const { dir, files } = scaffold(true, false);
  expect(files).not.toContain("backend/Cargo.toml");
  const manifest = JSON.parse(readText(dir, "plugin.json"));
  expect(manifest).toMatchObject({ ui: "dist/ui.js", permissions: {} });
  expect(manifest.backend).toBeUndefined();
  const code = await buildUi(dir);
  expect(code).not.toMatch(/function useState|react\.production/);
  await show(code, api(plugin, []));
  await screen.findByText("Hello from Hello!");
  fireEvent.click(screen.getByRole("button", { name: "Clicked 0 times" }));
  await screen.findByRole("button", { name: "Clicked 1 times" });
});

it("the UI + backend template's view asks its backend", async () => {
  const { dir } = scaffold(true, true);
  const requests: unknown[] = [];
  await show(await buildUi(dir), api(plugin, requests));
  await screen.findByText("Hello, Caskade!");
  expect(requests).toEqual([{ method: "hello", payload: { name: "Caskade" } }]);
});

it("the backend template has no UI and refuses a non-empty folder", () => {
  const { dir, files } = scaffold(false, true);
  expect(files.sort()).toEqual(
    [
      ".gitignore",
      "README.md",
      "backend/Cargo.toml",
      "backend/src/lib.rs",
      "package.json",
      "plugin.json",
      "scripts/build-backend.mjs",
      "scripts/extract-messages.mjs",
    ].sort(),
  );
  // An empty locales/ for its translations (#219).
  expect(listFolder(dir, "locales")).toEqual([]);
  const manifest = JSON.parse(readText(dir, "plugin.json"));
  expect(manifest.activation).toEqual(["onEvent:game.exited"]);
  expect(() =>
    generate({ id: "dev.example.hello", name: "Hello", ui: true, backend: false, dir }),
  ).toThrow(/not empty/);
  expect(() =>
    generate({ id: "Hello", name: "Hello", ui: true, backend: false, dir: join(dir, "x") }),
  ).toThrow(/reverse-DNS/);
});
