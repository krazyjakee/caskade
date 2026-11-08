/** The views' own content in the pseudo-locale (#9): Home, the Library's
 * filters (populated, empty, by platform and the Web mosaic) in both
 * layouts and its quick look, a game's page, the Store and a catalog game's
 * page, Collections and a collection, Activity's jobs and notifications,
 * and the Settings tabs the settings tests leave out (Scripts, Store
 * accounts, Streaming, Backup). Then what opens over them: console mode's
 * quick look, a game's menu, Add game and its dialogs, the game editor's
 * tabs, the move, uninstall and remove dialogs, and what the app draws
 * around a plugin's tabs and views. Then what stands in for them or comes up
 * on its own: signing in, the guide menu and the curtain, every question
 * Play can ask, install requests, selection and the bulk editor, the search
 * overlays, pairing, and what shows when something fails. Views that differ
 * by shell are checked in both. Every text, aria-label, title and
 * placeholder they show comes from a catalog; what the fixtures name is
 * data. Each view is checked once its data has loaded: a check that may pass
 * early would pass on a view still loading. */
import { i18n, loadCatalog, pseudoLocale } from "@caskade/i18n";
import type { PluginContext } from "@caskade/sdk";
import { I18nProvider } from "@lingui/react";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { FixtureAccount } from "../api/accountsFixtures";
import { AchievementsProvider } from "../api/achievements";
import {
  createAchievementsFixture,
  retroSet,
  staleGogSet,
  steamSet,
} from "../api/achievementsFixtures";
import { fixtureCatalogueItems } from "../api/addonsFixtures";
import { BackupProvider } from "../api/backup";
import { createBackupFixtureApi } from "../api/backupFixtures";
import { BrowseApiContext } from "../api/browse";
import { createBrowseFixtureApi } from "../api/browseFixtures";
import { createCatalogFixtureApi, fixtureCatalog } from "../api/catalogFixtures";
import { createEmulationFixtureApi } from "../api/emulationFixtures";
import { createEmulatedFixtureGames, createFixtureGames } from "../api/fixtures";
import { type GameEmulation, GameEmulationProvider } from "../api/gameEmulation";
import { CatalogProvider, type GameIgdbApi, GameIgdbProvider, type IgdbGame } from "../api/igdb";
import type { JobQuestion } from "../api/install";
import { fixtureJob } from "../api/installFixtures";
import type { InstallRequest, InstallRequestApi } from "../api/installRequests";
import { NotificationsProvider } from "../api/notifications";
import { createNotificationsFixtureApi, fixtureNotifications } from "../api/notificationsFixtures";
import type { BeforePlay, PlayingElsewhere, SaveVersion } from "../api/saves";
import { FIXTURE_LINK, fixtureShareInfo } from "../api/sharesFixtures";
import { createStorageFixtureApi } from "../api/storageFixtures";
import { StoreAccountsProvider } from "../api/storeAccounts";
import {
  createStoreAccountsFixtureApi,
  fixtureAccounts,
  fixtureProviders,
} from "../api/storeAccountsFixtures";
import { StreamingApiProvider } from "../api/streaming";
import { createStreamingFixtureApi, fixtureStreamHosts } from "../api/streamingFixtures";
import { type SyncApi, SyncProvider, type Transfer } from "../api/sync";
import { createSyncFixtureApi } from "../api/syncFixtures";
import type { GameDetails, LaunchPrompt } from "../api/types";
import { type GameUpdate, type GameUpdatesApi, GameUpdatesProvider } from "../api/updates";
import { InputProvider } from "../input";
import { type GuideApi, GuideMenu, type RunningGame } from "../input/GuideMenu";
import { PluginRegistry } from "../plugins/registry";
import type { UiPlugin } from "../plugins/types";
import type { Mode } from "../shell/console/mode";
import { AppErrorBoundary } from "../shell/ErrorFallback";
import { InstallIngress } from "../views/activity/InstallIngress";
import { presentPlatform } from "../views/library/platforms/catalog";
import {
  type AppOptions,
  fakePad,
  fakeViewport,
  focused,
  playedGames,
  renderApp,
  until,
} from "./harness";
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

// What opens over the views. These sit in portals, outside every view.

/** Menus and dialogs, and console mode's hints for them. */
const overlays = [
  '[role="menu"]',
  '[role="dialog"]',
  '[role="alertdialog"]',
  '[data-cs="hint-bar"]',
];

/** Not Caskade's text. Pad buttons keep the names printed on them
 * (`glyphs.ts`), and these come from React Aria's own catalogs, which
 * `LocaleProvider` gives the app's language (`ariaLocale.test.tsx`) but
 * which have no pseudo-locale: an empty Select, a search box's list and a
 * popover's hidden dismiss button. */
const padGlyphs = '[data-part="glyph"]';
const reactAria = ["Select an item", "Suggestions", "Dismiss"];

const over = (data: (RegExp | string)[] = []) =>
  shown(overlays, [...data, ...reactAria], padGlyphs);

const rightClick = (element: Element) =>
  fireEvent.contextMenu(element, { clientX: 120, clientY: 80, button: 2 });

/** The open menu's item named `name`, as pseudo text. */
const menuItem = async (name: RegExp) =>
  within(await screen.findByRole("menu")).findByRole("menuitem", { name });

/** The game the storage fixture installed, and one with no entries. */
function managedGames(): GameDetails[] {
  return createFixtureGames(2).map((game, index) =>
    index === 1
      ? {
          ...game,
          status: "installed",
          entries: [
            {
              id: "entry-1",
              source: "caskade-storage",
              state: "installed",
              location: "/fixture/Games/Fixture Game 0002",
              canLaunch: true,
              emulated: false,
            },
          ],
        }
      : game,
  );
}

const managed = (mode: Mode = "desktop") =>
  open("/games/fixture-1", { mode, games: managedGames(), storage: createStorageFixtureApi() });

const gameHeading = () => screen.findByRole("heading", { level: 1, name: "Fixture Game 0002" });

// Console mode's quick look.

it("shows console mode's quick look from the pad in the pseudo-locale", async () => {
  const app = await open("/library?filter=installed&sort=title", {
    mode: "console",
    games: playedGames(8),
  });
  await until(() => Boolean(focused()?.closest('[data-cs="game-card"]')), "the first game");
  app.pad.press("details");
  await ready('[data-cs="quick-look"] [data-part="facts"]');
  await ready('[data-cs="hint-bar"] [data-part="hint"]');
  expect(over()).toEqual([]);
});

// A game's menu.

/** Opens the managed game's menu and waits for the items that need its
 * details and the other devices. */
async function gameMenu() {
  rightClick(await gameHeading());
  await menuItem(/Ĥĩďē ƒŕōḿ ĺĩƀŕàŕŷ/);
  await menuItem(/Śĥàŕē ŵĩţĥ śōḿēōńē/);
}

/** Opens the game menu's `item`. */
async function fromMenu(item: RegExp) {
  await gameMenu();
  fireEvent.click(await menuItem(item));
}

const dialogData = ["/fixture/Games/Fixture Game 0002", ...paths, devices];

it.each(modes)("shows a game's menu in the pseudo-locale (%s)", async (mode) => {
  await managed(mode);
  await gameMenu();
  expect(over(dialogData)).toEqual([]);
});

it.each(modes)("shows a running game's menu in the pseudo-locale (%s)", async (mode) => {
  const app = await managed(mode);
  await gameHeading();
  await app.library.launchGame("entry-1");
  await gameMenu();
  await menuItem(/Śţōƥ/);
  expect(over(dialogData)).toEqual([]);
});

it("shows a web game's menu and its Remove dialog in the pseudo-locale", async () => {
  await open("/games/alpha", { games: [webGame("alpha")], storage: createStorageFixtureApi() });
  rightClick(await screen.findByRole("heading", { level: 1, name: "Web alpha" }));
  await menuItem(/Ƥĺàŷ ōƒƒĺĩńē/);
  expect(over([/Web alpha/g])).toEqual([]);
  fireEvent.click(await menuItem(/Ŕēḿōvē…/));
  await screen.findByRole("alertdialog");
  expect(over([/Web alpha/g])).toEqual([]);
});

// The dialogs a game's menu opens.

it.each(modes)("shows the move dialog in the pseudo-locale (%s)", async (mode) => {
  const roots = await createStorageFixtureApi().list();
  // Another location to go to, none, and a game Caskade didn't install.
  for (const list of [roots, roots.slice(0, 1), []]) {
    await open("/games/fixture-1", {
      mode,
      games: managedGames(),
      storage: { ...createStorageFixtureApi(), list: async () => structuredClone(list) },
    });
    await fromMenu(/Ḿōvē ĩńśţàĺĺēď ƒĩĺēś/);
    await ready('[role="dialog"] [data-cs="select"]');
    expect(over([...dialogData, /\bExternal\b/g])).toEqual([]);
    cleanup();
  }
});

it.each(modes)("shows the uninstall dialog in the pseudo-locale (%s)", async (mode) => {
  await managed(mode);
  await fromMenu(/Ũńĩńśţàĺĺ…/);
  await screen.findByRole("alertdialog");
  expect(over(dialogData)).toEqual([]);
});

it.each(modes)("shows the remove dialog in the pseudo-locale (%s)", async (mode) => {
  // With Caskade's install to delete, and without.
  for (const source of ["caskade-storage", "manual"]) {
    const games = managedGames().map((game) => ({
      ...game,
      entries: game.entries.map((entry) => ({ ...entry, source })),
    }));
    await open("/games/fixture-1", { mode, games, storage: createStorageFixtureApi() });
    rightClick(await gameHeading());
    fireEvent.click(await menuItem(/Ŕēḿōvē ƒŕōḿ ĺĩƀŕàŕŷ/));
    await screen.findByRole("alertdialog");
    expect(over(dialogData)).toEqual([]);
    cleanup();
  }
});

it.each(modes)("shows the collection picker in the pseudo-locale (%s)", async (mode) => {
  await managed(mode);
  await fromMenu(/Àďď ţō ćōĺĺēćţĩōń/);
  await ready('[role="dialog"] [role="checkbox"], [role="dialog"] input[type="checkbox"]');
  expect(over([...dialogData, ...collectionData])).toEqual([]);
});

it.each(modes)("shows Share with someone in the pseudo-locale (%s)", async (mode) => {
  await managed(mode);
  await fromMenu(/Śĥàŕē ŵĩţĥ śōḿēōńē/);
  const dialog = await screen.findByRole("dialog");
  expect(over(dialogData)).toEqual([]);
  // The link it makes.
  fireEvent.click(await within(dialog).findByRole("button", { name: /Ḿàķē à ĺĩńķ/ }));
  await ready('[data-cs="share-game"] [data-part="link"]');
  expect(over([/caskade:\/\/\S+/g, ...dialogData])).toEqual([]);
});

it("says a game didn't start in the pseudo-locale", async () => {
  const app = await managed();
  vi.spyOn(app.library, "launchGame").mockRejectedValue(new Error("The emulator is missing"));
  await fromMenu(/Ƥĺàŷ/);
  await screen.findByRole("alertdialog");
  // The reason is the core's (#214).
  expect(over([...dialogData, "The emulator is missing"])).toEqual([]);
});

// The game editor.

it.each(modes)("shows the game editor's tabs in the pseudo-locale (%s)", async (mode) => {
  const app = await managed(mode);
  const location = { kind: "folder", path: "/home/me/.local/share/fixture" } as const;
  const found = [{ spec: location, tags: ["save"], fileCount: 2 }];
  app.library.saves.seed({
    entryId: "entry-1",
    revision: 1,
    preferences: { manifestTitle: null, locations: null },
    matched: { type: "matched", title: "Fixture Game", method: "steam" },
    automatic: found,
    locations: found,
    warnings: [],
  });
  await fromMenu(/Ēďĩţ…/);
  // Every tab stays mounted, hidden, so all five are in the dialog at once.
  await ready('[data-cs="game-editor"] [data-part="field"]');
  await ready('[data-cs="game-editor"] [data-part="artwork"] [data-part="slot"]');
  await ready('[data-cs="game-editor"] [data-cs="game-actions"] [data-part="action"]');
  await ready('[data-cs="game-editor"] [data-cs="scripts"] [data-part="order"]');
  await ready('[data-cs="game-editor"] [data-cs="game-saves"] [data-part="location"]');
  // The built-in action's name is the core's (#214), as is the save match's;
  // Proton builds are named by their folders.
  const builds = await app.library.actions.protonBuilds();
  const data = [
    ...dialogData,
    /CASKADE_\w+/g,
    /\{\w+\}/g,
    /^Play$/,
    "Fixture Game",
    ...builds.installed,
  ];
  expect(over(data)).toEqual([]);

  // Unsaved work asks first.
  const title = one('[data-cs="game-editor"] input') as HTMLInputElement;
  fireEvent.change(title, { target: { value: "Changed" } });
  fireEvent.click(one('[data-cs="game-editor"] [data-part="cancel"]') as Element);
  await ready('[data-cs="game-editor-discard"]');
  expect(over(data)).toEqual([]);
});

// Add game.

async function openAddGame() {
  await ready('[data-cs="add-game"] button');
  fireEvent.click(one('[data-cs="add-game"] button') as Element);
}

/** Opens one of Add game's items. */
async function addGame(item: RegExp) {
  await openAddGame();
  fireEvent.click(await menuItem(item));
  return screen.findByRole("dialog");
}

const addGameTo = (mode: Mode) =>
  open("/library?filter=all&sort=title", { mode, storage: createStorageFixtureApi() });

const type = (field: Element | null, value: string) =>
  fireEvent.change(field as Element, { target: { value } });

it.each(modes)("shows Add game's menu in the pseudo-locale (%s)", async (mode) => {
  await addGameTo(mode);
  await openAddGame();
  await menuItem(/ƀŷ ţĩţĺē/);
  expect(over()).toEqual([]);
});

it.each(modes)(
  "shows Install from link and its storage in the pseudo-locale (%s)",
  async (mode) => {
    await addGameTo(mode);
    const dialog = await addGame(/ĺĩńķ…/);
    expect(over()).toEqual([]);
    type(dialog.querySelector("input"), "https://example.com/game.zip");
    fireEvent.submit(dialog.querySelector("form") as Element);
    await ready('[data-cs="install-root-picker"] input[type="radio"]');
    expect(over([...paths, /\b(?:Games|External)\b/g, /\bext4\b/g])).toEqual([]);
  },
);

it.each(modes)("shows Add a game on this computer in the pseudo-locale (%s)", async (mode) => {
  await addGameTo(mode);
  const dialog = await addGame(/ţĥĩś ćōḿƥũţēŕ/);
  expect(over()).toEqual([]);
  const [target, title] = dialog.querySelectorAll("input");
  // A shortcut, so the summary has all its rows, under a title already there.
  type(target ?? null, "/home/me/Desktop/celeste.desktop");
  fireEvent.blur(target as Element);
  await ready('[data-cs="manual-game"] [data-part="summary"]');
  type(title ?? null, "Fixture Game 0001");
  fireEvent.click(one('[data-cs="manual-game"] [data-part="add"]') as Element);
  await ready('[data-cs="manual-game"] [data-part="duplicate"]');
  expect(over([...paths, "--windowed"])).toEqual([]);
});

it.each(modes)("shows Add a game by title in the pseudo-locale (%s)", async (mode) => {
  await addGameTo(mode);
  await addGame(/ƀŷ ţĩţĺē/);
  expect(over()).toEqual([]);
});

it.each(modes)("shows Receive a shared game in the pseudo-locale (%s)", async (mode) => {
  await addGameTo(mode);
  const dialog = await addGame(/Ŕēćēĩvē à śĥàŕēď ĝàḿē/);
  expect(over()).toEqual([]);
  type(dialog.querySelector("input"), `${FIXTURE_LINK}hades`);
  fireEvent.click(await within(dialog).findByRole("button", { name: /Ĺōōķ/ }));
  await within(dialog).findByRole("button", { name: /Ŕēćēĩvē/ });
  expect(over([/Alex's Desk|Hades/g, /caskade:\/\/\S+/g])).toEqual([]);
});

it("shows the on-screen keyboard over Add game's field in the pseudo-locale", async () => {
  const app = await addGameTo("console");
  const dialog = await addGame(/ĺĩńķ…/);
  const field = dialog.querySelector("input");
  await until(() => focused() === field, "the link field");
  app.pad.press("confirm");
  await ready('[data-cs="osk"]');
  await ready('[data-cs="hint-bar"] [data-part="hint"][data-action="back"]');
  expect(shown(['[data-cs="osk"]', ...overlays], reactAria, padGlyphs)).toEqual([]);
});

it("shows console mode's folder browser from Add game in the pseudo-locale", async () => {
  await open("/library?filter=all&sort=title", {
    mode: "console",
    wrap: (children: ReactNode) => (
      <BrowseApiContext value={createBrowseFixtureApi()}>{children}</BrowseApiContext>
    ),
  });
  await addGame(/ƒōĺďēŕ…/);
  await ready('[data-cs="folder-browser"] [data-part="entry"]');
  // Files' names, and places the core names (#214).
  expect(over([/^(?:Home|File system|SD Card|Downloads|Emulators|Games|ROMs)$/, ...paths])).toEqual(
    [],
  );
});

// Plugins: their own words are theirs (a language they don't ship shows
// their English); what the app draws around them is not.

const hours: UiPlugin = {
  id: "dev.fixture.hours",
  name: "Hours",
  version: "0.1.0",
  entry: "caskade-plugin://dev.fixture.hours/ui.js",
  contributes: [
    { slot: "game.details.tabs", id: "hours", title: "How long" },
    { slot: "game.details.tabs", id: "broken", title: "Broken" },
    { slot: "game.actions", id: "repack", title: "Repack" },
    { slot: "views", id: "shelf", title: "Shelf" },
    { slot: "views", id: "couch", title: "Couch", modes: ["console"] },
    { slot: "views", id: "desk", title: "Desk", modes: ["desktop"] },
  ],
};

const pluginData = [/Hours|How long|Broken|Repack|Shelf|Couch|Desk|Twelve hours|no pack/g];

const HoursTab = () => <p>Twelve hours</p>;

function Broken(): ReactNode {
  throw new Error("broken on purpose");
}

function pluginRegistry() {
  const context = { plugin: { id: hours.id, name: hours.name } } as unknown as PluginContext;
  const registry = new PluginRegistry();
  registry.register(
    hours,
    "game.details.tabs",
    { id: "hours", title: "How long", component: HoursTab },
    context,
  );
  registry.register(
    hours,
    "game.details.tabs",
    { id: "broken", title: "Broken", component: Broken },
    context,
  );
  const run = () => Promise.reject(new Error("no pack"));
  registry.register(hours, "game.actions", { id: "repack", title: "Repack", run }, context);
  for (const [id, title] of [
    ["shelf", "Shelf"],
    ["couch", "Couch"],
    ["desk", "Desk"],
  ] as const)
    registry.register(hours, "views", { id, title, component: () => <p>{title}</p> }, context);
  return registry;
}

const pluginShown = (selectors: string[]) => shown(selectors, pluginData);

it.each(modes)("shows a plugin's tab on a game's page in the pseudo-locale (%s)", async (mode) => {
  // The crashed tab's placeholder logs what crashed.
  vi.spyOn(console, "error").mockImplementation(() => {});
  await open("/games/fixture-1", { mode, registry: pluginRegistry() });
  await ready('[data-part="plugin-details"] [role="tabpanel"]');
  const around = ['[data-part="plugin-details"]', '[data-part="plugin-nav"]'];
  expect(pluginShown(around)).toEqual([]);

  const tabs = all('[data-part="plugin-tabs"] [role="tab"]');
  fireEvent.click(tabs[1] as Element);
  await ready('[data-cs="plugin-error"]');
  expect(pluginShown(around)).toEqual([]);

  // An action that fails says so in the app's words.
  fireEvent.click(one('[data-part="plugin-actions"] button') as Element);
  await ready('[data-cs="plugin-notices"] [role="alert"]');
  expect(pluginShown(['[data-cs="plugin-notices"]'])).toEqual([]);
});

it.each(modes)("shows a plugin's view in the pseudo-locale (%s)", async (mode) => {
  // Its own, one the other mode hides, and one that isn't there.
  const hidden = mode === "console" ? "desk" : "couch";
  for (const view of ["shelf", hidden, "gone"]) {
    await open(`/plugins/${hours.id}/${view}`, { mode, registry: pluginRegistry() });
    await ready(view === "shelf" ? '[data-view="plugin"]' : 'main [role="status"]');
    expect(pluginShown(["main"])).toEqual([]);
    cleanup();
  }
});

// Whole windows: the shells around a view.

it.each(modes)("shows the whole window in the pseudo-locale (%s)", async (mode) => {
  await open("/", { mode, games: playedGames(8) });
  const shell = `[data-cs="${mode === "console" ? "console-shell" : "app-shell"}"]`;
  await ready(`${shell} [data-view="home"] [data-cs="game-card"]`);
  if (mode === "desktop")
    await waitFor(() => expect(one(shell)?.textContent).toContain("Fixture installation"));
  expect(shown([shell], ["Fixture installation", /\bPlayer\b/g], padGlyphs)).toEqual([]);
});

// Signing in.

const family: FixtureAccount[] = [
  { id: "ada", name: "Ada", role: "admin", avatar: "avatar:rocket", pin: "2468" },
  { id: "kid", name: "Kid", avatar: "avatar:moon" },
];

const picker = '[data-cs="account-picker"]';
const pinPad = '[data-cs="pin-pad"]';

/** Accounts' names, as their tiles show them. */
const signIn = () => shown([picker], [/\b(?:Ada|Kid)\b/g], padGlyphs);

const enterPin = (pin: string) => {
  const field = one(`${pinPad} [data-part="field"]`) as HTMLInputElement;
  fireEvent.change(field, { target: { value: pin } });
  fireEvent.submit(field.closest("form") as Element);
};

it.each(modes)("shows who's playing and the PIN pad in the pseudo-locale (%s)", async (mode) => {
  await open("/", { mode, accounts: family });
  await ready(`${picker} [data-part="account"]`);
  if (mode === "console") await ready(`${picker} [data-cs="hint-bar"] [data-part="hint"]`);
  expect(signIn()).toEqual([]);

  fireEvent.click(one(`${picker} [data-part="account"][data-status="locked"]`) as Element);
  await ready(pinPad);
  if (mode === "console") await ready(`${picker} [data-cs="hint-bar"] [data-part="hint"]`);
  expect(signIn()).toEqual([]);

  // Too short, then wrong.
  enterPin("12");
  await waitFor(() => expect(one(`${pinPad} [data-part="message"]`)?.textContent).not.toBe(""));
  expect(signIn()).toEqual([]);
  enterPin("1357");
  await waitFor(() => expect(one(pinPad)?.getAttribute("data-status")).toBe("wrong"));
  expect(signIn()).toEqual([]);
});

it("shows a locked PIN pad in the pseudo-locale", async () => {
  await open("/", {
    accounts: [
      { ...(family[0] as FixtureAccount), lockedUntilMs: Date.now() + 90_000 },
      family[1] as FixtureAccount,
    ],
  });
  await ready(`${picker} [data-part="account"]`);
  fireEvent.click(one(`${picker} [data-part="account"][data-status="locked"]`) as Element);
  await waitFor(() => expect(one(pinPad)?.getAttribute("data-status")).toBe("locked"));
  expect(signIn()).toEqual([]);
});

// Console mode's curtain and guide menu.

it("shows the curtain over a running game in the pseudo-locale", async () => {
  const app = await open("/games/fixture-1", { mode: "console" });
  await gameHeading();
  await app.library.launchGame("entry-1");
  await waitFor(() =>
    expect(one('[data-cs="game-curtain"]')?.getAttribute("data-state")).toBe("covered"),
  );
  await waitFor(() =>
    expect(one('[data-cs="game-curtain"] [data-part="title"]')?.textContent).toMatch(/Fixture/),
  );
  // The guide button's name is printed on the pad, as in the hint bar.
  expect(shown(['[data-cs="game-curtain"]', '[role="dialog"]'], [], "kbd")).toEqual([]);
});

it("shows the curtain before a game's title is known in the pseudo-locale", async () => {
  const app = await open("/", { mode: "console" });
  await ready('[data-cs="game-curtain"]');
  act(() =>
    app.library.emit({
      type: "game.started",
      sessionId: "s1",
      entryId: "gone",
      gameId: "not-in-the-library",
    }),
  );
  await ready('[data-cs="game-curtain"] [data-part="title"]');
  expect(shown(['[data-cs="game-curtain"]'], [], "kbd")).toEqual([]);
});

/** The guide menu, as the shell mounts it, for `game`. */
async function guide(game: RunningGame, stop?: () => Promise<void>) {
  i18n.loadAndActivate({ locale, messages: await loadCatalog(locale) });
  const pad = fakePad();
  const api: GuideApi = {
    running: async () => [game],
    returnToGame: async () => {},
    stop: stop ?? (async () => {}),
  };
  render(
    <I18nProvider i18n={i18n}>
      <InputProvider console onBack={() => {}} subscribe={pad.subscribe}>
        <GuideMenu api={api} openDetails={() => {}} />
      </InputProvider>
    </I18nProvider>,
  );
  pad.press("guide");
  await ready('[data-cs="guide-menu"]');
}

const guideButton = (index: number) =>
  all('[data-cs="guide-menu"] button')[index] as HTMLButtonElement;

it.each([false, true])("shows the guide menu in the pseudo-locale (stream: %s)", async (stream) => {
  await guide({ sessionId: "s1", gameId: "fixture-1", title: "Fixture Game 0002", stream }, () =>
    Promise.reject(new Error("The game won't stop")),
  );
  expect(shown(['[data-cs="guide-menu"]'])).toEqual([]);
  // Stop asks first, and may fail. The reason is the core's (#214).
  fireEvent.click(guideButton(1));
  await waitFor(() => expect(all('[data-cs="guide-menu"] button')).toHaveLength(2));
  expect(shown(['[data-cs="guide-menu"]'])).toEqual([]);
  fireEvent.click(guideButton(0));
  await ready('[data-cs="guide-menu"] [role="alert"]');
  expect(shown(['[data-cs="guide-menu"]'], ["The game won't stop"])).toEqual([]);
});

it("shows the Library's alphabet bar in the pseudo-locale", async () => {
  await open("/library?filter=all&sort=title", { games: createFixtureGames(60) });
  await ready('[data-cs="alphabet-bar"] [data-part="letter"]');
  expect(shown(['[data-cs="alphabet-bar"]'])).toEqual([]);
});

// What Play asks first.

const play = async () => {
  const [button] = await screen.findAllByRole("button", { name: /Ƥĺàŷ/ });
  fireEvent.click(button as Element);
};

const dialogOf = (name: string) => ready(`[data-cs="${name}"]`);

it.each(modes)("asks how to play in the pseudo-locale (%s)", async (mode) => {
  const prompt: LaunchPrompt = {
    entryId: "entry-1",
    platformId: "nintendo_snes",
    actions: [
      { id: "story", name: "Story", default: true },
      { id: "mods", name: "Mods", default: false },
    ],
    profiles: [
      {
        emulatorId: "ra",
        emulatorName: "RetroArch",
        profileId: "snes",
        profileName: "Snes9x",
        installed: true,
      },
      {
        emulatorId: "bsnes",
        emulatorName: "bsnes",
        profileId: "default",
        profileName: "Accuracy",
        installed: false,
      },
    ],
    discs: [{ index: 0, name: "Disc 1.chd" }],
  };
  const app = await open("/games/fixture-1", { mode });
  await gameHeading();
  const launch = vi.spyOn(app.library, "launchGame");
  launch.mockResolvedValueOnce({ type: "choose", prompt });
  await play();
  await dialogOf("launch-choice");
  const data = [/Story|Mods|RetroArch|Snes9x|bsnes|Accuracy|Disc 1/g, ...paths];
  expect(over(data)).toEqual([]);
  cleanup();

  const discs = await open("/games/fixture-1", { mode });
  await gameHeading();
  vi.spyOn(discs.library, "launchGame").mockResolvedValueOnce({
    type: "choose",
    prompt: { ...prompt, actions: [], profiles: [] },
  });
  await play();
  await dialogOf("launch-choice");
  expect(over(data)).toEqual([]);
});

const update: GameUpdate = {
  entryId: "entry-1",
  gameId: "fixture-1",
  provider: "github",
  project: "owner/game",
  projectUrl: "https://github.com/owner/game/releases",
  installed: "v1.0",
  latest: "v1.1",
  latestUrl: "https://github.com/owner/game/releases/tag/v1.1",
  available: true,
  skipped: null,
  prereleases: false,
  checkedAtMs: Date.UTC(2026, 9, 28),
  error: null,
};

const updatesApi = (offer: GameUpdate): GameUpdatesApi => ({
  game: async () => [offer],
  check: async () => offer,
  beforePlay: async () => offer,
  skip: async () => offer,
  setPrereleases: async () => offer,
  install: async () => "fixture-job",
});

it.each(modes)("offers an update before playing in the pseudo-locale (%s)", async (mode) => {
  // Known installed release, and not known.
  for (const offer of [update, { ...update, installed: null, latestUrl: null }]) {
    await open("/games/fixture-1", {
      mode,
      wrap: (children: ReactNode) => (
        <GameUpdatesProvider api={updatesApi(offer)}>{children}</GameUpdatesProvider>
      ),
    });
    await gameHeading();
    await play();
    await dialogOf("update-offer");
    expect(over([...dialogData])).toEqual([]);
    cleanup();
  }
});

it("follows the update it offered in the pseudo-locale", async () => {
  const app = await open("/games/fixture-1", {
    wrap: (children: ReactNode) => (
      <GameUpdatesProvider api={updatesApi(update)}>{children}</GameUpdatesProvider>
    ),
  });
  await gameHeading();
  await play();
  await dialogOf("update-offer");
  const dialog = one('[data-cs="update-offer"]') as Element;
  fireEvent.click([...all('[data-cs="update-offer"] button')].at(-1) as Element);
  await waitFor(() => expect(dialog.textContent).toContain("40"));
  expect(over()).toEqual([]);
  const states = [
    { status: "needs_user" },
    { status: "cancelled" },
    { status: "failed", error: "The disk is full" },
  ] as const;
  for (const [index, state] of states.entries()) {
    const before = dialog.textContent;
    act(() => app.installs.emit(fixtureJob({ ...state, revision: index + 2 })));
    await waitFor(() => expect(dialog.textContent).not.toBe(before));
    // The job's error is the core's (#214).
    expect(over(["The disk is full"])).toEqual([]);
  }
});

/** A save made on another device, and one made here. */
const version = (id: string, local: boolean, hour: number): SaveVersion => ({
  id,
  gameId: "fixture-1",
  accountId: "",
  deviceId: local ? "here" : "b".repeat(64),
  sessionId: null,
  createdAtMs: Date.UTC(2026, 10, 2, hour),
  contentHash: `sha256:${id}`,
  sizeBytes: 4096,
  fileCount: 1,
  parents: [],
  reason: "session",
  local,
});

const there = version("there", false, 18);
const here = version("here", true, 17);

async function playChecked(mode: Mode, check: BeforePlay) {
  const app = await open("/games/fixture-1", { mode });
  app.library.saves.seedCheck("entry-1", check);
  await gameHeading();
  await play();
  return app;
}

it.each(modes)("asks which save to play with in the pseudo-locale (%s)", async (mode) => {
  await playChecked(mode, {
    check: { type: "conflict", versions: [there, here] },
    checked: true,
    warnings: [],
  });
  await dialogOf("save-conflict");
  expect(over([devices])).toEqual([]);
});

it.each(modes)("waits for a save on its way in the pseudo-locale (%s)", async (mode) => {
  await playChecked(mode, {
    check: { type: "notArrived", version: there },
    checked: true,
    warnings: [],
  });
  await dialogOf("save-arriving");
  expect(over([devices])).toEqual([]);
});

it("says what the save check did in the pseudo-locale", async () => {
  const warning = "A save folder couldn't be read";
  const checks: BeforePlay[] = [
    { check: { type: "restored", version: there }, checked: false, warnings: [warning] },
    { check: { type: "upToDate" }, checked: false, warnings: [] },
  ];
  for (const check of checks) {
    await playChecked("desktop", check);
    await dialogOf("save-note");
    // A warning's words are the core's (#214).
    expect(shown(['[data-cs="save-note"]'], [warning, devices])).toEqual([]);
    cleanup();
  }

  // Skipped the wait: the game started without the newer save.
  await playChecked("desktop", {
    check: { type: "notArrived", version: there },
    checked: true,
    warnings: [],
  });
  await dialogOf("save-arriving");
  fireEvent.click(all('[data-cs="save-arriving"] button')[1] as Element);
  await dialogOf("save-note");
  expect(shown(['[data-cs="save-note"]'], [devices])).toEqual([]);
});

const onDeck: PlayingElsewhere = {
  deviceId: "b".repeat(64),
  deviceName: "Steam Deck",
  gameId: "fixture-1",
  title: "Fixture Game 0002",
  accountId: "",
  startedAtMs: Date.UTC(2026, 10, 2, 19),
};

it.each(modes)("warns about a game running elsewhere in the pseudo-locale (%s)", async (mode) => {
  // A device by its name, and one that has none.
  for (const deviceName of ["Steam Deck", ""]) {
    const app = await open("/games/fixture-1", { mode });
    app.library.saves.setPlaying([{ ...onDeck, deviceName }]);
    await gameHeading();
    await ready('[data-view="game"] [data-part="playing-elsewhere"]');
    expect(shown(['[data-part="playing-elsewhere"]'], [devices])).toEqual([]);
    await play();
    await dialogOf("playing-elsewhere");
    expect(over([devices])).toEqual([]);
    cleanup();
  }
});

/** A game only the living room's device has. */
function streamedGames(): GameDetails[] {
  const games = createFixtureGames(3);
  (games[2] as GameDetails).entries = [
    {
      id: "stream-2",
      source: "stream:fixture-living-room",
      state: "partial",
      location: null,
      canLaunch: true,
      emulated: false,
    },
  ];
  return games;
}

it.each(modes)("asks before stopping a busy device in the pseudo-locale (%s)", async (mode) => {
  // Its game by name, and one it can't name.
  for (const playing of ["Hades", null]) {
    const app = await open("/games/fixture-2", {
      mode,
      games: streamedGames(),
      wrap: (children: ReactNode) => (
        <StreamingApiProvider api={createStreamingFixtureApi()}>{children}</StreamingApiProvider>
      ),
    });
    app.library.busyHosts.set("stream-2", { device: "Living room", playing });
    await screen.findByRole("heading", { level: 1, name: "Fixture Game 0003" });
    fireEvent.click(await screen.findByRole("button", { name: /Śţŕēàḿ ƒŕōḿ/ }));
    await dialogOf("stop-host");
    expect(over([devices, "Hades"])).toEqual([]);
    cleanup();
  }
});

// Install requests. The app mounts these beside the router.

const requestsApi = (pending: InstallRequest[]): InstallRequestApi => ({
  pending: async () => structuredClone(pending),
  request: async () => pending[0] as InstallRequest,
  requestDrop: async () => pending[0] as InstallRequest,
  confirm: async () => "fixture-job",
  confirmCollection: async () => {},
  dismiss: async () => {},
  subscribe: async () => () => {},
});

const withRequests = (api: InstallRequestApi) => (children: ReactNode) => (
  <>
    {children}
    <InstallIngress api={api} />
  </>
);

const dropped = (kind: "shortcut" | "executable" | "folder" | "file"): InstallRequest => ({
  requestId: kind,
  kind: "path",
  value: "/home/me/game",
  plan: {
    kind,
    title: "Fixture Game 0002",
    dropped: "/home/me/game.desktop",
    input: "/home/me/game",
    launch: { path: "/home/me/game/run.sh", args: ["--windowed"], workingDir: null },
  },
});

const peer = (sizeBytes: number | null): InstallRequest => ({
  requestId: "peer",
  kind: "peer",
  value: "Fixture Game 0002",
  peer: {
    deviceId: "b".repeat(64),
    deviceName: "Steam Deck",
    entryId: "entry-1",
    gameId: null,
    title: "Fixture Game 0002",
    sizeBytes,
  },
});

const shared = (share = fixtureShareInfo()): InstallRequest => ({
  requestId: "share",
  kind: "share",
  value: FIXTURE_LINK,
  share,
});

const installRequests: [string, InstallRequest][] = [
  ["a link", { requestId: "url", kind: "url", value: "https://example.com/game.zip" }],
  ["a path", { requestId: "path", kind: "path", value: "/home/me/game.zip" }],
  ["a dropped shortcut", dropped("shortcut")],
  ["a dropped program", dropped("executable")],
  ["a dropped folder", dropped("folder")],
  ["a dropped file", dropped("file")],
  ["a copy from another device", peer(1_200_000_000)],
  ["a copy of unknown size", peer(null)],
  ["a shared game", shared()],
  [
    "a shared save",
    shared(
      fixtureShareInfo({ contents: { kind: "save", game: "Hades", here: "hades", files: 3 } }),
    ),
  ],
  [
    "a shared web game",
    shared(
      fixtureShareInfo({ contents: { kind: "web", url: "https://example.com/game", here: null } }),
    ),
  ],
  [
    "a shared ROM",
    shared(
      fixtureShareInfo({
        contents: { kind: "rom", platform: "Super Nintendo", emulator: "RetroArch", ready: false },
      }),
    ),
  ],
  [
    "a shared collection",
    shared(
      fixtureShareInfo({
        title: "Co-op nights",
        contents: {
          kind: "collection",
          name: "Co-op nights",
          games: [
            { title: "Hades", igdbId: "113112", here: "installed", gameId: "fixture-1" },
            { title: "It Takes Two", igdbId: "119171", here: "missing", gameId: null },
          ],
        },
      }),
    ),
  ],
];

const sharedWords = /Alex's Desk|Hades|Co-op nights|It Takes Two|Super Nintendo|RetroArch/g;

it.each(installRequests)("asks before installing %s in the pseudo-locale", async (_, request) => {
  await open("/", {
    storage: createStorageFixtureApi(),
    wrap: withRequests(requestsApi([request])),
  });
  await dialogOf("install-confirmation");
  const data = [/https?:\/\/\S+/g, ...paths, devices, sharedWords, "--windowed"];
  expect(over(data)).toEqual([]);
});

it("shows console mode's hints over an install request in the pseudo-locale", async () => {
  await open("/", {
    mode: "console",
    wrap: withRequests(requestsApi(installRequests.map(([, request]) => request).slice(0, 1))),
  });
  await dialogOf("install-confirmation");
  await ready('[data-cs="hint-bar"] [data-part="hint"]');
  expect(over([/https?:\/\/\S+/g, ...paths])).toEqual([]);
});

it("says when install requests can't be read in the pseudo-locale", async () => {
  const api = { ...requestsApi([]), pending: () => Promise.reject("gone") };
  await open("/", { wrap: withRequests(api) });
  await dialogOf("install-input-error");
  expect(shown(['[data-cs="install-input-error"]'])).toEqual([]);
});

const question: JobQuestion = {
  id: "7",
  kind: "launch_target",
  title: "Choose a launch target",
  message: "Several executable files were found.",
  choices: [
    { id: "0", label: "Fixture game", detail: "Game.exe" },
    { id: "1", label: "Level editor", detail: "Editor.exe" },
  ],
};

it.each(modes)("shows a job's question in the pseudo-locale (%s)", async (mode) => {
  // To answer, only to read, and with nothing to choose.
  for (const asked of [question, { ...question, canAnswer: false }, { ...question, choices: [] }]) {
    await open("/activity", {
      mode,
      jobs: [fixtureJob({ status: "needs_user", question: asked })],
    });
    fireEvent.click(await screen.findByRole("button", { name: /Ŕēvĩēŵ ćĥōĩćēś/ }));
    await dialogOf("job-question");
    // The job's words are the core's (#214).
    const data = strings([
      question.title,
      question.message,
      ...question.choices.flatMap((choice) => [choice.label, choice.detail]),
    ]);
    expect(over(data)).toEqual([]);
    cleanup();
  }
});

it.each(modes)("shows Add web game in the pseudo-locale (%s)", async (mode) => {
  await open("/library?filter=web", {
    mode,
    games: [webGame("alpha")],
    storage: createStorageFixtureApi(),
  });
  fireEvent.click(await screen.findByRole("button", { name: /Àďď ŵēƀ ĝàḿē/ }));
  await dialogOf("add-web-game");
  const dialog = one('[data-cs="add-web-game"]') as Element;
  expect(over(["https://"])).toEqual([]);
  // Then where to keep the offline copy.
  fireEvent.change(dialog.querySelector('input[type="url"]') as Element, {
    target: { value: "https://example.com/game" },
  });
  fireEvent.click(dialog.querySelector('input[type="checkbox"]') as Element);
  fireEvent.submit(dialog.querySelector("form") as Element);
  await ready('[data-cs="install-root-picker"] input[type="radio"]');
  expect(over([...paths, /\b(?:Games|External)\b/g, /\bext4\b/g])).toEqual([]);
});

/** What the fixture emulators and platforms name, and how the core
 * describes its own variables. */
async function emulationWords() {
  const snapshot = await createEmulationFixtureApi().snapshot();
  return [
    ...strings([
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
      ...snapshot.variables.flatMap((variable) => [variable.name, variable.description]),
    ]),
    ...paths,
  ];
}

it("shows a manual scan's review in the pseudo-locale", async () => {
  await open("/settings?tab=emulators&section=scanners");
  fireEvent.click(await screen.findByRole("button", { name: /Śćàń.*SNES collection/ }));
  await ready('[data-cs="rom-review"] [data-part="review-game"]');
  const data = [
    ...(await emulationWords()),
    /Chrono Trigger|Super Metroid|Final Fantasy III|Star Ocean(?: Bonus)?/g,
    /\b(?:USA|Japan|Rev 1|Disc \d)\b/g,
  ];
  expect(shown(['[data-cs="rom-review"]'], data)).toEqual([]);
});

it("shows the emulator profile editor in the pseudo-locale", async () => {
  await open("/settings?tab=emulators");
  fireEvent.click(await screen.findByRole("button", { name: /Àďď ćũśţōḿ ƥŕōƒĩĺē/ }));
  await dialogOf("emulation-profile-editor");
  // The preview shows a sample ROM's command, word by word.
  const data = [
    "Sample Game (USA).rom",
    ...(await emulationWords()),
    /\{\w+\}/g,
    /CASKADE_\w+/g,
    ...reactAria,
  ];
  const editor = '[data-cs="emulation-profile-editor"]';
  expect(shown([editor], data, 'code, [data-part="token"]')).toEqual([]);
});

// Choosing several games.

async function selecting(mode: Mode) {
  await open("/library?filter=all&sort=title", { mode });
  await ready('[data-view="library"] [data-cs="game-card"]');
  fireEvent.click(one('[data-view="library"] [data-part="select"]') as Element);
  await ready('[data-cs="selection-bar"]');
}

const bar = (part: string) => one(`[data-cs="selection-bar"] [data-part="${part}"]`) as Element;

/** Selects what's loaded and waits for the bar to say so. */
async function selectLoaded() {
  const before = bar("count").textContent;
  fireEvent.click(bar("select-loaded"));
  await waitFor(() => expect(bar("count").textContent).not.toBe(before));
}

it.each(modes)("shows the selection bar in the pseudo-locale (%s)", async (mode) => {
  await selecting(mode);
  expect(shown(['[data-cs="selection-bar"]'], [], padGlyphs)).toEqual([]);
  await selectLoaded();
  expect(shown(['[data-cs="selection-bar"]'], [], padGlyphs)).toEqual([]);
});

it.each(modes)("shows the bulk editor's steps in the pseudo-locale (%s)", async (mode) => {
  await selecting(mode);
  await selectLoaded();
  fireEvent.click(bar("edit"));
  await dialogOf("bulk-editor");
  const editor = one('[data-cs="bulk-editor"]') as Element;
  expect(over()).toEqual([]);

  // Every field ticked shows what it changes with.
  for (const box of editor.querySelectorAll('[data-part="field"] input[type="checkbox"]'))
    fireEvent.click(box);
  await waitFor(() => expect(editor.querySelector("textarea")).not.toBeNull());
  expect(over()).toEqual([]);

  const description = editor.querySelector("textarea") as Element;
  type(description, "Changed");
  const apply = () => one('[data-cs="bulk-editor"] [data-part="apply"]') as Element;
  fireEvent.click(apply());
  await ready('[data-cs="bulk-editor"] [data-part="results"]');
  // The games' values, and what the core says about them (#214).
  const data = [...dialogData, "Changed", /\bFixture Game\b/g];
  expect(over(data)).toEqual([]);
  fireEvent.click(apply());
  await ready('[data-cs="bulk-editor"] section[aria-label]');
  await waitFor(() => expect(one('[data-cs="bulk-editor"] [data-part="cancel"]')).toBeNull());
  expect(over(data)).toEqual([]);
});

it.each(modes)("shows Refresh details in the pseudo-locale (%s)", async (mode) => {
  await selecting(mode);
  await selectLoaded();
  fireEvent.click(bar("refresh"));
  await dialogOf("games-refresh-start");
  expect(over()).toEqual([]);
  const dialog = one('[data-cs="games-refresh-start"]') as Element;
  const paragraph = () => dialog.querySelector("p")?.textContent;
  const missing = paragraph();
  fireEvent.click(dialog.querySelectorAll('[role="radio"], input[type="radio"]')[1] as Element);
  await waitFor(() => expect(paragraph()).not.toBe(missing));
  expect(over()).toEqual([]);
  fireEvent.click(dialog.querySelector('[data-part="start"]') as Element);
  await ready('[data-cs="games-refresh-start"] a[href]');
  expect(over()).toEqual([]);
});

it("shows a refresh's card in Activity in the pseudo-locale", async () => {
  const refreshing = '[data-cs="games-refresh"]';
  const status = () => one(refreshing)?.getAttribute("data-status");
  // A run, how it ends with a game that failed, and a cancelled one.
  const app = await open("/activity");
  await ready('[data-view="activity"]');
  await act(() => app.library.edits.refreshStart(["fixture-1", "fail-2"], "missing"));
  await waitFor(() => expect(status()).toBe("running"));
  expect(shown([refreshing])).toEqual([]);
  act(() => app.library.edits.finishRefresh());
  await waitFor(() => expect(status()).toBe("done"));
  await ready(`${refreshing} [data-part="retry"]`);
  expect(shown([refreshing])).toEqual([]);
  cleanup();

  const cancelled = await open("/activity");
  await ready('[data-view="activity"]');
  await act(() => cancelled.library.edits.refreshStart(["fixture-1", "fixture-2"], "automatic"));
  await waitFor(() => expect(status()).toBe("running"));
  await act(() => cancelled.library.edits.refreshCancel().then(() => {}));
  act(() => cancelled.library.edits.finishRefresh());
  await waitFor(() => expect(status()).toBe("cancelled"));
  await ready(`${refreshing} [data-part="continue"]`);
  expect(shown([refreshing])).toEqual([]);
});

it.each(modes)(
  "shows a new and a smart collection's dialog in the pseudo-locale (%s)",
  async (mode) => {
    await open("/collections", { mode });
    fireEvent.click(await screen.findByRole("button", { name: /Ńēŵ ćōĺĺēćţĩōń|Ḿàńũàĺ ĺĩśţ/ }));
    await dialogOf("collection-dialog");
    expect(over(collectionData)).toEqual([]);
    const dialog = one('[data-cs="collection-dialog"]') as Element;
    fireEvent.click(dialog.querySelectorAll('[role="radio"], input[type="radio"]')[1] as Element);
    await ready('[data-cs="collection-dialog"] [data-part="rules"]');
    await waitFor(() =>
      expect(one('[data-cs="collection-dialog"] [data-part="matches"]')?.textContent).toMatch(/\d/),
    );
    expect(over([...collectionData, /\bPC\b/g])).toEqual([]);
  },
);

it.each(modes)("shows Edit Rules on a smart collection in the pseudo-locale (%s)", async (mode) => {
  await open("/collections/collection-favourites", { mode });
  fireEvent.click(await screen.findByRole("button", { name: /Ēďĩţ Ŕũĺēś/ }));
  await dialogOf("collection-dialog");
  await ready('[data-cs="collection-dialog"] [data-part="rules"]');
  expect(over(collectionData)).toEqual([]);
});

it.each(modes)("shows Add Games on a collection in the pseudo-locale (%s)", async (mode) => {
  await open("/collections/collection-weekend", { mode });
  fireEvent.click(await screen.findByRole("button", { name: /Àďď Ĝàḿēś/ }));
  await dialogOf("add-games");
  await ready('[data-cs="add-games"] [data-part="games"] input[type="checkbox"]');
  expect(over(collectionData)).toEqual([]);
  // Nothing found.
  type(one('[data-cs="add-games"] input'), "nothing-like-it");
  await waitFor(() =>
    expect(one('[data-cs="add-games"] [data-part="games"] input[type="checkbox"]')).toBeNull(),
  );
  await waitFor(() => expect(one('[data-cs="add-games"] [data-part="games"] p')).not.toBeNull());
  expect(over([...collectionData, "nothing-like-it"])).toEqual([]);
});

it.each(modes)("shows a game's screenshots in the pseudo-locale (%s)", async (mode) => {
  const games = createFixtureGames(2).map((game) => ({
    ...game,
    screenshots: [1, 2, 3].map((n) => ({
      thumb: `asset://shot-${n}/thumb.webp`,
      full: `asset://shot-${n}/full.webp`,
    })),
  }));
  await open("/games/fixture-1", { mode, games });
  await gameHeading();
  await ready('[data-part="screenshots"] button');
  expect(shown(['[data-part="screenshots"]'])).toEqual([]);
  fireEvent.click(one('[data-part="screenshots"] button') as Element);
  await dialogOf("screenshot-viewer");
  expect(over()).toEqual([]);
});

it.each(modes)("shows a game's saves in the pseudo-locale (%s)", async (mode) => {
  const app = await open("/games/fixture-1", { mode });
  const location = { kind: "folder", path: "/home/player/Saves/Fixture" } as const;
  app.library.saves.seed({
    entryId: "entry-1",
    revision: 0,
    preferences: { manifestTitle: null, locations: null },
    matched: { type: "unknown" },
    automatic: [],
    locations: [{ spec: location, tags: [], fileCount: 1 }],
    warnings: [],
  });
  const state = (each: SaveVersion, current = false) => ({
    version: each,
    current,
    complete: true,
  });
  // Saves that disagree, on a game whose saves don't sync.
  app.library.saves.seedHistory("fixture-1", {
    accountId: "",
    syncing: false,
    conflict: ["here", "there"],
    versions: [
      state(there),
      state(here, true),
      { ...state(version("old", true, 12)), complete: false },
    ],
  });
  await gameHeading();
  await ready('[data-cs="saves-panel"] [data-part="versions"]');
  expect(shown(['[data-cs="saves-panel"]'], [...paths, devices], padGlyphs)).toEqual([]);
});

// Searching.

it.each(modes)("shows the search box's suggestions in the pseudo-locale (%s)", async (mode) => {
  await open("/", { mode });
  const input = await waitFor(() => {
    const found = one('[data-cs="search"] input');
    expect(found).not.toBeNull();
    return found as HTMLInputElement;
  });
  // A suggestion's match is marked, in the middle of its title.
  const search = () =>
    shown(
      ['[data-cs="search"]', '[data-cs="search-popover"]'],
      ["Fixture", /Game \d+/g, ...reactAria],
      padGlyphs,
    );
  act(() => input.focus());
  fireEvent.change(input, { target: { value: "Fixture" } });
  // Arrow down opens the list once the suggestions are in.
  await waitFor(() => {
    fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(one('[data-cs="search-popover"] [data-part="suggestion"]')).not.toBeNull();
  });
  await ready('[data-cs="search-popover"] [data-part="see-all"]');
  expect(search()).toEqual([]);
  // Nothing matches.
  fireEvent.change(input, { target: { value: "nothing-like-it" } });
  await waitFor(() =>
    expect(one('[data-cs="search-popover"] [data-part="suggestion"]')).toBeNull(),
  );
  await ready('[data-cs="search-popover"] [data-part="see-all"]');
  await waitFor(() =>
    expect(one('[data-cs="search-popover"] [data-part="see-all"]')?.textContent).toContain(
      "nothing-like-it",
    ),
  );
  expect(
    shown(['[data-cs="search-popover"]'], ["nothing-like-it", ...reactAria], padGlyphs),
  ).toEqual([]);
});

it.each(modes)("shows the search overlay in the pseudo-locale (%s)", async (mode) => {
  await open("/library?filter=all&sort=title", { mode });
  await ready('[data-view="library"] [data-cs="game-card"]');
  fireEvent.keyDown(document.body, { key: "/", bubbles: true });
  await dialogOf("type-search");
  type(one('[data-cs="type-search"] input'), "0002");
  await waitFor(() =>
    expect(one('[data-cs="type-search"] [data-part="count"]')?.textContent).toMatch(/1/),
  );
  expect(shown(['[data-cs="type-search"]'], [], padGlyphs)).toEqual([]);
});

it("shows a Select's list in the pseudo-locale", async () => {
  await open("/library?filter=all&sort=title");
  await ready('[data-view="library"] [data-cs="select"] button');
  fireEvent.click(one('[data-view="library"] [data-cs="select"] button') as Element);
  await dialogOf("select-popover");
  await ready('[data-cs="select-popover"] [role="option"]');
  expect(shown(['[data-cs="select-popover"]'], reactAria)).toEqual([]);
});

// Pairing.

const withStreaming = (options: Parameters<typeof createStreamingFixtureApi>[0] = {}) => {
  const api = createStreamingFixtureApi(options);
  return {
    api,
    wrap: (children: ReactNode) => (
      <StreamingApiProvider api={api}>{children}</StreamingApiProvider>
    ),
  };
};

const pairingData = [devices, /\d+\.\d+\.\d+\.\d+(?::\d+)?/g, /Sunshine|Moonlight/g];

const pairStudy = async () => {
  const row = (await screen.findByText("Study")).closest("li") as HTMLElement;
  fireEvent.click(await within(row).findByRole("button", { name: /Ƥàĩŕ/ }));
  await dialogOf("pair-dialog");
};

it.each(["paired", "wrongCode", "declined", "expired", "failed"] as const)(
  "shows pairing to stream ending %s in the pseudo-locale",
  async (outcome) => {
    const { api, wrap } = withStreaming();
    await open("/settings?tab=streaming", { wrap });
    await pairStudy();
    await ready('[data-cs="pair-dialog"] [data-part="code"]');
    expect(over(pairingData)).toEqual([]);
    act(() => api.finishPairing("fixture-study", outcome));
    await waitFor(() =>
      expect(one('[data-cs="pair-dialog"]')?.getAttribute("data-status")).toBe(outcome),
    );
    expect(over(pairingData)).toEqual([]);
  },
);

it("shows pairing a device of your own, and one that can't be asked, in the pseudo-locale", async () => {
  const linked = withStreaming({ linked: ["fixture-study"] });
  await open("/settings?tab=streaming", { wrap: linked.wrap });
  await pairStudy();
  await waitFor(() =>
    expect(one('[data-cs="pair-dialog"]')?.getAttribute("data-status")).toBe("code"),
  );
  expect(over(pairingData)).toEqual([]);
  cleanup();

  const failing = withStreaming();
  vi.spyOn(failing.api, "pairWith").mockRejectedValue("gone");
  await open("/settings?tab=streaming", { wrap: failing.wrap });
  await pairStudy();
  await waitFor(() =>
    expect(one('[data-cs="pair-dialog"]')?.getAttribute("data-status")).toBe("failed"),
  );
  // The reason is the core's (#214).
  expect(over([...pairingData, "gone"])).toEqual([]);
});

it.each(modes)("asks to let a device stream from here in the pseudo-locale (%s)", async (mode) => {
  const { wrap } = withStreaming({
    hosting: { state: "ready", version: "2026.914.233613", problem: null, warning: null },
    requests: [{ id: "r1", name: "Steam Deck", address: "192.168.1.40" }],
  });
  await open("/", { mode, wrap });
  await dialogOf("stream-pair-prompt");
  expect(over(pairingData)).toEqual([]);
});

it("asks whether to use streaming set up by someone else in the pseudo-locale", async () => {
  const { wrap } = withStreaming({ existing: true });
  await open("/settings?tab=streaming", { wrap });
  await ready(
    '[role="tabpanel"] [data-cs="streaming-settings"] [role="switch"], [role="tabpanel"] [data-cs="streaming-settings"] input[role="switch"]',
  );
  fireEvent.click(one('[data-cs="streaming-settings"] [role="switch"]') as Element);
  await dialogOf("streaming-existing");
  expect(over(pairingData)).toEqual([]);
});

/** Each with its name, as the core words it (#214). */
const programs = () => [
  { part: "host" as const, label: "Hosting", installed: "v1.0", latest: "v1.1", update: true },
  { part: "client" as const, label: "Client", installed: "v6", latest: "v6", update: false },
];

it("shows Streaming updates in the pseudo-locale", async () => {
  const { wrap } = withStreaming({
    hosting: { state: "ready", version: "2026.914.233613", problem: null, warning: null },
    programs: programs(),
  });
  await open("/settings?tab=streaming", { wrap });
  await waitFor(() => expect(all('[data-cs="streaming-program"]')).toHaveLength(2));
  expect(shown(['[data-cs="streaming-program"]'], [...pairingData, /Hosting|Client/g])).toEqual([]);
});

const pairing = (state: string, overrides: object = {}) => ({
  deviceId: "9".repeat(64),
  name: "Office laptop",
  kind: "laptop",
  os: "mac",
  code: "123456",
  incoming: true,
  confirmed: false,
  state,
  ...overrides,
});

/** The devices fixture with this pairing under way. */
function pairingWith(current: ReturnType<typeof pairing>): SyncApi {
  const api = createSyncFixtureApi();
  return {
    ...api,
    status: async () => ({ ...(await api.status()), pairing: current }) as never,
  };
}

const withSync = (api: SyncApi) => (children: ReactNode) => (
  <SyncProvider api={api}>{children}</SyncProvider>
);

/** One account, not an admin. */
const standard: FixtureAccount[] = [{ id: "kid", name: "Kid" }];

it.each(modes)("shows pairing a device in the pseudo-locale (%s)", async (mode) => {
  const states = [
    pairing("waiting"),
    pairing("waiting", { incoming: false, confirmed: true }),
    pairing("paired"),
    pairing("cancelled"),
    pairing("failed"),
  ];
  for (const current of states) {
    await open("/", { mode, wrap: withSync(pairingWith(current)) });
    await dialogOf("pair-device");
    expect(over([devices])).toEqual([]);
    cleanup();
  }
});

it("tells someone who isn't an admin who can pair in the pseudo-locale", async () => {
  await open("/", { accounts: standard, wrap: withSync(pairingWith(pairing("waiting"))) });
  await dialogOf("pair-device");
  expect(over([devices])).toEqual([]);
  cleanup();

  const link = createSyncFixtureApi();
  link.openLink({ deviceId: "c".repeat(64), expiresAtMs: Date.UTC(2026, 10, 9), expired: false });
  await open("/", { accounts: standard, wrap: withSync(link) });
  await dialogOf("pair-device");
  expect(over([devices, /\bc{8}\b/g])).toEqual([]);
});

it.each(modes)("asks about a pairing link in the pseudo-locale (%s)", async (mode) => {
  const deviceId = "c".repeat(64);
  const link = /\bc{8}\b/g;
  // One that has expired.
  const old = createSyncFixtureApi();
  old.openLink({ deviceId, expiresAtMs: Date.UTC(2026, 9, 9), expired: true });
  await open("/", { mode, wrap: withSync(old) });
  await dialogOf("pair-device");
  expect(over([devices, link])).toEqual([]);
  cleanup();

  // One to accept.
  const sync = createSyncFixtureApi();
  sync.openLink({ deviceId, expiresAtMs: Date.UTC(2026, 10, 9), expired: false });
  await open("/", { mode, wrap: withSync(sync) });
  await dialogOf("pair-device");
  expect(over([devices, link])).toEqual([]);
  fireEvent.click([...all('[data-cs="pair-device"] button')].at(-1) as Element);
  await waitFor(() => expect(all('[data-cs="pair-device"] button')).toHaveLength(1));
  expect(over([devices, /Office desktop/g])).toEqual([]);
});

it("shows games moving between devices in the pseudo-locale", async () => {
  const transfer = (state: Transfer["state"], direction: Transfer["direction"]): Transfer => ({
    id: `${state}-${direction}`,
    title: "Fixture Game 0002",
    deviceName: "Steam Deck",
    direction,
    doneBytes: 1024 ** 3,
    totalBytes: 2 * 1024 ** 3,
    bytesPerSecond: 20 * 1024 ** 2,
    link: state === "waitingDirect" ? "relay" : "lan",
    state,
  });
  const sync = createSyncFixtureApi();
  sync.setTransfers([
    transfer("running", "incoming"),
    transfer("waitingDirect", "outgoing"),
    transfer("done", "incoming"),
    transfer("failed", "outgoing"),
  ]);
  await open("/settings?tab=devices", { wrap: withSync(sync) });
  await waitFor(() => expect(all('[data-cs="transfer"]')).toHaveLength(4));
  expect(shown(['[data-cs="transfer"]'], [devices])).toEqual([]);
});

// Add-ons and emulated releases.

it.each(modes)("shows an add-on's details in the pseudo-locale (%s)", async (mode) => {
  const items = fixtureCatalogueItems();
  const data = [
    ...strings(
      items.flatMap(({ entry }) => [
        entry.name,
        entry.author,
        entry.summary,
        entry.description,
        entry.license,
        entry.homepage,
      ]),
    ),
    /\d+\.\d+\.\d+/g,
    /https?:\/\/\S+/g,
    /\bcaskade\.\w+/g,
  ];
  await open("/settings?tab=plugins", { mode });
  fireEvent.click(await screen.findByRole("button", { name: /Paper/ }));
  await dialogOf("addon-details");
  expect(over(data)).toEqual([]);
});

/** A release that can't play yet, as the core describes it. */
const emulated: GameEmulation = {
  entryId: "entry-1",
  platformId: "nintendo_snes",
  platform: {
    id: "nintendo_snes",
    name: "Super Nintendo",
    short: "SNES",
    extensions: ["sfc"],
    bios: [],
  },
  target: { type: "platform_default" },
  args: { type: "default" },
  discs: [
    { name: "Disc 1", path: "/roms/Game (Disc 1).cue", exists: true, size: 10 },
    { name: "Disc 2", path: "/roms/Game (Disc 2).cue", exists: false, size: 10 },
  ],
  profile: null,
  compatible: [
    {
      emulatorId: "ra",
      profileId: "snes",
      emulatorName: "RetroArch",
      profileName: "Snes9x",
      available: true,
    },
  ],
  bios: [
    {
      file: "scph5501.bin",
      required: true,
      found: false,
      state: "missing",
      path: null,
      platform: "sony_playstation",
    },
    {
      file: "dc/optional.bin",
      required: false,
      found: null,
      state: null,
      path: null,
      platform: null,
    },
  ],
  readiness: { type: "no_emulator" },
  message: "No emulator for SNES",
};

it.each(modes)("shows an emulated release in the pseudo-locale (%s)", async (mode) => {
  const games = createFixtureGames(2).map((game) => ({
    ...game,
    entries: game.entries.map((entry) => ({ ...entry, emulated: true })),
  }));
  const api = {
    gameEmulation: async () => structuredClone(emulated),
    setGameEmulation: async () => structuredClone(emulated),
    splitArgs: async (text: string) => [text],
  };
  await open("/games/fixture-1", {
    mode,
    games,
    wrap: (children: ReactNode) => (
      <GameEmulationProvider api={api}>{children}</GameEmulationProvider>
    ),
  });
  await gameHeading();
  await dialogOf("emulation");
  const data = [
    /\{\w+\}/g,
    /RetroArch|Snes9x|Super Nintendo|SNES|No emulator for/g,
    /\bDisc \d\b/g,
    /scph5501|optional|\bdc\b/g,
    ...paths,
    ...reactAria,
  ];
  expect(shown(['[data-cs="emulation"]'], data)).toEqual([]);
});

// When things fail.

it.each(modes)("says a view couldn't be shown in the pseudo-locale (%s)", async (mode) => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  const [game] = createFixtureGames(1);
  await open("/games/fixture-0", {
    mode,
    games: [{ ...(game as GameDetails), entries: undefined as never }],
  });
  await dialogOf("view-error");
  // What threw says it in the language it was written in.
  const message = one('[data-cs="view-error"] [data-part="message"]')?.textContent ?? "";
  expect(shown(['[data-cs="view-error"]'], [message])).toEqual([]);
});

function Crash(): ReactNode {
  throw new Error("provider failed");
}

it("says the app crashed in the pseudo-locale", async () => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  await open("/", {
    wrap: () => (
      <AppErrorBoundary>
        <Crash />
      </AppErrorBoundary>
    ),
  });
  await dialogOf("view-error");
  expect(shown(['[data-cs="view-error"]'], ["provider failed"])).toEqual([]);
});

it.each(modes)("says a tool didn't start in the pseudo-locale (%s)", async (mode) => {
  const app = await managed(mode);
  await app.library.actions.save("fixture-1", {
    id: "",
    entryId: "entry-1",
    name: "Mod manager",
    kind: "utility",
    target: { type: "executable", path: "/fixture/mods", args: [], cwd: null, env: {} },
    tracking: { mode: "automatic", folder: null, names: [] },
    proton: null,
  });
  vi.spyOn(app.library.actions, "runTool").mockRejectedValue(new Error("The tool is missing"));
  await gameHeading();
  await gameMenu();
  fireEvent.click(await menuItem(/Mod manager/));
  await dialogOf("tool-failed");
  expect(over(["Mod manager", "The tool is missing"])).toEqual([]);
});
