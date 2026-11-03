/** First-run setup over fixtures, for its tests and the anatomy survey: a games folder, a scan that finds Steam's games on its second
 * look, the emulation fixtures and a RomM server that answers. */
import { render } from "@testing-library/react";
import type { ReactNode } from "react";
import { vi } from "vitest";
import type { DiscoveryApi, DiscoverySnapshot } from "../api/discovery";
import { createEmulationFixtureApi } from "../api/emulationFixtures";
import { createFixtureApi, createFixtureGames } from "../api/fixtures";
import { createSettingsLocaleApi } from "../api/locale";
import { createLibraryQueryClient, LibraryProvider } from "../api/provider";
import type { RommApi } from "../api/romm";
import { StorageProvider } from "../api/StorageProvider";
import type { RootSummary, StorageApi } from "../api/storage";
import { PadSource } from "../input";
import { LocaleProvider } from "../locale/LocaleProvider";
import { OnboardingGate } from "../onboarding/OnboardingGate";
import { ModeProvider } from "../shell/console/mode";
import { DiscoveryProvider } from "../views/settings/discovery/DiscoveryProvider";
import { EmulationProvider } from "../views/settings/emulators/EmulationProvider";
import { RommApiContext } from "../views/settings/romm/useRomm";
import { SettingsProvider } from "../views/settings/useSettings";
import { emptyStorage, fakeConsoleApi, fakePad } from "./harness";
import { createSettingsFixture } from "./settingsFixture";

const root: RootSummary = {
  root: { id: "games", path: "/home/player/Games/Caskade", isDefault: true, label: null },
  status: "online",
  capabilities: {
    writable: true,
    executable: true,
    symlinks: true,
    caseSensitive: true,
    freeBytes: 400 * 1024 ** 3,
    totalBytes: 1024 ** 4,
    filesystem: "ext4",
    warnings: [],
  },
  folderTemplate: "{title}",
  games: [],
  gamesBytes: 0,
  otherBytes: null,
};
const storage: StorageApi = { ...emptyStorage, list: async () => [root] };

/** A scan that finds Steam's games on the second look. */
export function discovery(counts = { found: 12, imported: 12 }) {
  let looks = 0;
  const snapshot = (): DiscoverySnapshot => ({
    revision: looks,
    os: "linux",
    checks: [],
    phase: looks > 1 ? "complete" : "scanning",
    launchers: [],
    sources:
      looks > 0 ? [{ source: "steam", count: 12, durationMs: 1, warnings: [], error: null }] : [],
    found: looks > 1 ? counts.found : 0,
    imported: looks > 1 ? counts.imported : 0,
    review: [],
    warnings: [],
    error: null,
    watching: false,
  });
  const scan = vi.fn(async () => snapshot());
  const api: DiscoveryApi = {
    status: async () => {
      looks++;
      return snapshot();
    },
    launchers: async () => snapshot(),
    scan,
    cancel: async () => snapshot(),
    review: async () => snapshot(),
    subscribe: async () => () => {},
  };
  return { api, scan };
}

export function romm(): RommApi & { saved: string[] } {
  const saved: string[] = [];
  const refuse = async () => Promise.reject(new Error("unexpected"));
  return {
    saved,
    overview: async () =>
      ({
        settings: { servers: [], cache: { maxGb: 20, keepPinned: true } },
        servers: [],
        cache: { usedBytes: 0, entries: [] },
        downloads: [],
      }) as unknown as Awaited<ReturnType<RommApi["overview"]>>,
    saveSettings: refuse,
    saveServer: async (server, secret) => {
      saved.push(`${server.id} ${server.url} ${server.auth}:${server.username}:${secret}`);
      return {} as Awaited<ReturnType<RommApi["saveServer"]>>;
    },
    removeServer: refuse,
    testServer: async () => ({ version: "4.1", platforms: [], collections: [] }),
    sync: async (id) => [
      {
        serverId: id ?? "",
        added: 30,
        updated: 0,
        unchanged: 2,
        removed: 0,
        orphaned: 0,
        unmatchedPlatforms: [],
        warnings: [],
      },
    ],
    clearCache: refuse,
    setPinned: refuse,
    evict: refuse,
    cancelDownload: refuse,
    subscribeDownloads: async () => () => {},
  };
}

export function renderOnboarding(
  settings = createSettingsFixture(),
  found = discovery(),
  systemLanguages = ["en-US"],
  /** Accounts and devices, for setting up from another device (#242). */
  extra: {
    accounts?: (library: ReturnType<typeof createFixtureApi>) => void;
    wrap?: (children: ReactNode) => ReactNode;
  } = {},
) {
  const onDone = vi.fn();
  // The Language setting switches the language as the core would.
  const locale = createSettingsLocaleApi({ settings: settings.api, system: () => systemLanguages });
  const consoleApi = fakeConsoleApi();
  const pad = fakePad();
  const library = createFixtureApi(createFixtureGames(3));
  extra.accounts?.(library);
  const gate = (
    <OnboardingGate onDone={onDone}>
      <p>The library</p>
    </OnboardingGate>
  );
  render(
    <LocaleProvider api={locale}>
      <LibraryProvider api={library} client={createLibraryQueryClient()}>
        <StorageProvider api={storage}>
          <DiscoveryProvider api={found.api}>
            <EmulationProvider api={createEmulationFixtureApi()}>
              <RommApiContext.Provider value={romm()}>
                <ModeProvider api={consoleApi.api}>
                  <SettingsProvider api={settings.api}>
                    <PadSource value={pad.subscribe}>
                      {extra.wrap ? extra.wrap(gate) : gate}
                    </PadSource>
                  </SettingsProvider>
                </ModeProvider>
              </RommApiContext.Provider>
            </EmulationProvider>
          </DiscoveryProvider>
        </StorageProvider>
      </LibraryProvider>
    </LocaleProvider>,
  );
  return { settings, onDone, consoleApi, library, pad };
}
