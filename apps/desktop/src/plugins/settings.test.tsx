import { definePlugin, type PluginDefinition, useSettings } from "@caskade/sdk";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { SettingsApi, SettingsChanged, SettingsSnapshot } from "../api/settings";
import { SettingsProvider } from "../views/settings/useSettings";
import { PluginLoader } from "./loader";
import { PluginProvider } from "./PluginProvider";
import { PluginSettingsPages } from "./PluginViews";
import { PluginRegistry } from "./registry";
import type { InstalledPlugin, PluginEvent, PluginUiApi, UiPlugin } from "./types";

afterEach(cleanup);

const namespace = "plugins.dev.stats";
const schema = {
  type: "object",
  properties: {
    showHours: { type: "boolean", title: "Show hours", default: true },
    limit: { type: "integer", title: "Top games", minimum: 1, maximum: 10, default: 5 },
  },
  additionalProperties: false,
};

/** Core's settings service for one plugin section, validating like core. */
function backend(installed: InstalledPlugin[], contributes: UiPlugin["contributes"] = []) {
  let snapshot: SettingsSnapshot = {
    namespace,
    version: 1,
    revision: 0,
    value: { showHours: true, limit: 5 },
  };
  const settingsListeners = new Set<(event: SettingsChanged) => void>();
  const pluginListeners = new Set<(event: PluginEvent) => void>();
  const write = async (value: unknown, revision: number) => {
    const next = value as { showHours?: unknown; limit?: unknown };
    if (revision !== snapshot.revision) throw new Error("Settings changed; try again.");
    if (typeof next.limit !== "number" || next.limit < 1 || next.limit > 10)
      throw new Error(`invalid settings for ${namespace} at ["/limit"]`);
    snapshot = { ...snapshot, revision: snapshot.revision + 1, value: next };
    for (const listener of settingsListeners) listener({ namespace, version: 1 });
    for (const listener of pluginListeners)
      listener({ name: "settings.changed", payload: { namespace, revision: snapshot.revision } });
    return snapshot;
  };
  const settings: SettingsApi = {
    sections: async () => [
      { namespace, version: 1, schema, defaults: { showHours: true, limit: 5 } },
    ],
    get: async () => snapshot,
    set: vi.fn(async (_namespace, value, revision) => write(value, revision)),
    reset: async (_namespace, revision) => write({ showHours: true, limit: 5 }, revision),
    subscribe: async (listener) => {
      settingsListeners.add(listener);
      return () => settingsListeners.delete(listener);
    },
  };
  const plugins: PluginUiApi & { emit(event: PluginEvent): void } = {
    list: async () =>
      installed.map((plugin) => ({
        id: plugin.id,
        name: plugin.name,
        version: "1.0.0",
        entry: `caskade-plugin://${plugin.id}/ui.js`,
        contributes,
      })),
    installed: async () => installed,
    request: async () => null,
    subscribe: async (listener) => {
      pluginListeners.add(listener);
      return () => pluginListeners.delete(listener);
    },
    settings: {
      get: async () => snapshot,
      set: async (_id, value, revision) => write(value, revision),
    },
    emit: (event) => {
      for (const listener of pluginListeners) listener(event);
    },
  };
  return { settings, plugins, current: () => snapshot };
}

function mount(api: ReturnType<typeof backend>, definition: PluginDefinition) {
  render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <SettingsProvider api={api.settings}>
        <PluginProvider api={api.plugins} importModule={async () => ({ default: definition })}>
          <PluginSettingsPages />
        </PluginProvider>
      </SettingsProvider>
    </QueryClientProvider>,
  );
}
const stats: InstalledPlugin = {
  id: "dev.stats",
  name: "Playtime stats",
  enabled: true,
  hasSettings: true,
  settingsError: null,
};

it("renders a plugin's schema as its settings page and the UI half sees saved values", async () => {
  const api = backend([stats]);
  const seen: unknown[] = [];
  let read: unknown;
  mount(
    api,
    definePlugin({
      activate(ctx) {
        ctx.settings.on((value) => {
          seen.push(value);
          void ctx.settings.get().then((current) => {
            read = current;
          });
        });
      },
    }),
  );
  const page = await screen.findByRole("region", { name: "Playtime stats" });
  const toggle = await within(page).findByRole("switch", { name: "Show hours" });
  expect(within(page).getByRole("slider", { name: "Top games" })).toBeTruthy();
  fireEvent.click(toggle);
  await waitFor(() => expect(api.current().value).toEqual({ showHours: false, limit: 5 }));
  expect(api.settings.set).toHaveBeenCalledWith(namespace, { showHours: false, limit: 5 }, 0);
  await waitFor(() => expect(seen).toEqual([{ showHours: false, limit: 5 }]));
  await waitFor(() => expect(read).toEqual({ showHours: false, limit: 5 }));
});

/** A plugin's own settings page, using the SDK hook. */
function Page() {
  const { settings, setSettings, error } = useSettings<{ showHours: boolean; limit: number }>();
  return (
    <div>
      <p>Limit {settings?.limit ?? "…"}</p>
      <button type="button" onClick={() => settings && setSettings({ ...settings, limit: 8 })}>
        Eight
      </button>
      <button type="button" onClick={() => settings && setSettings({ ...settings, limit: 99 })}>
        Too many
      </button>
      {error && <p role="alert">{error.message}</p>}
    </div>
  );
}

it("uses the plugin's own settings page with useSettings(), and core validates its writes", async () => {
  const api = backend([stats], [{ slot: "settings.pages", id: "page", title: "Stats" }]);

  mount(
    api,
    definePlugin({
      activate(ctx) {
        ctx.slots.register("settings.pages", { id: "page", title: "Stats", component: Page });
      },
    }),
  );
  await screen.findByText("Limit 5");
  expect(screen.queryByRole("switch", { name: "Show hours" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Eight" }));
  await screen.findByText("Limit 8");
  fireEvent.click(screen.getByRole("button", { name: "Too many" }));
  expect((await screen.findByRole("alert")).textContent).toContain("/limit");
  expect(api.current().value).toEqual({ showHours: true, limit: 8 });
});

it("reports a refused schema without hiding the plugin, and skips disabled plugins", async () => {
  const api = backend([
    {
      ...stats,
      settingsError: "settings: settings registration: invalid or unresolved settings schema",
    },
    { ...stats, id: "dev.off", name: "Disabled plugin", enabled: false },
  ]);
  mount(api, definePlugin({ activate() {} }));
  const page = await screen.findByRole("region", { name: "Playtime stats" });
  expect(within(page).getByRole("alert").textContent).toContain("invalid or unresolved");
  expect(screen.queryByRole("region", { name: "Disabled plugin" })).toBeNull();
});

it("gives each settings page one Settings card, and shows nothing when none has settings", async () => {
  const schemaOnly = backend([stats]);
  mount(schemaOnly, definePlugin({ activate() {} }));
  await screen.findByRole("switch", { name: "Show hours" });
  const sections = screen.getAllByRole("region", { name: "Playtime stats" });
  expect(sections.map((section) => section.getAttribute("data-cs"))).toEqual(["settings-section"]);
  cleanup();

  const ownPage = backend([stats], [{ slot: "settings.pages", id: "page", title: "Stats" }]);
  mount(
    ownPage,
    definePlugin({
      activate(ctx) {
        ctx.slots.register("settings.pages", { id: "page", title: "Stats", component: Page });
      },
    }),
  );
  await screen.findByText("Limit 5");
  expect(screen.getByRole("region", { name: "Playtime stats" }).getAttribute("data-cs")).toBe(
    "settings-section",
  );
  cleanup();

  mount(backend([{ ...stats, hasSettings: false }]), definePlugin({ activate() {} }));
  await waitFor(() => expect(document.body.textContent).toBe(""));
  expect(screen.queryByRole("region")).toBeNull();
});

it("delivers settings.changed only for the plugin's own section", async () => {
  const api = backend([stats]);
  const heard: unknown[] = [];
  const loader = new PluginLoader(api.plugins, new PluginRegistry(), async () => ({
    default: definePlugin({
      activate(ctx) {
        ctx.events.on("settings.changed", (payload) => heard.push(payload));
      },
    }),
  }));
  await loader.loadAll();
  await waitFor(() => {
    api.plugins.emit({ name: "settings.changed", payload: { namespace: "plugins.dev.other" } });
    api.plugins.emit({ name: "settings.changed", payload: { namespace } });
    expect(heard.length).toBeGreaterThan(0);
  });
  expect(heard.every((payload) => (payload as { namespace: string }).namespace === namespace)).toBe(
    true,
  );
});
