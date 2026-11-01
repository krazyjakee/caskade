// #133: the example plugins' UI halves (plugins/examples/), built with
// @caskade/vite-plugin and rendered through the loader with the app's React.
// Their backends are tested in crates/caskade-plugin-host/tests/examples.rs.
import {
  type GameQuery,
  type GamesQuery,
  HostScope,
  type HostServices,
  type PluginGame,
  type PluginMessages,
} from "@caskade/sdk";
import { caskadePlugin } from "@caskade/vite-plugin";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { build, type Rolldown } from "vite";
import { afterEach, expect, it } from "vitest";
import { readText } from "../../../../packages/create-plugin/testing.mjs";
import { fakeLayout, fakeViewport, renderApp } from "../test/harness";
import { PluginGameDetails } from "./GameSlots";
import { PluginLoader } from "./loader";
import { PluginSlot } from "./PluginSlot";
import { PluginRegistry, PluginRegistryContext } from "./registry";
import { exposeSharedModules, sharedModules } from "./shared";
import { appExports, importMap, shimSource } from "./sharedExports";
import type { PluginEvent, PluginUiApi, UiPlugin } from "./types";

afterEach(cleanup);

const examples = new URL("../../../../plugins/examples/", import.meta.url).pathname;
const dataUrl = (source: string) =>
  `data:text/javascript;base64,${btoa(unescape(encodeURIComponent(source)))}`;

async function load(example: string) {
  const output = (await build({
    root: `${examples.replace(/\/$/, "")}/${example}`,
    configFile: false,
    logLevel: "silent",
    plugins: [caskadePlugin()],
    build: { write: false },
  })) as Rolldown.RolldownOutput | Rolldown.RolldownOutput[];
  const ui = (Array.isArray(output) ? output : [output])
    .flatMap((result) => result.output)
    .find((file) => file.fileName === "ui.js");
  if (ui?.type !== "chunk") throw new Error("no ui.js");
  exposeSharedModules();
  const shims = Object.fromEntries(
    Object.keys(importMap().imports).map((specifier) => [
      specifier,
      dataUrl(
        shimSource(specifier, appExports[specifier] ?? Object.keys(sharedModules[specifier] ?? {})),
      ),
    ]),
  );
  return ui.code.replace(/from "([^"]+)"/g, (whole, specifier: string) =>
    shims[specifier] ? `from "${shims[specifier]}"` : whole,
  );
}

const game = (id: string, title: string, hours: number): PluginGame => ({
  id,
  title,
  platform: "PC",
  status: "installed",
  favorite: false,
  playtimeSeconds: hours * 3600,
  lastPlayedAt: null,
  description: "",
  tags: [],
});
const library = [game("g1", "Celeste", 40), game("g2", "Hades", 12.5), game("g3", "Unplayed", 0)];

function fixture(plugin: UiPlugin, answer: unknown) {
  const listeners = new Set<(event: PluginEvent) => void>();
  const requests: unknown[] = [];
  const api: PluginUiApi = {
    list: async () => [plugin],
    installed: async () => [],
    request: async (_id, method, payload) => {
      requests.push({ method, payload });
      return answer;
    },
    subscribe: async (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    settings: {
      get: async () => ({ revision: 0, value: {} }),
      set: async (_id, value, revision) => ({ revision: revision + 1, value }),
    },
  };
  const emit = (event: PluginEvent) => {
    for (const listener of listeners) listener(event);
  };
  return { api, requests, emit };
}

const host: HostServices = {
  useGame: (id): GameQuery => ({
    data: library.find((item) => item.id === id) ?? null,
    isPending: false,
    error: null,
  }),
  useGames: (options): GamesQuery => ({
    data: [...library]
      .sort((a, b) => b.playtimeSeconds - a.playtimeSeconds)
      .slice(0, options?.limit ?? 100),
    isPending: false,
    error: null,
  }),
};

async function show(example: string, api: PluginUiApi, slot: "views" | "game.details.tabs") {
  const code = await load(example);
  const registry = new PluginRegistry();
  const loader = new PluginLoader(
    api,
    registry,
    () => import(/* @vite-ignore */ dataUrl(code)),
    document,
    async (id) => library.find((item) => item.id === id) ?? null,
  );
  await loader.loadAll();
  expect(registry.listNotices()).toEqual([]);
  render(
    <QueryClientProvider client={new QueryClient()}>
      <HostScope.Provider value={host}>
        <PluginRegistryContext.Provider value={registry}>
          <PluginSlot slot={slot}>
            {({ contribution }) => {
              const Component = (contribution as { component: (props: { gameId: string }) => null })
                .component;
              return <Component gameId="g1" />;
            }}
          </PluginSlot>
        </PluginRegistryContext.Provider>
      </HostScope.Provider>
    </QueryClientProvider>,
  );
  return registry;
}

it("playtime-stats charts the most played games and notes finished sessions", async () => {
  const plugin: UiPlugin = {
    id: "dev.caskade.playtime-stats",
    name: "Playtime stats",
    version: "0.1.0",
    entry: "caskade-plugin://dev.caskade.playtime-stats/dist/ui.js",
    contributes: [{ slot: "views", id: "stats", title: "Playtime stats" }],
  };
  const { api, emit } = fixture(plugin, null);
  const registry = await show("playtime-stats", api, "views");
  const bars = await screen.findByRole("list", { name: "Most played games, hours played" });
  const rows = within(bars).getAllByRole("listitem");
  expect(rows.map((row) => row.textContent)).toEqual(["Celeste40 h", "Hades13 h"]);
  expect(screen.getByText("53 h")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Table" }));
  expect(screen.getByRole("table", { name: "Most played games" })).toBeTruthy();

  await act(async () => {
    emit({ name: "game.exited", payload: { gameId: "g2", durationMs: 25 * 60_000 } });
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  expect(registry.listNotices().at(-1)?.message).toBe("Played Hades for 25m");
});

it("playtime-stats works in console mode by pad alone (#152)", async () => {
  const plugin: UiPlugin = {
    id: "dev.caskade.playtime-stats",
    name: "Playtime stats",
    version: "0.1.0",
    entry: "caskade-plugin://dev.caskade.playtime-stats/dist/ui.js",
    contributes: [
      { slot: "views", id: "stats", title: "Playtime stats", modes: ["desktop", "console"] },
    ],
  };
  const { api } = fixture(plugin, null);
  const registry = new PluginRegistry();
  const code = await load("playtime-stats");
  await new PluginLoader(
    api,
    registry,
    () => import(/* @vite-ignore */ dataUrl(code)),
    document,
  ).loadAll();
  expect(registry.listNotices()).toEqual([]);
  const undoLayout = fakeLayout();
  fakeViewport();
  try {
    const app = renderApp("/", { mode: "console", registry });
    // In the console sidebar; confirm opens it.
    const nav = await screen.findByRole("navigation", { name: "Plugins" });
    const link = within(nav).getByRole("link", { name: "Playtime stats" });
    act(() => link.focus());
    app.pad.press("confirm");
    await screen.findByRole("list", { name: "Most played games, hours played" });
    // Its LB/RB binding, through the SDK, switches chart and table.
    app.pad.press("tab.next");
    await screen.findByRole("table", { name: "Most played games" });
    expect(document.querySelector('[data-cs="hint-bar"]')?.textContent).toContain("Chart or table");
    app.pad.press("tab.prev");
    await screen.findByRole("list", { name: "Most played games, hours played" });
    // Its own buttons are reachable and pressable.
    for (let i = 0; i < 40 && document.activeElement?.textContent !== "Table"; i++) {
      const before = document.activeElement;
      app.pad.press("nav.down");
      await waitFor(() => expect(document.activeElement).not.toBe(before));
    }
    expect(document.activeElement?.textContent).toBe("Table");
    app.pad.press("confirm");
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Table" }).getAttribute("aria-pressed")).toBe(
        "true",
      ),
    );
  } finally {
    undoLayout();
  }
});

it("howlongtobeat shows the backend's times for the game's title", async () => {
  const plugin: UiPlugin = {
    id: "dev.caskade.howlongtobeat",
    name: "HowLongToBeat",
    version: "0.1.0",
    entry: "caskade-plugin://dev.caskade.howlongtobeat/dist/ui.js",
    contributes: [{ slot: "game.details.tabs", id: "hltb", title: "How long to beat" }],
  };
  const { api, requests } = fixture(plugin, {
    title: "Celeste",
    mainHours: 8,
    extrasHours: 11.5,
    completionistHours: null,
  });
  await show("howlongtobeat", api, "game.details.tabs");
  const times = await screen.findByRole("region", { name: "How long to beat Celeste" });
  expect(within(times).getByText("8 h")).toBeTruthy();
  expect(within(times).getByText("11.5 h")).toBeTruthy();
  expect(within(times).getByText("—")).toBeTruthy();
  expect(requests).toEqual([{ method: "lookup", payload: { title: "Celeste" } }]);
});

it("a crashing plugin shows placeholders while the others keep rendering", async () => {
  const stats: UiPlugin = {
    id: "dev.caskade.playtime-stats",
    name: "Playtime stats",
    version: "0.1.0",
    entry: "playtime-stats",
    contributes: [{ slot: "views", id: "stats", title: "Playtime stats" }],
  };
  const crashy: UiPlugin = {
    id: "dev.caskade.crashy",
    name: "Crashy",
    version: "0.1.0",
    entry: "crashy",
    contributes: [
      { slot: "views", id: "broken", title: "Crashy" },
      { slot: "home.sections", id: "broken", title: "Crashy" },
      { slot: "game.details.panels", id: "broken" },
    ],
  };
  const code = { "playtime-stats": await load("playtime-stats"), crashy: await load("crashy") };
  const { api } = fixture(stats, null);
  api.list = async () => [crashy, stats];
  const registry = new PluginRegistry();
  await new PluginLoader(
    api,
    registry,
    (entry) => import(/* @vite-ignore */ dataUrl(code[entry as keyof typeof code])),
  ).loadAll();
  expect(registry.listNotices()).toEqual([]);
  const errors = console.error;
  console.error = () => {};
  try {
    render(
      <QueryClientProvider client={new QueryClient()}>
        <HostScope.Provider value={host}>
          <PluginRegistryContext.Provider value={registry}>
            {(["views", "home.sections", "game.details.panels"] as const).map((slot) => (
              <PluginSlot key={slot} slot={slot}>
                {({ contribution }) => {
                  const Component = (
                    contribution as { component: (props: { gameId: string }) => null }
                  ).component;
                  return <Component gameId="g1" />;
                }}
              </PluginSlot>
            ))}
          </PluginRegistryContext.Provider>
        </HostScope.Provider>
      </QueryClientProvider>,
    );
    await screen.findByRole("list", { name: "Most played games, hours played" });
    const placeholders = screen
      .getAllByRole("alert")
      .filter((element) => element.dataset.plugin === "dev.caskade.crashy");
    expect(placeholders).toHaveLength(3);
    expect(placeholders[0]?.textContent).toBe("Crashy couldn't show this part.");
  } finally {
    console.error = errors;
  }
});

/** caskade-pack's Swedish catalog as the host sends it (`plugin_messages`). */
function swedishPack(): PluginMessages {
  // Vitest serves this file from `/@fs/<path>`; read it from the disk.
  const po = readText(examples.replace(/^\/@fs(?=\/)/, ""), "caskade-pack/locales/sv.po");
  const messages = [...po.matchAll(/^msgid (".+")\nmsgstr (".+")$/gm)].map((match) => ({
    context: null,
    id: JSON.parse(match[1] ?? '""') as string,
    translations: [JSON.parse(match[2] ?? '""') as string],
  }));
  return {
    locale: "sv",
    messages,
    plurals: Array.from({ length: 200 }, (_, n) => (n === 1 ? 0 : 1)),
  };
}

it("caskade-pack's game action shows in Swedish and follows a language switch (#219)", async () => {
  const plugin: UiPlugin = {
    id: "dev.caskade.example.pack",
    name: "Caskade-paket (exempel)",
    version: "0.2.0",
    entry: "caskade-plugin://dev.caskade.example.pack/dist/ui.js",
    contributes: [{ slot: "game.actions", id: "about-packs", title: "Om spelpaket" }],
  };
  const { api } = fixture(plugin, null);
  let catalog: PluginMessages | null = swedishPack();
  api.messages = async () => catalog ?? { locale: "en", messages: [], plurals: [] };
  const code = await load("caskade-pack");
  const registry = new PluginRegistry();
  const loader = new PluginLoader(api, registry, () => import(/* @vite-ignore */ dataUrl(code)));
  await loader.loadAll();
  expect(registry.listNotices()).toEqual([]);
  render(
    <QueryClientProvider client={new QueryClient()}>
      <HostScope.Provider value={host}>
        <PluginRegistryContext.Provider value={registry}>
          <PluginGameDetails gameId="g1" />
        </PluginRegistryContext.Provider>
      </HostScope.Provider>
    </QueryClientProvider>,
  );
  fireEvent.click(await screen.findByRole("button", { name: "Om spelpaket" }));
  await waitFor(() =>
    expect(registry.listNotices().at(-1)?.message).toBe(
      "Caskade installerar .cpk-spelpaket: släpp ett på biblioteket, eller klistra in en länk till dess sida på games.example.com.",
    ),
  );

  // The language changed: the app activates the plugin again with its new
  // catalog (English here: no messages), and the action follows.
  catalog = null;
  await act(async () => {
    await loader.reloadAll();
  });
  await screen.findByRole("button", { name: "About game packs" });
  expect(screen.queryByRole("button", { name: "Om spelpaket" })).toBeNull();
});

it("a plugin whose catalog can't be fetched shows English (#219)", async () => {
  const plugin: UiPlugin = {
    id: "dev.caskade.example.pack",
    name: "Caskade Pack (example)",
    version: "0.2.0",
    entry: "caskade-plugin://dev.caskade.example.pack/dist/ui.js",
    contributes: [{ slot: "game.actions", id: "about-packs", title: "About game packs" }],
  };
  const { api } = fixture(plugin, null);
  api.messages = async () => {
    throw new Error("no catalog");
  };
  const code = await load("caskade-pack");
  const registry = new PluginRegistry();
  const errors = console.error;
  console.error = () => {};
  try {
    await new PluginLoader(api, registry, () => import(/* @vite-ignore */ dataUrl(code))).loadAll();
  } finally {
    console.error = errors;
  }
  expect(registry.listNotices()).toEqual([]);
  expect(registry.list("game.actions").map((item) => item.contribution.title)).toEqual([
    "About game packs",
  ]);
});
