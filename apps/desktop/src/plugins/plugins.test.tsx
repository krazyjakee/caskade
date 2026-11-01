import * as Sdk from "@caskade/sdk";
import { definePlugin, useBackendQuery } from "@caskade/sdk";
import * as Ui from "@caskade/ui";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PluginGameDetails } from "./GameSlots";
import { PluginLoader } from "./loader";
import { PluginProvider } from "./PluginProvider";
import { PluginNav, PluginNotices, PluginViewPage } from "./PluginViews";
import { PluginRegistry } from "./registry";
import { exposeSharedModules, sharedModules } from "./shared";
import { appExports, importMap, shimPath, shimSource } from "./sharedExports";
import type { PluginEvent, PluginUiApi, UiPlugin } from "./types";

afterEach(cleanup);

function plugin(id: string, contributes: UiPlugin["contributes"]): UiPlugin {
  return {
    id,
    name: id.toUpperCase(),
    version: "0.1.0",
    entry: `caskade-plugin://${id}/ui.js`,
    contributes,
  };
}

function fixtureApi(plugins: UiPlugin[]) {
  const listeners = new Set<(event: PluginEvent) => void>();
  const api: PluginUiApi & { emit(event: PluginEvent): void } = {
    list: vi.fn(async () => plugins),
    request: vi.fn(async (id: string, method: string, payload: unknown) => ({
      id,
      method,
      payload,
    })),
    async subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    emit: (event) => {
      for (const listener of listeners) listener(event);
    },
    installed: vi.fn(async () =>
      plugins.map((p) => ({
        id: p.id,
        name: p.name,
        enabled: true,
        hasSettings: false,
        settingsError: null,
      })),
    ),
    settings: {
      get: vi.fn(async () => ({ revision: 0, value: {} })),
      set: vi.fn(async (_id: string, value: unknown, revision: number) => ({
        revision: revision + 1,
        value,
      })),
    },
  };
  return api;
}

describe("registry", () => {
  it("accepts only declared contributions and keeps snapshots stable", () => {
    const registry = new PluginRegistry();
    const p = plugin("dev.a", [{ slot: "home.sections", id: "stats" }]);
    const before = registry.list("home.sections");
    expect(() =>
      registry.register(p, "home.sections", { id: "other", title: "x", component: () => null }),
    ).toThrow(/did not declare/);
    expect(registry.list("home.sections")).toBe(before);
    const undo = registry.register(p, "home.sections", {
      id: "stats",
      title: "Stats",
      component: () => null,
    });
    const after = registry.list("home.sections");
    expect(after).toHaveLength(1);
    expect(registry.list("home.sections")).toBe(after);
    undo();
    expect(registry.list("home.sections")).toHaveLength(0);
  });
});

describe("shared modules", () => {
  it("lists the app modules' runtime exports for the shims", () => {
    expect([...(appExports["@caskade/sdk"] ?? [])].sort()).toEqual(Object.keys(Sdk).sort());
    expect([...(appExports["@caskade/ui"] ?? [])].sort()).toEqual(Object.keys(Ui).sort());
    expect(importMap().imports.react).toBe("/plugin-shims/react.js");
    expect(shimPath("@caskade/sdk")).toBe("/plugin-shims/caskade-sdk.js");
  });

  it("shims resolve to the app's own React and SDK instances", async () => {
    exposeSharedModules();
    const load = (specifier: string, names: string[]) =>
      import(
        /* @vite-ignore */ `data:text/javascript;base64,${btoa(shimSource(specifier, names))}`
      );
    const react = await load("react", Object.keys(React));
    expect(react.useState).toBe(React.useState);
    expect(react.default).toBe((sharedModules.react as { default?: unknown }).default ?? React);
    const sdk = await load("@caskade/sdk", [...(appExports["@caskade/sdk"] ?? [])]);
    expect(sdk.definePlugin).toBe(Sdk.definePlugin);
  });
});

describe("loader", () => {
  it("activates plugins, isolates one that fails and unloads cleanly", async () => {
    const registry = new PluginRegistry();
    const good = plugin("dev.good", [{ slot: "home.sections", id: "stats" }]);
    const broken = plugin("dev.broken", [{ slot: "home.sections", id: "x" }]);
    const missing = plugin("dev.missing", []);
    const deactivate = vi.fn();
    const modules: Record<string, () => Promise<{ default?: unknown }>> = {
      [good.entry]: async () => ({
        default: definePlugin({
          activate(ctx) {
            ctx.slots.register("home.sections", {
              id: "stats",
              title: "Stats",
              component: () => null,
            });
          },
          deactivate,
        }),
      }),
      [broken.entry]: async () => ({
        default: definePlugin({
          activate() {
            throw new Error("boom");
          },
        }),
      }),
      [missing.entry]: async () => {
        throw new Error("404");
      },
    };
    const loader = new PluginLoader(fixtureApi([good, broken, missing]), registry, (url) =>
      (modules[url] ?? (() => Promise.reject(new Error(url))))(),
    );
    await loader.loadAll();
    expect(registry.list("home.sections").map((item) => item.plugin.id)).toEqual(["dev.good"]);
    expect(
      registry
        .listNotices()
        .map((notice) => notice.pluginId)
        .sort(),
    ).toEqual(["dev.broken", "dev.missing"]);
    await loader.unloadAll();
    expect(registry.list("home.sections")).toHaveLength(0);
    expect(deactivate).toHaveBeenCalledOnce();
  });

  it("routes backend calls and events to the plugin", async () => {
    const registry = new PluginRegistry();
    const p = plugin("dev.events", []);
    const api = fixtureApi([p]);
    const seen: unknown[] = [];
    let answer: unknown;
    const loader = new PluginLoader(api, registry, async () => ({
      default: definePlugin({
        async activate(ctx) {
          ctx.events.on("game.exited", (payload) => seen.push(payload));
          answer = await ctx.backend.call("lookup", { title: "Celeste" });
          ctx.notify("ready");
        },
      }),
    }));
    await loader.loadAll();
    expect(answer).toEqual({ id: "dev.events", method: "lookup", payload: { title: "Celeste" } });
    await waitFor(() => {
      api.emit({ name: "game.exited", payload: { gameId: "g1" } });
      expect(seen).toEqual([{ gameId: "g1" }]);
    });
    api.emit({ name: "game.started", payload: {} });
    expect(seen).toHaveLength(1);
    expect(registry.listNotices()[0]?.message).toBe("ready");
  });

  it("links a plugin's stylesheet while it is loaded", async () => {
    const styled = { ...plugin("dev.styled", []), style: "caskade-plugin://dev.styled/ui.css" };
    const loader = new PluginLoader(fixtureApi([styled]), new PluginRegistry(), async () => ({
      default: definePlugin({ activate() {} }),
    }));
    await loader.loadAll();
    // A <link>, not an @import: WebKitGTK doesn't load the latter from the plugin scheme.
    const link = document.head.querySelector<HTMLLinkElement>('link[data-plugin="dev.styled"]');
    expect(link?.rel).toBe("stylesheet");
    expect(link?.getAttribute("href")).toBe("caskade-plugin://dev.styled/ui.css");
    await loader.unloadAll();
    expect(document.head.querySelector('[data-plugin="dev.styled"]')).toBeNull();
  });
});

function HltbTab({ gameId }: { gameId: string }) {
  const [count] = React.useState(3);
  const times = useBackendQuery<{ payload: { gameId: string } }>("lookup", { gameId });
  return (
    <p>
      Main story {count}h for {times.data?.payload.gameId ?? "…"}
    </p>
  );
}
function Crash(): React.ReactNode {
  throw new Error("render failed");
}

it("renders a plugin view and game-details tab in the shell, and contains a crash", async () => {
  const hltb = plugin("dev.hltb", [
    { slot: "views", id: "stats", title: "Stats" },
    { slot: "game.details.tabs", id: "hltb", title: "How long to beat" },
    { slot: "game.details.panels", id: "crash" },
  ]);
  const api = fixtureApi([hltb]);
  const importModule = async () => ({
    default: definePlugin({
      activate(ctx) {
        ctx.slots.register("views", {
          id: "stats",
          title: "Stats",
          component: () => <p>Stats view</p>,
        });
        ctx.slots.register("game.details.tabs", {
          id: "hltb",
          title: "How long to beat",
          component: HltbTab,
        });
        ctx.slots.register("game.details.panels", { id: "crash", component: Crash });
      },
    }),
  });
  const root = createRootRoute({
    component: () => (
      <>
        <PluginNav />
        <Outlet />
        <PluginNotices />
      </>
    ),
  });
  const game = createRoute({
    getParentRoute: () => root,
    path: "/games/$gameId",
    component: () => <PluginGameDetails gameId="g1" />,
  });
  const view = createRoute({
    getParentRoute: () => root,
    path: "/plugins/$pluginId/$viewId",
    component: function View() {
      const { pluginId, viewId } = view.useParams();
      return <PluginViewPage pluginId={pluginId} viewId={viewId} />;
    },
  });
  const router = createRouter({
    routeTree: root.addChildren([game, view]),
    history: createMemoryHistory({ initialEntries: ["/games/g1"] }),
  });
  const { QueryClient, QueryClientProvider } = await import("@tanstack/react-query");
  vi.spyOn(console, "error").mockImplementation(() => {});
  render(
    <QueryClientProvider client={new QueryClient()}>
      <PluginProvider api={api} importModule={importModule}>
        <RouterProvider router={router} />
      </PluginProvider>
    </QueryClientProvider>,
  );
  await screen.findByRole("tab", { name: "How long to beat" });
  await screen.findByText("Main story 3h for g1");
  expect(api.request).toHaveBeenCalledWith("dev.hltb", "lookup", { gameId: "g1" });
  // The crashing panel shows its placeholder; the tab beside it still works.
  expect(screen.getByRole("alert").dataset.plugin).toBe("dev.hltb");
  expect(screen.getByRole("alert").textContent).toContain("DEV.HLTB couldn't show this part");

  fireEvent.click(screen.getByRole("link", { name: "Stats" }));
  await screen.findByText("Stats view");
  await act(async () => {
    await router.navigate({
      to: "/plugins/$pluginId/$viewId",
      params: { pluginId: "x", viewId: "y" },
    });
  });
  expect(screen.getByText("This plugin view isn't available.")).toBeTruthy();
});

it("unloads a plugin core disabled and reports why; opening a view reports onView", async () => {
  const crashy = plugin("dev.crashy", [
    { slot: "home.sections", id: "stats" },
    { slot: "views", id: "board", title: "Board" },
  ]);
  const api = fixtureApi([crashy]);
  const viewOpened = vi.fn(async () => {});
  api.viewOpened = viewOpened;
  const registry = new PluginRegistry();
  const importModule = async () => ({
    default: definePlugin({
      activate(ctx) {
        ctx.slots.register("home.sections", { id: "stats", title: "Stats", component: () => null });
        ctx.slots.register("views", {
          id: "board",
          title: "Board",
          component: () => <p>Board content</p>,
        });
      },
    }),
  });
  render(
    <PluginProvider api={api} registry={registry} importModule={importModule}>
      <PluginViewPage pluginId="dev.crashy" viewId="board" />
    </PluginProvider>,
  );
  await screen.findByText("Board content");
  expect(viewOpened).toHaveBeenCalledWith("dev.crashy", "board");
  await waitFor(() => expect(registry.list("home.sections")).toHaveLength(1));

  act(() =>
    api.emit({
      name: "plugin.disabled",
      payload: { plugin: "dev.crashy", reason: "stopped after 3 failures in a row: trap" },
    }),
  );
  await waitFor(() => expect(registry.list("home.sections")).toHaveLength(0));
  expect(registry.list("views")).toHaveLength(0);
  const notice = registry.listNotices().at(-1);
  expect(notice?.tone).toBe("error");
  expect(notice?.message).toBe(
    "DEV.CRASHY was turned off: stopped after 3 failures in a row: trap",
  );
  await screen.findByText("This plugin view isn't available.");
});

it("follows plugins.changed and re-imports a reloaded dev plugin under a fresh URL", async () => {
  const one = plugin("dev.one", [{ slot: "home.sections", id: "a" }]);
  const two = plugin("dev.two", [{ slot: "home.sections", id: "b" }]);
  let listed = [one];
  const api = fixtureApi([]);
  api.list = vi.fn(async () => listed);
  const registry = new PluginRegistry();
  const imported: string[] = [];
  const importModule = async (url: string) => {
    imported.push(url);
    const id = url.split("/")[2] ?? "";
    const version = url.includes("v=") ? "new" : "old";
    return {
      default: definePlugin({
        activate(ctx) {
          ctx.slots.register("home.sections", {
            id: id === "dev.one" ? "a" : "b",
            title: `${id} ${version}`,
            component: () => null,
          });
        },
      }),
    };
  };
  render(
    <PluginProvider api={api} registry={registry} importModule={importModule}>
      <p>shell</p>
    </PluginProvider>,
  );
  const titles = () => registry.list("home.sections").map((item) => item.contribution.title);
  await waitFor(() => expect(titles()).toEqual(["dev.one old"]));

  // Enabled elsewhere: loaded; disabled: unloaded.
  listed = [one, two];
  act(() => api.emit({ name: "plugins.changed", payload: { plugin: "dev.two" } }));
  await waitFor(() => expect(titles()).toEqual(["dev.one old", "dev.two old"]));
  listed = [two];
  act(() => api.emit({ name: "plugins.changed", payload: { plugin: "dev.one" } }));
  await waitFor(() => expect(titles()).toEqual(["dev.two old"]));

  // A dev rebuild replaces the old contributions.
  act(() =>
    api.emit({ name: "plugin.reloaded", payload: { plugin: "dev.two", ui: true, error: null } }),
  );
  await waitFor(() => expect(titles()).toEqual(["dev.two new"]));
  expect(imported.at(-1)).toBe("caskade-plugin://dev.two/ui.js?v=3");

  // A broken rebuild keeps what runs and says why.
  act(() =>
    api.emit({
      name: "plugin.reloaded",
      payload: { plugin: "dev.two", ui: false, error: "not a component" },
    }),
  );
  expect(titles()).toEqual(["dev.two new"]);
  expect(registry.listNotices().at(-1)?.message).toBe("DEV.TWO: rebuild failed: not a component");
});
