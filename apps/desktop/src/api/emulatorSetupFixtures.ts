/** Synthetic one-click emulator installs for the browser preview and
 * tests: installs finish after a short download. */
import type { EmulatorSetupApi, Installable, MissingPlatform, SetupJob } from "./emulatorSetup";

const fixtureCatalog: Installable[] = [
  {
    definitionId: "ppsspp",
    name: "PPSSPP",
    website: "https://www.ppsspp.org/",
    platforms: ["sony_psp"],
    source: "github",
    updates: true,
    projectUrl: "https://github.com/hrydgard/ppsspp/releases",
    managed: {
      emulatorId: "managed-ppsspp",
      definitionId: "ppsspp",
      source: "github",
      dir: "/home/you/.local/share/Caskade/emulators/ppsspp",
      installed: "v1.20.3",
      file: "PPSSPP-v1.20.3-anylinux-x86_64.AppImage",
      latest: "v1.20.4",
      latestUrl: null,
      checkedAtMs: 0,
      skipped: null,
      error: null,
    },
    installedLabel: "v1.20.3",
    latestLabel: "v1.20.4",
    updateAvailable: true,
  },
  {
    definitionId: "duckstation",
    name: "DuckStation",
    website: "https://github.com/stenzek/duckstation",
    platforms: ["sony_playstation"],
    source: "github",
    updates: true,
    projectUrl: "https://github.com/stenzek/duckstation/releases",
    managed: null,
    installedLabel: null,
    latestLabel: null,
    updateAvailable: false,
  },
  {
    definitionId: "mgba",
    name: "mGBA",
    website: "https://mgba.io/",
    platforms: ["nintendo_gb", "nintendo_gba", "nintendo_gbc"],
    source: "github",
    updates: true,
    projectUrl: "https://github.com/mgba-emu/mgba/releases",
    managed: null,
    installedLabel: null,
    latestLabel: null,
    updateAvailable: false,
  },
  {
    definitionId: "dolphin",
    name: "Dolphin",
    website: "https://dolphin-emu.org/",
    platforms: ["nintendo_gamecube", "nintendo_wii"],
    source: "flatpak",
    updates: false,
    projectUrl: "https://flathub.org/apps/org.DolphinEmu.dolphin-emu",
    managed: null,
    installedLabel: null,
    latestLabel: null,
    updateAvailable: false,
  },
];

export function createEmulatorSetupFixtureApi(): EmulatorSetupApi {
  let catalog = structuredClone(fixtureCatalog);
  let missing: MissingPlatform[] = [
    {
      platformId: "sony_playstation",
      name: "Sony PlayStation",
      short: "PS1",
      games: 12,
      recommended: { definitionId: "duckstation", name: "DuckStation", profile: "Default" },
    },
    {
      platformId: "nintendo_switch",
      name: "Nintendo Switch",
      short: "Switch",
      games: 2,
      recommended: null,
    },
  ];
  const jobs = new Map<string, SetupJob>();
  const listeners = new Set<(job: SetupJob) => void>();
  const emit = (job: SetupJob) => {
    jobs.set(job.id, job);
    for (const listener of listeners) listener(structuredClone(job));
  };
  const start = (definitionId: string, platformId: string | null): SetupJob => {
    const item = catalog.find((row) => row.definitionId === definitionId);
    if (!item) throw new Error(`unknown emulator ${definitionId}`);
    const job: SetupJob = {
      id: `setup-${jobs.size + 1}`,
      revision: 0,
      definitionId,
      name: item.name,
      platformId,
      status: "downloading",
      version: "v1.0",
      downloaded: 0,
      total: 40 * 1048576,
      emulatorId: null,
      defaultsSet: [],
      fetchedBios: [],
      warnings: [],
      error: null,
    };
    emit(job);
    setTimeout(() => {
      const platforms = item.platforms;
      catalog = catalog.map((row) =>
        row.definitionId === definitionId
          ? {
              ...row,
              managed: {
                emulatorId: `managed-${definitionId}`,
                definitionId,
                source: row.source,
                dir: `/home/you/.local/share/Caskade/emulators/${definitionId}`,
                installed: "v1.0",
                file: null,
                latest: "v1.0",
                latestUrl: null,
                checkedAtMs: Date.now(),
                skipped: null,
                error: null,
              },
              installedLabel: "v1.0",
              latestLabel: "v1.0",
              updateAvailable: false,
            }
          : row,
      );
      missing = missing.filter((row) => !platforms.includes(row.platformId));
      emit({
        ...job,
        revision: 1,
        status: "done",
        downloaded: job.total ?? 0,
        emulatorId: `managed-${definitionId}`,
        defaultsSet: platforms,
      });
    }, 30);
    return structuredClone(job);
  };
  const managedOf = (emulatorId: string) => {
    const managed = catalog.find((row) => row.managed?.emulatorId === emulatorId)?.managed;
    if (!managed) throw new Error(`${emulatorId} isn't installed by Caskade`);
    return structuredClone(managed);
  };
  return {
    installable: async () => structuredClone(catalog),
    missing: async () => structuredClone(missing),
    install: async (definitionId) => start(definitionId, null),
    installFor: async (platformId) => {
      const row = missing.find((platform) => platform.platformId === platformId);
      if (!row?.recommended) throw new Error(`nothing to install for ${platformId}`);
      return start(row.recommended.definitionId, platformId);
    },
    jobs: async () => [...jobs.values()],
    check: async (emulatorId) => managedOf(emulatorId),
    skip: async (emulatorId) => {
      catalog = catalog.map((row) =>
        row.managed?.emulatorId === emulatorId
          ? {
              ...row,
              updateAvailable: false,
              managed: { ...row.managed, skipped: row.managed.latest },
            }
          : row,
      );
      return managedOf(emulatorId);
    },
    update: async (emulatorId) => start(managedOf(emulatorId).definitionId, null),
    fetchBios: async (platformId) => ({
      platformId,
      fetched: ["scph5500.bin"],
      missing: [],
      dir: "/home/you/.local/share/duckstation/bios",
    }),
    cancel: async (jobId) => {
      const job = jobs.get(jobId);
      if (!job) throw new Error(`unknown emulator install ${jobId}`);
      return job;
    },
    subscribe: async (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}
