import { caskadePlugin } from "@caskade/vite-plugin";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { build, type Rolldown } from "vite";
import { afterEach, expect, it } from "vitest";
import { PluginLoader } from "./loader";
import { PluginSlot } from "./PluginSlot";
import { PluginRegistry, PluginRegistryContext } from "./registry";
import { exposeSharedModules, sharedModules } from "./shared";
import { appExports, importMap, shimSource } from "./sharedExports";
import type { PluginUiApi, UiPlugin } from "./types";

afterEach(cleanup);

const fixture = new URL("../../../../packages/vite-plugin/fixture/", import.meta.url).pathname;
const importUrl = (url: string): Promise<{ default?: unknown }> => import(/* @vite-ignore */ url);
const dataUrl = (source: string) =>
  `data:text/javascript;base64,${btoa(unescape(encodeURIComponent(source)))}`;

async function buildFixture() {
  const output = (await build({
    root: fixture,
    configFile: false,
    logLevel: "silent",
    plugins: [caskadePlugin()],
    build: { write: false },
  })) as Rolldown.RolldownOutput | Rolldown.RolldownOutput[];
  const files = (Array.isArray(output) ? output : [output]).flatMap((result) => result.output);
  const ui = files.find((file) => file.fileName === "ui.js");
  const css = files.find((file) => file.fileName === "ui.css");
  if (ui?.type !== "chunk" || css?.type !== "asset") throw new Error("missing ui.js or ui.css");
  return { code: ui.code, css: String(css.source), files: files.map((file) => file.fileName) };
}

it("builds a UI plugin without React and loads it through the import map", async () => {
  const { code, css, files } = await buildFixture();
  expect(files.sort()).toEqual(["ui.css", "ui.js"]);
  // No copy of React or the SDK: only bare imports the import map resolves.
  expect(code.length).toBeLessThan(4000);
  expect(code).not.toMatch(/react\.production|__CLIENT_INTERNALS|function useState/);
  for (const specifier of ["react", "react/jsx-runtime", "@caskade/sdk", "@caskade/ui"])
    expect(code).toContain(`from "${specifier}"`);
  expect(css.startsWith("@layer cs.plugins {")).toBe(true);

  // The shell's import map points each specifier at a shim reading the app's
  // own module; here the shims are data: modules and the map is applied to
  // the module's imports.
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
  const plugin: UiPlugin = {
    id: "dev.example.counter",
    name: "Counter",
    version: "0.1.0",
    entry: "caskade-plugin://dev.example.counter/dist/ui.js",
    contributes: [{ slot: "home.sections", id: "counter", title: "Counter" }],
  };
  const api: PluginUiApi = {
    list: async () => [plugin],
    installed: async () => [],
    request: async () => null,
    subscribe: async () => () => {},
    settings: {
      get: async () => ({ revision: 0, value: {} }),
      set: async (_id, value, revision) => ({ revision: revision + 1, value }),
    },
  };
  const registry = new PluginRegistry();
  await new PluginLoader(api, registry, () => importUrl(dataUrl(mapped))).loadAll();
  expect(registry.listNotices()).toEqual([]);
  render(
    <PluginRegistryContext.Provider value={registry}>
      <PluginSlot slot="home.sections">{({ contribution: { component: C } }) => <C />}</PluginSlot>
    </PluginRegistryContext.Provider>,
  );
  // Hooks only work if the plugin uses the app's React instance.
  fireEvent.click(await screen.findByRole("button", { name: "dev.example.counter clicked 0" }));
  await screen.findByRole("button", { name: "dev.example.counter clicked 1" });
  // A real Vite build: over 5 s in a loaded full run.
}, 60_000);
