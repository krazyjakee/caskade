import type {
  BeforePlay,
  ManifestStatus,
  SaveHistory,
  SaveState,
  SavesApi,
  SaveVersion,
} from "./saves";

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
  const restored: { entryId: string; versionId: string }[] = [];
  const revealed: { entryId: string; location: number }[] = [];
  const histories = new Map<string, SaveHistory>();
  const listeners = new Set<(gameId: string) => void>();
  let manifest: ManifestStatus = { available: true, checkedAtMs: Date.UTC(2026, 10, 1, 9) };
  const api: SavesApi & {
    seed(value: SaveState): void;
    seedCheck(entryId: string, check: BeforePlay): void;
    seedHistory(gameId: string, history: SaveHistory): void;
    seedManifest(status: ManifestStatus): void;
    chosen: typeof chosen;
    restored: typeof restored;
    revealed: typeof revealed;
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
    history: async (gameId) =>
      structuredClone(
        histories.get(gameId) ?? { accountId: "", versions: [], conflict: [], syncing: true },
      ),
    async restore(entryId, versionId) {
      for (const [gameId, history] of histories) {
        const from = history.versions.find((each) => each.version.id === versionId);
        if (!from) continue;
        if (!from.complete)
          throw new Error("That save's files haven't arrived on this device yet.");
        restored.push({ entryId, versionId });
        const made: SaveVersion = {
          ...from.version,
          id: `${versionId}-restored`,
          parents: [history.versions.find((each) => each.current)?.version.id ?? versionId],
          reason: "restored",
          local: true,
          createdAtMs: Date.now(),
        };
        histories.set(gameId, {
          ...history,
          conflict: [],
          versions: [
            { version: made, current: true, complete: true },
            ...history.versions.map((each) => ({ ...each, current: false })),
          ],
        });
        for (const listener of listeners) listener(gameId);
        return structuredClone(made);
      }
      throw new Error(`Save version ${versionId} was not found.`);
    },
    async reveal(entryId, location) {
      revealed.push({ entryId, location });
    },
    manifestStatus: async () => structuredClone(manifest),
    async manifestUpdate() {
      manifest = { available: true, checkedAtMs: Date.now() };
      return structuredClone(manifest);
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    seed(value) {
      states.set(value.entryId, structuredClone(value));
    },
    seedHistory(gameId, history) {
      histories.set(gameId, structuredClone(history));
    },
    seedManifest(status) {
      manifest = structuredClone(status);
    },
    seedCheck(entryId, check) {
      checks.set(entryId, structuredClone(check));
    },
    chosen,
    restored,
    revealed,
  };
  return api;
}
