/** Explicit dev data only (`VITE_CASKADE_FIXTURES=1`): installed plugins in
 * memory, with synthetic manifests. Nothing is loaded or run. Never a
 * fallback for failed native IPC. */
import type { PluginManagerApi, PluginSummary } from "./pluginManager";

function plugin(
  id: string,
  name: string,
  description: string,
  status: PluginSummary["status"],
): PluginSummary {
  return {
    id,
    name,
    version: "1.0.0",
    description,
    author: "Caskade fixtures",
    folder: `/fixture/plugins/${id}`,
    status,
    errors: [],
    permissions: null,
    contributes: null,
    hasUi: true,
    hasBackend: false,
    failures: 0,
    lastError: null,
    settingsError: null,
    dev: false,
  };
}

export function createPluginManagerFixture(): PluginManagerApi {
  const plugins = [
    plugin("dev.fixture.stats", "Fixture Stats", "Charts of synthetic play sessions.", "enabled"),
    plugin("dev.fixture.notes", "Fixture Notes", "Notes kept in plugin storage.", "disabled"),
  ];
  const find = (id: string) => {
    const found = plugins.find((candidate) => candidate.id === id);
    if (!found) throw new Error(`Unknown plugin ${id}.`);
    return found;
  };
  return {
    list: async () => structuredClone(plugins),
    setEnabled: async (id, enabled) => {
      const found = find(id);
      found.status = enabled ? "enabled" : "disabled";
      return structuredClone(found);
    },
    chooseFolder: async () => null,
    preview: async () => {
      throw new Error("Installing plugins needs the Caskade core.");
    },
    install: async () => {
      throw new Error("Installing plugins needs the Caskade core.");
    },
    uninstall: async (id) => {
      plugins.splice(plugins.indexOf(find(id)), 1);
    },
  };
}
