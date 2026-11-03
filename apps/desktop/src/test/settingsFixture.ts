/** Core's settings sections in memory, and a quiet discovery backend, for
 * tests that open Settings in the whole app (#151). Defaults follow
 * `caskade_core::settings::builtin`. */
import type { DiscoveryApi, DiscoverySnapshot } from "../api/discovery";
import type { SettingsApi, SettingsChanged, SettingsSnapshot } from "../api/settings";

const installerSchema = {
  type: "object",
  properties: {
    shortcuts: { type: "boolean", title: "Create desktop shortcuts" },
    downloads: { type: "integer", title: "Parallel downloads", minimum: 1, maximum: 4 },
  },
};

export function coreSettingsDefaults(): Record<string, Record<string, unknown>> {
  return {
    "core.general": {
      launchOnLogin: false,
      startMinimized: false,
      startView: "home",
      onLaunch: "minimize",
      onClose: "tray",
      confirmQuit: true,
      cacheDir: "/fixture/cache",
      language: "system",
      timeFormat: "system",
    },
    "core.appearance": {
      theme: "caskade.default",
      density: "comfortable",
      cardSize: 176,
      uiScale: 100,
      showArtTitles: true,
      motion: "system",
      reduceEffects: false,
      accent: null,
      customCss: "",
      customCssEnabled: true,
    },
    "core.console": { start: "auto", active: false },
    "core.emulation": { desktopFullscreen: false },
    "core.launchers": { enabled: { steam: true, heroic: true, lutris: false } },
    "core.discovery": {
      enabled: true,
      scanOnStartup: true,
      backgroundRefresh: true,
      refreshInterval: "24h",
      watchFolders: true,
      deepScans: {
        customFolders: false,
        externalDrives: false,
        portableExecutables: false,
        winePrefixes: false,
        fullDisk: false,
      },
      osChecks: { linux: { desktop: true } },
      scanPaths: [],
      ignoredEntries: [],
    },
    "discovery.import": { seedPlaytime: true },
    "core.storage": { alwaysUseDefault: false },
    "core.installer": { shortcuts: false, downloads: 2 },
    "core.onboarding": { completed: false, sounds: true },
    "core.metadata": { online: true },
    "core.appUpdates": { check: true },
    "core.storeAccounts": { backgroundRefresh: false, intervalHours: 24, playtimeImport: "always" },
    "core.backups": {
      destination: "",
      components: ["artwork", "plugins", "pluginData", "themes"],
      schedule: "off",
      keep: 7,
    },
    "core.accounts": { ratingBoard: "pegi" },
    "core.streaming": {
      hosting: false,
      addresses: [],
      existing: false,
      quality: { resolution: null, fps: null, bitrateKbps: null },
    },
    "core.achievements": { enabled: true, background: true, notifyUnlocks: true },
    "core.saves": {
      manifestUrl:
        "https://raw.githubusercontent.com/mtkennerly/ludusavi-manifest/master/data/manifest.yaml",
      keep: 20,
    },
    "core.sync": {
      categories: {
        library: "sync",
        artwork: "sync",
        achievementLinks: "sync",
        playSessions: "sync",
        saves: "sync",
        accounts: "sync",
        appearance: "sync",
        rommServers: "sync",
        launchActions: "thisDeviceOnly",
        gameScripts: "thisDeviceOnly",
        preferredEmulators: "thisDeviceOnly",
        addons: "thisDeviceOnly",
      },
      neverRelay: false,
      port: null,
      acceptCopies: false,
      remotePlay: true,
    },
  };
}

/** Every write lands in `values`; `writes` lists the namespaces in order. */
export function createSettingsFixture(values = coreSettingsDefaults()) {
  const defaults = structuredClone(values);
  const revisions: Record<string, number> = {};
  const writes: string[] = [];
  const listeners = new Set<(event: SettingsChanged) => void>();
  const snapshot = (namespace: string): SettingsSnapshot => ({
    namespace,
    version: 1,
    revision: revisions[namespace] ?? 0,
    value: structuredClone(values[namespace] ?? {}),
  });
  const api: SettingsApi = {
    sections: async () =>
      Object.keys(values).map((namespace) => ({
        namespace,
        version: 1,
        schema: namespace === "core.installer" ? installerSchema : {},
        defaults: defaults[namespace],
      })),
    get: async (namespace) => {
      if (!(namespace in values)) throw new Error(`unknown settings section ${namespace}`);
      return snapshot(namespace);
    },
    set: async (namespace, value, revision) => {
      if (revision !== (revisions[namespace] ?? 0)) throw new Error("Revision conflict");
      values[namespace] = structuredClone(value);
      revisions[namespace] = revision + 1;
      writes.push(namespace);
      for (const listener of listeners) listener({ namespace, version: 1 });
      return snapshot(namespace);
    },
    reset: async (namespace, revision) =>
      api.set(namespace, structuredClone(defaults[namespace] ?? {}), revision),
    subscribe: async (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
  return { api, values, writes };
}

export function createDiscoveryFixture(): DiscoveryApi {
  const snapshot: DiscoverySnapshot = {
    revision: 1,
    os: "linux",
    checks: [{ id: "desktop", label: "Desktop entries", sources: ["desktop"] }],
    phase: "complete",
    launchers: [
      {
        source: "steam",
        found: true,
        path: "/fixture/Steam",
        version: "1",
        kind: { type: "native" },
      },
    ],
    sources: [],
    found: 0,
    imported: 0,
    review: [],
    warnings: [],
    error: null,
    watching: false,
  };
  return {
    status: async () => snapshot,
    launchers: async () => snapshot,
    scan: async () => snapshot,
    cancel: async () => snapshot,
    review: async () => snapshot,
    subscribe: async () => () => {},
  };
}
