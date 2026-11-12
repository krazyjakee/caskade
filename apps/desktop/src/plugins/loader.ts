import {
  createPluginI18n,
  type PluginContext,
  type PluginDefinition,
  type PluginGame,
  type PluginI18n,
} from "@caskade/sdk";
import { t } from "@lingui/core/macro";
import { errorMessage } from "../api/reply";
import { effectSubscription } from "../api/subscription";
import { drawnDefinition } from "./drawn";
import type { PluginRegistry } from "./registry";
import { pluginSettingsNamespace } from "./settingsNamespace";
import type { PluginEventName, PluginUiApi, UiPlugin } from "./types";

type ImportModule = (url: string) => Promise<{ default?: unknown }>;
interface Loaded {
  plugin: UiPlugin;
  definition?: PluginDefinition;
  dispose: (() => void)[];
}

const importUrl: ImportModule = (url) => import(/* @vite-ignore */ url);

/** Loads plugins' UI halves and keeps them registered; one failing plugin
 * is reported and skipped. */
export class PluginLoader {
  private active = new Map<string, Loaded>();
  private reloads = 0;

  constructor(
    private api: PluginUiApi,
    private registry: PluginRegistry,
    private importModule: ImportModule = importUrl,
    private document: Document | undefined = globalThis.document,
    /** `ctx.library.game`: a library game by id. */
    private findGame: (id: string) => Promise<PluginGame | null> = async () => null,
  ) {}

  /** Activates every enabled plugin with a UI half. */
  async loadAll() {
    const plugins = await this.api.list();
    await Promise.all(plugins.map((plugin) => this.load(plugin)));
  }

  async load(plugin: UiPlugin) {
    await this.unload(plugin.id);
    const loaded: Loaded = { plugin, dispose: [] };
    this.active.set(plugin.id, loaded);
    try {
      if (plugin.style) loaded.dispose.push(this.style(plugin));
      // A 0.2 plugin's backend draws its UI (#109): nothing to import.
      const [module, i18n] = await Promise.all([
        plugin.drawn
          ? { default: drawnDefinition(plugin, this.api) }
          : this.importModule(plugin.entry),
        this.catalog(plugin),
      ]);
      const definition = module.default as PluginDefinition | undefined;
      if (typeof definition?.activate !== "function")
        throw new Error(t`its UI module has no definePlugin() default export`);
      loaded.definition = definition;
      await definition.activate(this.context(plugin, loaded.dispose, i18n));
    } catch (error) {
      await this.unload(plugin.id);
      console.error(`Plugin ${plugin.id} failed to activate`, error);
      this.registry.notify(
        plugin,
        t`${{ plugin: plugin.name }} couldn't start: ${{
          error: errorMessage(error),
        }}`,
        "error",
      );
    }
  }

  /** The plugin's own translations (#219); English when it has none or
   * they can't be fetched. */
  private async catalog(plugin: UiPlugin): Promise<PluginI18n> {
    try {
      return createPluginI18n(await this.api.messages?.(plugin.id));
    } catch (error) {
      console.error(`Plugin ${plugin.id} translations unavailable`, error);
      return createPluginI18n(null);
    }
  }

  /** Activates every plugin again, e.g. after the language changed (#219):
   * names, titles and catalogs come again from the host, and what the
   * plugins register is translated anew. */
  async reloadAll() {
    const plugins = await this.api.list();
    const listed = new Set(plugins.map((plugin) => plugin.id));
    await Promise.all(
      [...this.active.keys()].filter((id) => !listed.has(id)).map((id) => this.unload(id)),
    );
    await Promise.all(plugins.map((plugin) => this.load(plugin)));
  }

  /** Loads newly enabled plugins, unloads ones that went away, and
   * imports `reload` again under a fresh URL (dev mode, #131). */
  async refresh(reload?: string) {
    const plugins = await this.api.list();
    const listed = new Set(plugins.map((plugin) => plugin.id));
    await Promise.all(
      [...this.active.keys()].filter((id) => !listed.has(id)).map((id) => this.unload(id)),
    );
    const version = ++this.reloads;
    const fresh = (url: string) => `${url}${url.includes("?") ? "&" : "?"}v=${version}`;
    await Promise.all(
      plugins
        .filter((plugin) => plugin.id === reload || !this.active.has(plugin.id))
        .map((plugin) =>
          this.load(
            plugin.id === reload
              ? {
                  ...plugin,
                  entry: fresh(plugin.entry),
                  style: plugin.style && fresh(plugin.style),
                }
              : plugin,
          ),
        ),
    );
  }

  /** A loaded plugin's name. */
  name(pluginId: string): string | undefined {
    return this.active.get(pluginId)?.plugin.name;
  }

  async unloadAll() {
    await Promise.all([...this.active.keys()].map((id) => this.unload(id)));
  }

  async unload(pluginId: string) {
    const loaded = this.active.get(pluginId);
    this.active.delete(pluginId);
    this.registry.unregisterPlugin(pluginId);
    if (!loaded) return;
    for (const undo of loaded.dispose) undo();
    try {
      await loaded.definition?.deactivate?.();
    } catch (error) {
      console.error(`Plugin ${pluginId} failed to deactivate`, error);
    }
  }

  private context(plugin: UiPlugin, dispose: (() => void)[], i18n: PluginI18n): PluginContext {
    const context: PluginContext = {
      plugin: { id: plugin.id, name: plugin.name, version: plugin.version },
      slots: {
        register: (slot, contribution) => {
          const undo = this.registry.register(plugin, slot, contribution, context);
          dispose.push(undo);
          return undo;
        },
      },
      events: {
        on: (name: PluginEventName, handler) => {
          const off = effectSubscription(this.api.subscribe, (event) => {
            if (event.name !== name) return;
            // Only this plugin's own settings section.
            if (
              name === "settings.changed" &&
              (event.payload as { namespace?: string } | null)?.namespace !==
                pluginSettingsNamespace(plugin.id)
            )
              return;
            try {
              handler(event.payload);
            } catch (error) {
              console.error(`Plugin ${plugin.id} failed handling ${name}`, error);
            }
          });
          dispose.push(off);
          return off;
        },
      },
      backend: {
        call: async <T>(method: string, payload?: unknown) =>
          (await this.api.request(plugin.id, method, payload ?? null)) as T,
      },
      settings: {
        get: async <T>() => (await this.api.settings.get(plugin.id)).value as T,
        set: async <T>(value: T) => {
          // Writes merge with the latest revision, like the settings views.
          const latest = await this.api.settings.get(plugin.id);
          return (await this.api.settings.set(plugin.id, value, latest.revision)).value as T;
        },
        on: <T>(listener: (value: T) => void) =>
          context.events.on("settings.changed", () => {
            void this.api.settings
              .get(plugin.id)
              .then((snapshot) => listener(snapshot.value as T))
              .catch((error) => console.error(`Plugin ${plugin.id} settings unreadable`, error));
          }),
      },
      notify: (message) => this.registry.notify(plugin, message, "info"),
      i18n,
      library: { game: (id) => this.findGame(id) },
    };
    return context;
  }

  /** Plugin CSS goes into the `cs.plugins` cascade layer: `@caskade/vite-plugin`
   * wraps `ui.css` in that layer. A `<link>`, as themes use: WebKitGTK doesn't
   * load an `@import` from the `caskade-plugin:` scheme. */
  private style(plugin: UiPlugin): () => void {
    const element = this.document?.createElement("link");
    if (!element || !plugin.style) return () => {};
    element.rel = "stylesheet";
    element.dataset.plugin = plugin.id;
    element.href = plugin.style;
    this.document?.head.append(element);
    return () => element.remove();
  }
}
