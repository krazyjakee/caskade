import type { BeforePlay, SaveState, SavesApi, SaveVersion } from "./saves";

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
  // What the next check before a launch answers, per entry.
  const checks = new Map<string, BeforePlay>();
  const chosen: { entryId: string; versionId: string }[] = [];
  const api: SavesApi & {
    seed(value: SaveState): void;
    seedCheck(entryId: string, check: BeforePlay): void;
    chosen: typeof chosen;
  } = {
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
    beforePlay: async (entryId) =>
      structuredClone(
        checks.get(entryId) ?? { check: { type: "noSaves" }, checked: true, warnings: [] },
      ),
    async choose(entryId, versionId) {
      const check = checks.get(entryId)?.check;
      const versions = check?.type === "conflict" ? check.versions : [];
      const version = versions.find((each) => each.id === versionId);
      if (!version) throw new Error(`Save version ${versionId} was not found.`);
      chosen.push({ entryId, versionId });
      checks.delete(entryId);
      const made: SaveVersion = {
        ...version,
        id: `${versionId}-chosen`,
        parents: versions.map((each) => each.id),
        reason: "chosen",
        local: true,
        createdAtMs: Date.now(),
      };
      return structuredClone(made);
    },
    seed(value) {
      states.set(value.entryId, structuredClone(value));
    },
    seedCheck(entryId, check) {
      checks.set(entryId, structuredClone(check));
    },
    chosen,
  };
  return api;
}
