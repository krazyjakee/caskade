import { isTauri } from "@tauri-apps/api/core";
import { commands, events, type Value } from "../api/bindings";
import { unwrap } from "../api/reply";
import { nativeSettingsApi } from "../api/settings";
import { noPlugins } from "./api";
import { pluginSettingsNamespace } from "./settingsNamespace";
import { type PluginSlot, type PluginUiApi, pluginSlots } from "./types";

/** Plugins through the typed IPC: the host lists UI halves, serves them
 * over `caskade-plugin://` and routes backend calls. */
const nativePlugins: PluginUiApi = {
  async list() {
    // 0.1 JavaScript halves, then 0.2 plugins their backends draw (#109).
    const [halves, drawn] = await Promise.all([
      unwrap(commands.pluginsUi()),
      unwrap(commands.pluginsDrawn()),
    ]);
    return [
      ...halves,
      ...drawn.map((plugin) => ({ ...plugin, entry: "", style: null, drawn: true })),
    ].map((plugin) => ({
      ...plugin,
      contributes: plugin.contributes.flatMap(({ slot, id, title, modes }) =>
        pluginSlots.includes(slot as PluginSlot)
          ? [{ slot: slot as PluginSlot, id, title, modes }]
          : [],
      ),
    }));
  },
  async render(pluginId, slot, id, context) {
    return unwrap(commands.pluginUiRender(pluginId, slot, id, context));
  },
  async act(pluginId, slot, id, action, value, context) {
    return unwrap(
      commands.pluginUiAct(pluginId, slot, id, action, (value ?? null) as Value, context),
    );
  },
  async installed() {
    return (await unwrap(commands.pluginsList())).map((plugin) => ({
      id: plugin.id,
      name: plugin.name,
      enabled: plugin.status === "enabled" || plugin.status === "active",
      hasSettings: plugin.contributes?.settings != null,
      settingsError: plugin.settingsError,
    }));
  },
  settings: {
    get: (pluginId) => nativeSettingsApi.get(pluginSettingsNamespace(pluginId)),
    set: (pluginId, value, revision) =>
      nativeSettingsApi.set(
        pluginSettingsNamespace(pluginId),
        value as Record<string, unknown>,
        revision,
      ),
  },
  async messages(pluginId) {
    return unwrap(commands.pluginMessages(pluginId));
  },
  async request(pluginId, method, payload) {
    return unwrap(commands.pluginRequest(pluginId, method, (payload ?? null) as Value));
  },
  async viewOpened(pluginId, viewId) {
    await unwrap(commands.pluginViewOpened(pluginId, viewId));
  },
  async subscribe(listener) {
    const stops = await Promise.all([
      events.gameStarted.listen(({ payload }) => listener({ name: "game.started", payload })),
      events.gameExited.listen(({ payload }) => listener({ name: "game.exited", payload })),
      events.libraryChanged.listen(({ payload }) => listener({ name: "library.changed", payload })),
      events.settingsChanged.listen(({ payload }) =>
        listener({ name: "settings.changed", payload }),
      ),
      events.pluginDisabled.listen(({ payload }) => listener({ name: "plugin.disabled", payload })),
      events.pluginsChanged.listen(({ payload }) => listener({ name: "plugins.changed", payload })),
      events.pluginReloaded.listen(({ payload }) => listener({ name: "plugin.reloaded", payload })),
      events.localeChanged.listen(({ payload }) => listener({ name: "locale.changed", payload })),
    ]);
    return () => {
      for (const stop of stops) stop();
    };
  },
};

export function connectPluginApi(): PluginUiApi {
  return isTauri() ? nativePlugins : noPlugins;
}
