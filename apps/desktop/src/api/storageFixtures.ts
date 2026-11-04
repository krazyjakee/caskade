/** Explicit dev data only (`VITE_CASKADE_FIXTURES=1`): two synthetic storage
 * roots in memory, with `core.storage` read from the settings fixture.
 * Never a fallback for failed native IPC. */
import { settingsFixtureApi } from "./settingsFixtures";
import type { RootSummary, StorageApi } from "./storage";

const GiB = 1024 ** 3;

function root(id: string, path: string, label: string | null, isDefault: boolean): RootSummary {
  return {
    root: { id, path, label, isDefault },
    status: "online",
    capabilities: {
      writable: true,
      executable: true,
      symlinks: true,
      caseSensitive: true,
      freeBytes: 412 * GiB,
      totalBytes: 931 * GiB,
      filesystem: "ext4",
      volume: label,
      warnings: [],
    },
    folderTemplate: "{title}",
    games: [],
    gamesBytes: 0,
    otherBytes: 120 * GiB,
  };
}

export function createStorageFixtureApi(): StorageApi {
  const roots: RootSummary[] = [
    root("root-1", "/fixture/Games", "Games", true),
    root("root-2", "/fixture/External/Games", "External", false),
  ];
  roots[0]?.games.push({
    entryId: "entry-1",
    gameId: "fixture-1",
    title: "Fixture Game 0002",
    relativeDir: "Fixture Game 0002",
    sizeBytes: GiB,
    available: true,
    moveBlocked: null,
  });
  for (const summary of roots) {
    summary.gamesBytes = summary.games.reduce((total, game) => total + game.sizeBytes, 0);
  }
  const find = (id: string) => {
    const found = roots.find((summary) => summary.root.id === id);
    if (!found) throw new Error("Unknown storage location.");
    return found;
  };
  return {
    list: async () => structuredClone(roots),
    preferences: async () => {
      const { value } = await settingsFixtureApi.get("core.storage");
      return { alwaysUseDefault: value.alwaysUseDefault === true };
    },
    add: async (path, label) => {
      const added = root(`root-${roots.length + 1}`, path, label ?? null, false);
      roots.push(added);
      return structuredClone(added);
    },
    edit: async (id, label, template) => {
      const summary = find(id);
      summary.root.label = label || null;
      summary.folderTemplate = template;
      return structuredClone(summary);
    },
    setDefault: async (id) => {
      for (const summary of roots) summary.root.isDefault = summary.root.id === id;
      return structuredClone(find(id));
    },
    remove: async (id) => {
      roots.splice(roots.indexOf(find(id)), 1);
    },
    moveAndRemove: async () => {
      throw new Error("Moving games needs the Caskade core.");
    },
    chooseDirectory: async () => null,
    move: async () => {
      throw new Error("Moving games needs the Caskade core.");
    },
    uninstall: async () => {
      throw new Error("Uninstalling games needs the Caskade core.");
    },
    subscribe: async () => () => {},
  };
}
