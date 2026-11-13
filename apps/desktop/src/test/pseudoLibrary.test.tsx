/** The views' own content in the pseudo-locale (#9): Home, the Library (its
 * filters, platforms, Web mosaic, quick look and alphabet bar), a game's
 * page, screenshots and saves, the Store and a catalog game's page,
 * Collections, and the search overlays. */
import { act, cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { expect, it, vi } from "vitest";
import { AchievementsProvider } from "../api/achievements";
import {
  createAchievementsFixture,
  retroSet,
  staleGogSet,
  steamSet,
} from "../api/achievementsFixtures";
import { createCatalogFixtureApi, fixtureCatalog } from "../api/catalogFixtures";
import { createEmulatedFixtureGames, createFixtureGames } from "../api/fixtures";
import { CatalogProvider, type GameIgdbApi, GameIgdbProvider, type IgdbGame } from "../api/igdb";
import type { SaveVersion } from "../api/saves";
import type { Mode } from "../shell/console/mode";
import { presentPlatform } from "../views/library/platforms/catalog";
import { playedGames } from "./harness";
import {
  all,
  collectionData,
  devices,
  dialogOf,
  gameHeading,
  here,
  modes,
  one,
  open,
  over,
  padGlyphs,
  paths,
  reactAria,
  ready,
  shown,
  strings,
  there,
  type,
  version,
  webGame,
} from "./pseudoHarness";

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

it("shows the Library's alphabet bar in the pseudo-locale", async () => {
  await open("/library?filter=all&sort=title", { games: createFixtureGames(60) });
  await ready('[data-cs="alphabet-bar"] [data-part="letter"]');
  expect(shown(['[data-cs="alphabet-bar"]'])).toEqual([]);
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
