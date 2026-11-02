import type { SaveState, SavesApi } from "./saves";

/** Explicit fixture state, with the same stale-editor rule as the core. */
export function createFixtureSaves() {
  const states = new Map<string, SaveState>();
  const state = (entryId: string): SaveState =>
    structuredClone(
      states.get(entryId) ?? {
        entryId,
        revision: 0,
        preferences: { manifestTitle: null, locations: null },
        matched: { type: "unknown" },
        automatic: [],
        locations: [],
        warnings: [],
      },
    );
  const api: SavesApi & { seed(value: SaveState): void } = {
    state: async (entryId) => state(entryId),
    async edit(entryId, preferences, revision) {
      const current = state(entryId);
      if (revision !== current.revision)
        throw new Error("Save settings changed elsewhere. Reload them and try again.");
      const next: SaveState = {
        ...current,
        revision: revision + 1,
        preferences: structuredClone(preferences),
        locations:
          preferences.locations?.map(
            (spec) =>
              [...current.locations, ...current.automatic].find(
                (location) => location.spec.path === spec.path,
              ) ?? {
                spec,
                tags: [],
                fileCount: 0,
              },
          ) ?? current.automatic,
      };
      states.set(entryId, next);
      return structuredClone(next);
    },
    seed(value) {
      states.set(value.entryId, structuredClone(value));
    },
  };
  return api;
}
