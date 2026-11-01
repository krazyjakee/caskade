/** Settings' Library Discovery, Emulation (every section, RomM included),
 * Storage and Statistics panels in the pseudo-locale (#217): every text,
 * aria-label, title and placeholder they show comes from a catalog. What the
 * fixtures name (emulators, platforms, paths, drives) is data. */
import { i18n, loadCatalog, pseudoLocale } from "@caskade/i18n";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { createEmulationFixtureApi } from "../../api/emulationFixtures";
import type { RommApi, RommOverview } from "../../api/romm";
import { createStorageFixtureApi } from "../../api/storageFixtures";
import { fixtureStatsState } from "../../api/systemStatsFixtures";
import { renderApp } from "../../test/harness";
import { untranslated } from "../../test/untranslated";
import { RommApiContext } from "./romm/useRomm";
import { StatisticsDashboard } from "./statistics/StatisticsDashboard";

afterEach(cleanup);

const locale = pseudoLocale ?? "en-XA";

/** Paths, file names and extensions, and ids written as code. */
// `~/`, not a bare `~`: the pseudo-locale pads its text with tildes.
const paths = [/(?:~\/|\/)[^\s,·()[\]]*/g, /\.\w{2,5}\b/g];

const panel = () => document.querySelector('[role="tabpanel"]');

async function emulationData(): Promise<string[]> {
  const snapshot = await createEmulationFixtureApi().snapshot();
  return [
    ...snapshot.emulators.flatMap((emulator) => [
      emulator.name,
      ...emulator.profiles.map(({ config }) =>
        config.type === "custom" ? config.name : config.definitionProfile,
      ),
      ...emulator.definitionProfiles.map((profile) => profile.name),
    ]),
    ...snapshot.definitions.flatMap((definition) => [
      definition.name,
      ...definition.profiles.map((profile) => profile.name),
    ]),
    ...snapshot.platforms.flatMap((platform) => [platform.name, platform.short]),
    // The core describes its own variables.
    ...snapshot.variables.flatMap((variable) => [variable.name, variable.description]),
  ]
    .filter(Boolean)
    .sort((a, b) => b.length - a.length);
}

function rommApi(): RommApi {
  const overview: RommOverview = {
    settings: {
      servers: [
        {
          id: "home",
          name: "Home",
          url: "https://romm.local",
          auth: "password",
          username: "admin",
          allowInvalidCerts: false,
          importAll: true,
          platforms: [],
          collections: [],
          syncOnStartup: true,
          syncInterval: "6h",
        },
      ],
      cache: { dir: "/games/romm-cache", limitGb: 100, keepFavorites: true, verify: true },
    },
    servers: [
      {
        id: "home",
        state: "untested",
        version: null,
        error: null,
        lastSyncMs: null,
        syncing: false,
        games: 0,
        hasSecret: true,
      },
    ],
    cache: { dir: "/games/romm-cache", used: 0, limit: 100 * 1024 ** 3, games: [] },
    downloads: [],
  };
  const copy = async () => structuredClone(overview);
  return {
    overview: copy,
    saveSettings: copy,
    saveServer: copy,
    removeServer: copy,
    testServer: async () => ({ version: "4.4.0", platforms: [], collections: [] }),
    sync: async () => [],
    clearCache: async () => structuredClone(overview.cache),
    setPinned: async () => Promise.reject(new Error("unexpected setPinned")),
    evict: async () => true,
    cancelDownload: async () => true,
    subscribeDownloads: async () => () => {},
  };
}

it("shows Library Discovery in the pseudo-locale", async () => {
  renderApp("/settings?tab=library-discovery", { locale, messages: await loadCatalog(locale) });
  await screen.findAllByRole("switch", { name: /^\[.*\]$/ });
  await waitFor(() =>
    expect(
      untranslated([panel()], {
        // Launcher ids and the adapter's check labels come from the core.
        data: [...paths, /\b(steam|heroic|lutris|legendary|desktop|linux)\b/g, "Desktop entries"],
      }),
    ).toEqual([]),
  );
});

for (const section of ["emulators", "browse", "platforms", "scanners", "romm"]) {
  it(`shows Emulation → ${section} in the pseudo-locale`, async () => {
    const data = await emulationData();
    renderApp(`/settings?tab=emulators&section=${section}`, {
      locale,
      messages: await loadCatalog(locale),
      wrap: (children) => (
        <RommApiContext.Provider value={rommApi()}>{children}</RommApiContext.Provider>
      ),
    });
    await screen.findByRole("radio", { name: /^\[.*\]$/, checked: true });
    await waitFor(() =>
      expect(
        untranslated([panel()], {
          data: [
            ...data,
            ...paths,
            "Home",
            "admin",
            "RomM",
            "RetroArch",
            "Flatpak",
            "AppImage",
            // Emulators and platforms the setup offers, a scanner's name,
            // file types, BIOS files and launch commands.
            "Sample Game (USA)",
            "SNES collection",
            "bios/",
            "org.libretro.RetroArch",
            /Sony PlayStation|Nintendo Switch|Switch|DuckStation/g,
            /scph\d+/g,
            /\b(sfc|smc|cue|chd|flatpak|run)\b/g,
          ],
          // Patterns are code, and the Flatpak hint is the core's text.
          skip: 'code, [data-part="flatpak-hint"]',
        }),
      ).toEqual([]),
    );
  });
}

it("shows Storage in the pseudo-locale", async () => {
  renderApp("/settings?tab=storage", {
    locale,
    messages: await loadCatalog(locale),
    storage: createStorageFixtureApi(),
  });
  await screen.findAllByRole("button", { name: /Games/ });
  await waitFor(() =>
    expect(
      untranslated([panel()], {
        // The folder-name template's placeholder shows as typed.
        data: [...paths, /Fixture Game \d+/g, "External", "Games", "ext4", "{title}"],
      }),
    ).toEqual([]),
  );
});

it("shows Statistics in the pseudo-locale", async () => {
  i18n.loadAndActivate({ locale, messages: await loadCatalog(locale) });
  const state = fixtureStatsState();
  const sample = state.samples.at(-1);
  const data = [
    ...(sample?.gpus.devices.flatMap((gpu) => [gpu.name, gpu.id]) ?? []),
    ...(sample?.disks.flatMap((disk) => [disk.name, disk.fileSystem ?? ""]) ?? []),
    ...(sample?.network.interfaces.map((item) => item.name) ?? []),
    "AMD",
    "Intel",
    "NVIDIA",
  ]
    .filter(Boolean)
    .sort((a, b) => b.length - a.length);
  const { container } = render(<StatisticsDashboard stats={state} />);
  expect(untranslated([container], { data: [...data, ...paths, /\b[AP]M\b/g] })).toEqual([]);
});
