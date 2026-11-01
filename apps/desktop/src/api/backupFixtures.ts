/** An in-memory backup core for the browser preview and tests: backups
 * finish at once, and any path ending in `.zip` inspects as a backup. */
import type {
  BackupApi,
  BackupComponent,
  BackupInspection,
  BackupPlan,
  BackupStatus,
} from "./backup";

const MB = 1024 * 1024;
const sizes: Record<BackupComponent, [files: number, bytes: number]> = {
  library: [1, 68 * MB],
  artwork: [24, 3 * MB],
  downloadedArtwork: [6000, 7000 * MB],
  plugins: [12, 2 * MB],
  pluginData: [4, MB / 2],
  themes: [3, MB / 4],
};

export const fixtureInspection = (path: string): BackupInspection => ({
  manifest: {
    format: 1,
    appVersion: "0.1.0",
    schemaVersion: 21,
    createdAtMs: Date.UTC(2026, 9, 28, 21, 30),
    os: "linux",
    setId: null,
    components: (["library", "artwork", "plugins"] as const).map((id) => ({
      id,
      files: sizes[id][0],
      bytes: sizes[id][1],
    })),
    tables: [
      { name: "games", rows: 812 },
      { name: "play_sessions", rows: 4210 },
      { name: "collections", rows: 9 },
    ],
    excluded: ["credentials", "browserData", "gameFiles"],
    files: [],
  },
  components: (["library", "artwork", "plugins"] as const).map((id) => ({
    id,
    files: sizes[id][0],
    bytes: sizes[id][1],
  })),
  currentSchema: 21,
  needsMigration: path.includes("old"),
  otherOs: path.includes("windows"),
  missingRoots: path.includes("windows") ? ["D:\\Games"] : [],
  missingGames: path.includes("windows") ? 14 : 0,
});

export function createBackupFixtureApi(initial?: Partial<BackupStatus>): BackupApi & {
  calls: string[];
} {
  let status: BackupStatus = {
    running: null,
    last: null,
    nextAutomaticMs: null,
    defaultDestination: "/home/player/.local/share/caskade/backups",
    stagedRestore: null,
    lastRestore: null,
    ...initial,
  };
  const listeners = new Set<(status: BackupStatus) => void>();
  const calls: string[] = [];
  const emit = (next: BackupStatus) => {
    status = next;
    for (const listener of listeners) listener(status);
  };
  return {
    calls,
    status: async () => status,
    plan: async (destination, components): Promise<BackupPlan> => {
      const planned: BackupComponent[] = ["library", ...components];
      const all: BackupComponent[] = [
        "library",
        "artwork",
        "downloadedArtwork",
        "plugins",
        "pluginData",
        "themes",
      ];
      const list = all.map((id) => ({
        id,
        selected: planned.includes(id),
        files: sizes[id][0],
        bytes: sizes[id][1],
      }));
      const missing = destination?.includes("unplugged") ?? false;
      return {
        destination: destination || status.defaultDestination,
        components: list,
        excluded: [
          "credentials",
          "browserData",
          "gameFiles",
          "managedEmulators",
          "downloadedRoms",
          "caches",
          "pendingJobs",
          "logs",
        ],
        estimatedBytes: list.filter((c) => c.selected).reduce((sum, c) => sum + c.bytes, 0),
        availableBytes: missing ? null : 120 * 1024 * MB,
        destinationExists: !missing,
      };
    },
    start: async () => {
      calls.push("start");
      const at = Date.now();
      emit({
        ...status,
        running: {
          kind: "backup",
          progress: { phase: "writing", doneBytes: 20 * MB, totalBytes: 70 * MB },
        },
      });
      emit({
        ...status,
        running: null,
        last: {
          atMs: at,
          automatic: false,
          path: `${status.defaultDestination}/caskade-backup-20261030-093000.zip`,
          bytes: 71 * MB,
          error: null,
        },
      });
    },
    cancel: async () => {
      calls.push("cancel");
    },
    inspect: async (path) => {
      calls.push(`inspect ${path}`);
      if (!path.endsWith(".zip")) {
        throw Object.assign(new Error("This isn't a Caskade backup."), { code: "corrupt" });
      }
      return fixtureInspection(path);
    },
    restore: async (path, components) => {
      calls.push(`restore ${path} ${components.join(",")}`);
      const journal = {
        state: "staged" as const,
        archive: path.split(/[\\/]/).pop() ?? path,
        createdAtMs: Date.UTC(2026, 9, 28, 21, 30),
        appVersion: "0.1.0",
        components: ["library" as const, ...components],
        inspection: fixtureInspection(path),
      };
      emit({ ...status, stagedRestore: journal });
      return journal;
    },
    discardRestore: async () => {
      calls.push("discard");
      emit({ ...status, stagedRestore: null });
    },
    restart: async () => {
      calls.push("restart");
    },
    reveal: async (path) => {
      calls.push(`reveal ${path}`);
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}
