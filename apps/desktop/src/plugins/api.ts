import type { PluginUiApi } from "./types";

/** No plugins: the browser preview, and native builds until the host lists some. */
export const noPlugins: PluginUiApi = {
  async list() {
    return [];
  },
  async request(pluginId) {
    throw new Error(`Plugin ${pluginId} has no backend here`);
  },
  async subscribe() {
    return () => {};
  },
  async installed() {
    return [];
  },
  settings: {
    async get(pluginId) {
      throw new Error(`Plugin ${pluginId} has no settings here`);
    },
    async set(pluginId) {
      throw new Error(`Plugin ${pluginId} has no settings here`);
    },
  },
};
