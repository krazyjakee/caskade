/** The views' own content in the pseudo-locale (#9): Home, the Library's
 * filters (populated, empty, by platform and the Web mosaic) in both
 * layouts and its quick look, a game's page, the Store and a catalog game's
 * page, Collections and a collection, Activity's jobs and notifications,
 * and the Settings tabs the settings tests leave out (Scripts, Store
 * accounts, Streaming, Backup). Views that differ by shell are checked in
 * both. Every text, aria-label, title and placeholder they show comes from
 * a catalog; what the fixtures name is data. Each view is checked once its
 * data has loaded: a check that may pass early would pass on a view still
 * loading. */
import { loadCatalog, pseudoLocale } from "@caskade/i18n";
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { AchievementsProvider } from "../api/achievements";
import {
  createAchievementsFixture,
  retroSet,
  staleGogSet,
  steamSet,
} from "../api/achievementsFixtures";
import { BackupProvider } from "../api/backup";
import { createBackupFixtureApi } from "../api/backupFixtures";
import { createCatalogFixtureApi, fixtureCatalog } from "../api/catalogFixtures";
import { createEmulatedFixtureGames, createFixtureGames } from "../api/fixtures";
import { CatalogProvider, type GameIgdbApi, GameIgdbProvider, type IgdbGame } from "../api/igdb";
import { fixtureJob } from "../api/installFixtures";
import { NotificationsProvider } from "../api/notifications";
import { createNotificationsFixtureApi, fixtureNotifications } from "../api/notificationsFixtures";
import { StoreAccountsProvider } from "../api/storeAccounts";
import {
  createStoreAccountsFixtureApi,
  fixtureAccounts,
  fixtureProviders,
} from "../api/storeAccountsFixtures";
import { StreamingApiProvider } from "../api/streaming";
import { createStreamingFixtureApi, fixtureStreamHosts } from "../api/streamingFixtures";
import type { GameDetails } from "../api/types";
import type { Mode } from "../shell/console/mode";
import { presentPlatform } from "../views/library/platforms/catalog";
import { type AppOptions, fakeViewport, playedGames, renderApp } from "./harness";
import { untranslated } from "./untranslated";

beforeEach(() => {
  fakeViewport();
  vi.stubGlobal("scrollTo", vi.fn());
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const locale = pseudoLocale ?? "en-XA";
const modes: Mode[] = ["desktop", "console"];

const MONTH = "(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)";

/** Data every view may show: the brand, the fixture library's games, and
 * what the pseudo-locale formats as English (Intl knows en-XA as English):
 * dates, times, relative days and sizes. */
const common: (RegExp | string)[] = [
  /CASKADE|Caskade/g,
  /IGDB/g,
  "Original development fixture. No personal library data.",
  "Development library",
  /Fixture Game \d+/g,
  // The fixture games' tag and source.
  /\bFixture\b/gi,
  /\b(?:Adventure|Strategy|Puzzle)\b/g,
  /\bPC\b/g,
  new RegExp(
    `\\b${MONTH}(?![a-z])(?: \\d{1,2})?,? ?(?:\\d{4})?(?:, \\d{1,2}:\\d{2}\\s?[AP]M)?`,
    "g",
  ),
  /\b(?:Mon|Tues|Wednes|Thurs|Fri|Satur|Sun)day\b/g,
  /\b[AP]M\b/g,
  /\b(?:[Tt]oday|[Yy]esterday|[Ll]ast week|\d+ (?:days|weeks) ago)\b/g,
  /\d+(?:[.,]\d+)?\s?(?:kB|MB|GB|TB|PB)\b/g,
];

/** Devices paired through sync, as the fixtures name them. */
const devices =
  /Den-PC|Steam Deck|Office laptop|Work PC|Living room|Kids' PC|Study|Deck|Bedroom TV/g;

/** Paths, file names and extensions. `~/`, not a bare `~`: the
 * pseudo-locale pads its text with tildes. */
const paths = [/(?:~\/|\/)[^\s,·()[\]]*/g, /\.\w{2,5}\b/g];

const strings = (values: (string | null | undefined)[]) =>
  values.filter((value): value is string => Boolean(value));

async function open(path: string, options: Omit<AppOptions, "locale" | "messages"> = {}) {
  return renderApp(path, { locale, messages: await loadCatalog(locale), ...options });
}

const one = (selector: string) => document.querySelector(selector);
const all = (selector: string) => document.querySelectorAll(selector);

/** Waits until `selector` matches something. */
const ready = (selector: string) => waitFor(() => expect(one(selector)).not.toBeNull());

/** English left in what `selectors` match. */
const shown = (selectors: string[], data: (RegExp | string)[] = [], skip?: string) =>
  untranslated(
    selectors.flatMap((selector) => [...all(selector)]),
    { data: [...data, ...common], skip },
  );

// Home.

it.each(modes)("shows Home with games played in the pseudo-locale (%s)", async (mode) => {
  await open("/", { mode, games: playedGames(8) });
  await waitFor(() =>
    expect(one('[data-view="home"] [data-cs="hero"] [data-part="title"]')?.textContent).toMatch(
      /Fixture Game/,
    ),
  );
  await waitFor(() =>
    expect(all('[data-view="home"] [data-cs="game-card"]').length).toBeGreaterThan(0),
  );
  expect(shown(['[data-view="home"]'])).toEqual([]);
});

it.each(modes)("shows an empty Home in the pseudo-locale (%s)", async (mode) => {
  await open("/", { mode, games: [] });
  await ready('[data-view="home"] a[href="/library?filter=all"]');
  expect(shown(['[data-view="home"]'])).toEqual([]);
});

// The Library.

it.each(modes)("shows a Library filter in both layouts in the pseudo-locale (%s)", async (mode) => {
  for (const layout of ["grid", "list"] as const) {
    await open(`/library?filter=installed&sort=title&layout=${layout}`, { mode });
    const item = layout === "grid" ? "game-card" : "game-row";
    await waitFor(() =>
      expect(all(`[data-view="library"] [data-cs="${item}"]`).length).toBeGreaterThan(0),
    );
    expect(shown(['[data-view="library"]'])).toEqual([]);
    cleanup();
  }
});

it.each(modes)("shows the Library's empty filters in the pseudo-locale (%s)", async (mode) => {
  // Nothing played yet, and no web games.
  for (const filter of ["recent", "web"]) {
    await open(`/library?filter=${filter}`, { mode });
    await ready('[data-view="library"] [data-part="empty"]');
    expect(shown(['[data-view="library"]'])).toEqual([]);
    cleanup();
  }
});

it.each(modes)("shows the Library's platforms in the pseudo-locale (%s)", async (mode) => {
  const games = createEmulatedFixtureGames();
  const platforms = [...new Set(games.map((game) => game.platform ?? ""))].filter(Boolean);
  await open("/library?filter=emulated&sort=title", { mode, games });
  await waitFor(() => expect(all('[data-cs="platform-card"]').length).toBeGreaterThan(1));
  expect(
    shown(
      ['[data-view="library"]'],
      [
        // Platforms, their short names and makers.
        ...platforms.flatMap((name) => [name, presentPlatform(name).title]),
        /Nintendo|Sony|Sega/g,
      ],
    ),
  ).toEqual([]);
});

/** A web game, as the Web tab lists them. */
function webGame(id: string, overrides: Partial<GameDetails> = {}): GameDetails {
  const [template] = createFixtureGames(1);
  if (!template) throw new Error("no fixture");
  return {
    ...template,
    id,
    title: `Web ${id}`,
    entries: [
      {
        id: `${id}-entry`,
        source: "web",
        state: "installed",
        location: null,
        canLaunch: true,
        emulated: false,
      },
    ],
    ...overrides,
  };
}

it.each(modes)("shows the Web tab's mosaic in the pseudo-locale (%s)", async (mode) => {
  const games = [webGame("alpha"), webGame("beta", { favorite: true }), webGame("gamma")];
  await open("/library?filter=web", { mode, games });
  await waitFor(() =>
    expect(all('[data-view="library"] [data-cs="game-card"][data-variant="web"]')).toHaveLength(3),
  );
  expect(shown(['[data-view="library"]'], [/Web (?:alpha|beta|gamma)/g])).toEqual([]);
});

it("shows a game's quick look in the pseudo-locale", async () => {
  await open("/library?filter=all&sort=title");
  const [link] = await screen.findAllByRole("link", { name: /Fixture Game 0002/ });
  if (!link) throw new Error("card missing");
  fireEvent.click(link);
  await ready('[data-cs="quick-look"] [data-part="actions"]');
  await waitFor(() =>
    expect(one('[data-cs="quick-look"] [data-part="title"]')?.textContent).toMatch(/Fixture Game/),
  );
  expect(shown(['[data-cs="quick-look"]'])).toEqual([]);
});

// A game's page.

const lantern = fixtureCatalog.find((game) => game.id === "1001") as IgdbGame;

/** The game's IGDB match: the catalog's Lantern Tide, with its ratings. */
const gameIgdb: GameIgdbApi = {
  game: async (gameId) => ({
    gameId,
    igdbId: lantern.id,
    source: "store",
    pending: false,
    game: structuredClone(lantern),
  }),
  set: async () => Promise.reject(new Error("unexpected set")),
  rematch: async () => Promise.reject(new Error("unexpected rematch")),
  refresh: async () => Promise.reject(new Error("unexpected refresh")),
  search: async () => [],
  separate: async () => Promise.reject(new Error("unexpected separate")),
  ageRatings: async () => ({ ratings: structuredClone(lantern.ageRatings), minAge: 16 }),
};

const achievementSets = () => [steamSet(12), staleGogSet(), retroSet()];

/** What the IGDB match and the achievements name. */
function gameData(): (RegExp | string)[] {
  return [
    ...strings([
      lantern.title,
      ...lantern.genres,
      ...lantern.platforms.flatMap((platform) => [platform.name, platform.abbreviation]),
      ...lantern.videos.map((video) => video.name),
      ...achievementSets().flatMap((set) => [
        set.account.providerName,
        set.account.displayName,
        set.sync.error?.summary,
        ...set.achievements.flatMap((item) => [item.definition.name, item.definition.description]),
      ]),
    ]),
    /PEGI|ESRB/g,
    // Other devices' copies and games running there.
    devices,
  ];
}

it.each(modes)("shows a game's page in the pseudo-locale (%s)", async (mode) => {
  await open("/games/fixture-1", {
    mode,
    wrap: (children: ReactNode) => (
      <GameIgdbProvider api={gameIgdb}>
        <AchievementsProvider api={createAchievementsFixture(() => achievementSets())}>
          {children}
        </AchievementsProvider>
      </GameIgdbProvider>
    ),
  });
  await screen.findByRole("heading", { level: 1, name: "Fixture Game 0002" });
  // The IGDB match, the age rating and the achievements have loaded.
  await waitFor(() => expect(one('[data-view="game"]')?.textContent).toContain("Lantern Tide"));
  await waitFor(() => expect(one('[data-view="game"]')?.textContent).toContain("PEGI 16"));
  await ready('[data-view="game"] [data-cs="achievements"]');
  await ready('[data-view="game"] [data-cs="videos"]');
  expect(shown(['[data-view="game"]'], gameData())).toEqual([]);
});

// The Store.

/** What the fixture catalog names: titles, descriptions, genres,
 * platforms, release dates as IGDB words them, stores and links. */
function catalogData(): (RegExp | string)[] {
  return [
    ...strings(
      fixtureCatalog.flatMap((game) => [
        game.title,
        game.description,
        game.status,
        ...game.genres,
        ...game.platforms.flatMap((platform) => [platform.name, platform.abbreviation]),
        ...game.releaseDates.flatMap((release) => [
          release.human,
          release.platform?.name,
          release.platform?.abbreviation,
        ]),
        ...game.storeLinks.flatMap((link) => [link.name, ...link.platforms]),
        ...game.websites.map((site) => site.name),
        ...game.videos.map((video) => video.name),
      ]),
    ),
    /PEGI|ESRB/g,
    /https?:\/\/[^\s,]+/g,
  ];
}

function openStore(path: string, mode: Mode) {
  // The fixture catalog's dates are relative to this day.
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date("2026-10-30T12:00:00Z"));
  return open(path, {
    mode,
    wrap: (children: ReactNode) => (
      <CatalogProvider api={createCatalogFixtureApi()}>{children}</CatalogProvider>
    ),
  });
}

const storeResults = () => all('[data-catalog="results"] [data-cs="game-card"]');

it.each(modes)("shows the Store in the pseudo-locale (%s)", async (mode) => {
  await openStore("/store", mode);
  await waitFor(() => expect(storeResults()).toHaveLength(24));
  // The release shelves beside the results.
  await waitFor(() =>
    expect(all('[data-view="store"] [data-part="view-all"]').length).toBeGreaterThan(0),
  );
  expect(shown(['[data-view="store"]'], catalogData())).toEqual([]);
});

it.each(modes)("shows the Store's upcoming releases in the pseudo-locale (%s)", async (mode) => {
  await openStore("/store?tab=upcoming", mode);
  await waitFor(() => expect(storeResults()).toHaveLength(3));
  expect(shown(['[data-view="store"]'], catalogData())).toEqual([]);
});

it("shows the Store's empty search in the pseudo-locale", async () => {
  await openStore("/store?q=nothing-like-it", "desktop");
  await ready('[data-view="store"] [data-part="empty"]');
  expect(shown(['[data-view="store"]'], [...catalogData(), "nothing-like-it"])).toEqual([]);
});

it.each(modes)("shows a catalog game's page in the pseudo-locale (%s)", async (mode) => {
  await openStore("/store/games/1001", mode);
  await screen.findByRole("heading", { level: 1, name: "Lantern Tide" });
  // Its similar games, the last part to load.
  await waitFor(() =>
    expect(all('[data-cs="shelf-row"] [data-cs="game-card"]').length).toBeGreaterThan(1),
  );
  expect(shown(['[data-view="store-game"]'], catalogData())).toEqual([]);
});

// Collections.

/** The fixture collections' names and descriptions. */
const collectionData = [
  "Favourites",
  "The ones worth coming back to.",
  "Weekend Picks",
  "Short list for when there's finally time.",
];

it.each(modes)("shows Collections in the pseudo-locale (%s)", async (mode) => {
  await open("/collections", { mode });
  await waitFor(() => expect(all('[data-cs="collection-card"]').length).toBeGreaterThan(1));
  expect(shown(['[data-view="collections"]'], collectionData)).toEqual([]);
});

it.each(modes)("shows a manual collection in the pseudo-locale (%s)", async (mode) => {
  await open("/collections/collection-weekend", { mode });
  await screen.findByRole("heading", { level: 1, name: "Weekend Picks" });
  await waitFor(() =>
    expect(all('[data-view="collection"] [data-cs="game-card"]').length).toBeGreaterThan(0),
  );
  expect(shown(['[data-view="collection"]'], collectionData)).toEqual([]);
});

it.each(modes)("shows a smart collection in the pseudo-locale (%s)", async (mode) => {
  await open("/collections/collection-favourites", { mode });
  await screen.findByRole("heading", { level: 1, name: "Favourites" });
  await ready('[data-view="collection"] [data-part="rule"]');
  await waitFor(() =>
    expect(all('[data-view="collection"] [data-cs="game-card"]').length).toBeGreaterThan(0),
  );
  expect(shown(['[data-view="collection"]'], collectionData)).toEqual([]);
});

// Activity.

it("shows Activity's jobs in the pseudo-locale", async () => {
  const job = fixtureJob();
  await open("/activity");
  await waitFor(() =>
    expect(one('[data-view="activity"]')?.textContent).toContain("Fixture installation"),
  );
  // The job's own words are the core's (#214).
  const data = strings([
    job.title,
    job.message,
    job.targetDir,
    ...(job.warnings ?? []).map((warning) => warning.message),
    ...(job.dependencies ?? []).map((dependency) => dependency.name),
    ...(job.strategies ?? []).flatMap((strategy) => [strategy.label, strategy.reason]),
  ]);
  expect(shown(['[data-view="activity"]'], [...data, ...paths])).toEqual([]);
});

it("shows Activity's notifications in the pseudo-locale", async () => {
  await open("/activity?tab=notifications", {
    wrap: (children: ReactNode) => (
      <NotificationsProvider api={createNotificationsFixtureApi()}>
        {children}
      </NotificationsProvider>
    ),
  });
  await waitFor(() =>
    expect(one('[data-view="activity"]')?.textContent).toContain("10 games added"),
  );
  // Titles and bodies are the core's (#214).
  const data = strings(
    fixtureNotifications().flatMap((item) => [
      item.title,
      item.body,
      ...item.sources.map((source) => source.source),
    ]),
  );
  expect(shown(['[data-view="activity"]'], data)).toEqual([]);
});

// Settings tabs the settings tests leave out.

const settings = () => ['[data-view="settings"]', '[role="tabpanel"]'];

it("shows Settings → Scripts in the pseudo-locale", async () => {
  await open("/settings?tab=scripts");
  await ready('[role="tabpanel"] [data-cs="scripts"] [data-part="order"]');
  // Placeholders and variables are written as code.
  expect(shown(settings(), [/\{\w+\}/g, /CASKADE_\w+/g], "code")).toEqual([]);
});

it("shows Settings → Store accounts in the pseudo-locale", async () => {
  await open("/settings?tab=stores", {
    wrap: (children: ReactNode) => (
      <StoreAccountsProvider api={createStoreAccountsFixtureApi()}>
        {children}
      </StoreAccountsProvider>
    ),
  });
  await waitFor(() => expect(one('[role="tabpanel"]')?.textContent).toContain("Old Origin"));
  await waitFor(() => expect(one('[role="tabpanel"]')?.textContent).toContain("Ada Epic"));
  // Stores, accounts and the core's words for what went wrong (#214).
  const data = [
    ...strings([
      ...fixtureProviders.map((provider) => provider.name),
      ...fixtureAccounts().flatMap((summary) => [
        summary.account.displayName,
        summary.account.sync.error?.summary,
      ]),
    ]),
    /\borigin\b/g,
    /Steam|Epic Games|GOG|Lutris|itch\.io/g,
  ];
  expect(shown(settings(), data)).toEqual([]);
});

it("shows Settings → Streaming while hosting in the pseudo-locale", async () => {
  const streaming = createStreamingFixtureApi({
    hosting: { state: "ready", version: "2026.914.233613", problem: null, warning: null },
    requests: [{ id: "r1", name: "Steam Deck", address: "192.168.1.40" }],
    paired: [
      { uuid: "u1", name: "Deck" },
      { uuid: "u2", name: "Bedroom TV" },
    ],
  });
  await open("/settings?tab=streaming", {
    wrap: (children: ReactNode) => (
      <StreamingApiProvider api={streaming}>{children}</StreamingApiProvider>
    ),
  });
  await ready('[role="tabpanel"] [data-cs="streaming-settings"]');
  await waitFor(() => {
    const text = one('[role="tabpanel"]')?.textContent ?? "";
    for (const name of ["Living room", "Study", "Bedroom TV", "Steam Deck"])
      expect(text).toContain(name);
  });
  const data = [
    ...strings(fixtureStreamHosts().flatMap((host) => [host.name, host.address])),
    devices,
    /\d+\.\d+\.\d+\.\d+(?::\d+)?/g,
    /Sunshine|Moonlight/g,
  ];
  expect(shown(settings(), data)).toEqual([]);
});

it("shows Settings → Backup in the pseudo-locale", async () => {
  const destination = "/home/player/.local/share/caskade/backups";
  const backup = createBackupFixtureApi({
    last: {
      atMs: Date.UTC(2026, 9, 30, 9, 30),
      automatic: true,
      path: `${destination}/caskade-backup-20261030-093000.zip`,
      bytes: 71 * 1024 * 1024,
      error: null,
    },
  });
  await open("/settings?tab=backup", {
    wrap: (children: ReactNode) => <BackupProvider api={backup}>{children}</BackupProvider>,
  });
  await ready(`[role="tabpanel"] [placeholder="${destination}"]`);
  // The last backup's outcome, with its size.
  await waitFor(() => expect(one('[role="tabpanel"]')?.textContent).toContain("71.0 MB"));
  expect(shown(settings(), [...paths, devices])).toEqual([]);
});
