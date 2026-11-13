/** Settings → Plugins: list, enable, review and install plugins (#123, #124). */
import { t } from "@lingui/core/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { commands, type PluginPreview_Serialize, type PluginSummary_Serialize } from "./bindings";
import { injectable } from "./injectable";
import { unwrap } from "./reply";

export type PluginSummary = PluginSummary_Serialize;
type PluginPreview = PluginPreview_Serialize;

export interface PluginManagerApi {
  list(): Promise<PluginSummary[]>;
  setEnabled(id: string, enabled: boolean): Promise<PluginSummary>;
  /** A folder the user picked, or null when they cancelled. */
  chooseFolder(): Promise<string | null>;
  /** Reads the folder's manifest; copies and runs nothing. */
  preview(folder: string): Promise<PluginPreview>;
  /** Installs a reviewed folder and grants what it asks for. */
  install(folder: string): Promise<PluginSummary>;
  /** Removes the plugin, its storage, settings and data (#128). */
  uninstall(id: string): Promise<void>;
  /** Loads a folder in place and reloads it on change (#131; debug builds). */
  loadUnpacked?(folder: string): Promise<void>;
}

function createNativePluginManager(): PluginManagerApi {
  const native = () => {
    if (!isTauri()) throw new Error(t`Open Caskade desktop to manage plugins.`);
  };
  return {
    async list() {
      native();
      return unwrap(commands.pluginsList());
    },
    async setEnabled(id, enabled) {
      native();
      return unwrap(enabled ? commands.pluginEnable(id) : commands.pluginDisable(id));
    },
    async chooseFolder() {
      native();
      const { open } = await import("@tauri-apps/plugin-dialog");
      const folder = await open({ directory: true, multiple: false, title: t`Plugin folder` });
      return typeof folder === "string" ? folder : null;
    },
    async preview(folder) {
      native();
      return unwrap(commands.pluginInstallPreview(folder));
    },
    async install(folder) {
      native();
      return unwrap(commands.pluginInstall(folder));
    },
    async uninstall(id) {
      native();
      await unwrap(commands.pluginUninstall(id));
    },
    async loadUnpacked(folder) {
      native();
      await unwrap(commands.pluginLoadUnpacked(folder));
    },
  };
}

export const {
  Provider: PluginManagerProvider,
  useApi: usePluginManager,
  preview: previewPluginManager,
} = injectable(createNativePluginManager);

/** Under `plugins`, so the UI-half loader refreshes after changes too. */
const managedPluginsKey = ["plugins", "managed"] as const;

export function useManagedPlugins() {
  const api = usePluginManager();
  return useQuery({ queryKey: managedPluginsKey, queryFn: () => api.list() });
}

export function useSetPluginEnabled() {
  const api = usePluginManager();
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) => api.setEnabled(id, enabled),
    onSuccess: () => client.invalidateQueries({ queryKey: ["plugins"] }),
  });
}
