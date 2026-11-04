/** Explicit dev data only (`VITE_CASKADE_FIXTURES=1`): the core's settings
 * sections in memory, with the defaults of `caskade_core::settings::builtin`
 * and `caskade_sources::settings`. Never a fallback for failed native IPC. */
import type { SettingsApi, SettingsChanged, SettingsSection, SettingsSnapshot } from "./settings";

const launchers = [
  "steam",
  "heroic",
  "lutris",
  "epic",
  "gog",
  "ea",
  "ubisoft",
  "battlenet",
  "itch",
  "amazon",
  "riot",
  "rockstar",
];
const enabled = (ids: string[]) => Object.fromEntries(ids.map((id) => [id, true]));

/** `core.updates`'s JSON Schema as schemars writes it. */
const updatesSchema = {
  $schema: "https://json-schema.org/draft/2020-12/schema",
  title: "UpdateSettings",
  description:
    "Game updates (`crate::updates`): newer releases of games installed from GitHub, found without an account.",
  type: "object",
  properties: {
    check: {
      title: "Check for game updates",
      description: "Look for new releases of games installed from GitHub in the background.",
      type: "boolean",
    },
    intervalHours: {
      title: "Check every (hours)",
      description: "How long to wait between checks of a game.",
      type: "integer",
      format: "uint32",
      minimum: 1,
      maximum: 168,
    },
    askBeforePlay: {
      title: "Offer updates before playing",
      description:
        "When a newer release is out, ask before starting the game whether to update it first.",
      type: "boolean",
    },
  },
  additionalProperties: false,
  required: ["check", "intervalHours", "askBeforePlay"],
};

/** `core.installer`'s JSON Schema as schemars writes it. */
const installerSchema = {
  $schema: "https://json-schema.org/draft/2020-12/schema",
  title: "InstallerSettings",
  type: "object",
  properties: {
    windowsMethod: {
      title: "Windows install method",
      description: "Run supported installers on Windows, or prefer extracting their files.",
      $ref: "#/$defs/WindowsMethod",
    },
    execution: {
      title: "Running installers",
      description:
        "Allow installer processes, ask before running them, or only use plans that do not execute code.",
      $ref: "#/$defs/ExecutionPolicy",
    },
    downloadExecution: {
      title: "Running installers from downloads",
      description:
        "For games downloaded from a link (deep links, pasted or dropped URLs, magnet links and torrents, updates). The stricter of this and the execution policy applies.",
      $ref: "#/$defs/ExecutionPolicy",
    },
    askBelowConfidence: {
      title: "Ask when less sure than",
      description:
        "Ask before installing when Caskade is less sure than this of how a game installs.",
      type: "number",
      format: "float",
      minimum: 0,
      maximum: 1,
    },
    language: {
      title: "Installer language",
      description:
        "Leave empty for the installer's default. Unsupported language switches are reported by its handler.",
      type: "string",
      maxLength: 64,
      pattern: "^[A-Za-z0-9_-]*$",
    },
    letInstallersCreateShortcuts: {
      title: "Let installers create shortcuts",
      description: "Allow installer-created shortcuts where the package supports this preference.",
      type: "boolean",
    },
    compatibilityRunner: {
      title: "Compatibility runner",
      description:
        "Proton uses umu. GE-Proton and Steam Proton use detected builds; system Wine uses the detected Wine executable. Applies to new jobs.",
      $ref: "#/$defs/CompatibilityRunner",
    },
    sourceHandling: {
      title: "After installing from a file",
      description:
        "Keep, move to the system trash, or delete the selected local source after a successful managed install. Downloads have their own setting.",
      $ref: "#/$defs/SourceHandling",
    },
    keepDownloads: {
      title: "Keep downloads",
      description: "Retain downloaded installers after successful installation.",
      type: "boolean",
    },
    concurrentDownloads: {
      title: "Concurrent downloads",
      description:
        "Maximum simultaneous downloads. Active downloads finish when this limit is lowered.",
      type: "integer",
      format: "uint32",
      minimum: 1,
      maximum: 8,
    },
  },
  additionalProperties: false,
  required: [
    "windowsMethod",
    "execution",
    "downloadExecution",
    "askBelowConfidence",
    "language",
    "letInstallersCreateShortcuts",
    "compatibilityRunner",
    "sourceHandling",
    "keepDownloads",
    "concurrentDownloads",
  ],
  $defs: {
    WindowsMethod: { type: "string", enum: ["run_installer", "extract"] },
    ExecutionPolicy: { type: "string", enum: ["never", "ask", "allow"] },
    CompatibilityRunner: {
      oneOf: [
        { type: "string", const: "proton", title: "Proton via umu" },
        { type: "string", const: "ge_proton", title: "GE-Proton via umu" },
        { type: "string", const: "steam_proton", title: "Steam Proton via umu" },
        { type: "string", const: "system_wine", title: "System Wine" },
      ],
    },
    SourceHandling: {
      oneOf: [
        { type: "string", const: "keep", title: "Keep the file" },
        { type: "string", const: "trash", title: "Move to trash" },
        { type: "string", const: "delete", title: "Delete the file" },
      ],
    },
  },
};

const sections: {
  namespace: string;
  version: number;
  schema?: unknown;
  defaults: Record<string, unknown>;
}[] = [
  {
    namespace: "core.general",
    version: 1,
    defaults: {
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
  },
  {
    namespace: "core.appearance",
    version: 2,
    defaults: {
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
  },
  { namespace: "core.console", version: 1, defaults: { start: "auto", active: false } },
  { namespace: "core.emulation", version: 1, defaults: { desktopFullscreen: false } },
  { namespace: "core.launchers", version: 1, defaults: { enabled: enabled(launchers) } },
  {
    namespace: "core.discovery",
    version: 1,
    defaults: {
      enabled: true,
      scanOnStartup: true,
      backgroundRefresh: true,
      refreshInterval: "24h",
      rescanAfterLauncherInstall: true,
      watchFolders: true,
      mergeDuplicates: true,
      preferLauncherMetadata: true,
      deepScans: {
        customFolders: false,
        externalDrives: false,
        romDirectories: false,
        portableExecutables: false,
        winePrefixes: false,
        fullDisk: false,
      },
      osChecks: {
        windows: enabled(["registry", "manifests", "start-menu", "uwp"]),
        macos: enabled(["applications", "user-apps", "manifests", "bundles"]),
        linux: enabled(["desktop", "xdg", "launcher-config", "wine"]),
      },
      scanPaths: [],
      ignoredEntries: [],
      acceptedEntries: [],
    },
  },
  { namespace: "core.storage", version: 1, defaults: { alwaysUseDefault: false } },
  {
    namespace: "core.installer",
    version: 3,
    schema: installerSchema,
    defaults: {
      windowsMethod: "run_installer",
      execution: "allow",
      downloadExecution: "allow",
      askBelowConfidence: 0.5,
      language: "",
      letInstallersCreateShortcuts: false,
      compatibilityRunner: "proton",
      sourceHandling: "keep",
      keepDownloads: false,
      concurrentDownloads: 2,
    },
  },
  {
    namespace: "core.updates",
    version: 1,
    schema: updatesSchema,
    defaults: { check: true, intervalHours: 6, askBeforePlay: true },
  },
  { namespace: "core.onboarding", version: 1, defaults: { completed: false, sounds: true } },
  { namespace: "core.metadata", version: 1, defaults: { online: true } },
  { namespace: "core.appUpdates", version: 1, defaults: { check: true } },
  {
    namespace: "core.storeAccounts",
    version: 2,
    defaults: { backgroundRefresh: false, intervalHours: 24, playtimeImport: "always" },
  },
  {
    namespace: "core.backups",
    version: 1,
    defaults: {
      destination: "",
      components: ["artwork", "plugins", "pluginData", "themes"],
      schedule: "off",
      keep: 7,
    },
  },
  { namespace: "core.accounts", version: 1, defaults: { ratingBoard: "pegi" } },
  {
    namespace: "core.streaming",
    version: 1,
    defaults: {
      hosting: false,
      addresses: [],
      existing: false,
      quality: { resolution: null, fps: null, bitrateKbps: null },
    },
  },
  {
    namespace: "core.sync",
    version: 1,
    defaults: {
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
        emulatorConfig: "thisDeviceOnly",
      },
      neverRelay: false,
      port: null,
      peerDownloads: true,
      acceptCopies: false,
      remotePlay: true,
    },
  },
  {
    namespace: "core.achievements",
    version: 1,
    defaults: { enabled: true, background: true, notifyUnlocks: true },
  },
  { namespace: "discovery.import", version: 1, defaults: { seedPlaytime: true } },
];

export function createSettingsFixtureApi(): SettingsApi {
  const values = new Map(sections.map(({ namespace, defaults }) => [namespace, defaults]));
  const revisions = new Map<string, number>();
  const listeners = new Set<(event: SettingsChanged) => void>();
  const section = (namespace: string) => {
    const found = sections.find((candidate) => candidate.namespace === namespace);
    if (!found) throw new Error(`Unknown settings section ${namespace}.`);
    return found;
  };
  const snapshot = (namespace: string): SettingsSnapshot => ({
    namespace,
    version: section(namespace).version,
    revision: revisions.get(namespace) ?? 0,
    value: structuredClone(values.get(namespace) ?? {}) as Record<string, unknown>,
  });
  const api: SettingsApi = {
    sections: async () =>
      sections.map(
        ({ namespace, version, schema, defaults }): SettingsSection => ({
          namespace,
          version,
          schema: structuredClone(schema ?? { type: "object" }),
          defaults: structuredClone(defaults),
        }),
      ),
    get: async (namespace) => snapshot(namespace),
    set: async (namespace, value, revision) => {
      const { version } = section(namespace);
      if (revision !== (revisions.get(namespace) ?? 0))
        throw new Error("These settings changed elsewhere. Reload and try again.");
      values.set(namespace, structuredClone(value));
      revisions.set(namespace, revision + 1);
      for (const listener of listeners) listener({ namespace, version });
      return snapshot(namespace);
    },
    reset: (namespace, revision) =>
      api.set(namespace, structuredClone(section(namespace).defaults), revision),
    subscribe: async (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
  return api;
}

/** One store for the whole page, shared by every consumer of the settings API. */
export const settingsFixtureApi = createSettingsFixtureApi();
