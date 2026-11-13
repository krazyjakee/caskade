import { type AnatomyEntry, anatomy, anatomyRoots, checkAnatomy, ownedParts } from "@caskade/ui";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, onTestFinished, vi } from "vitest";
import { type AchievementSet, AchievementsProvider } from "./api/achievements";
import {
  createAchievementsFixture,
  emptySet,
  retroSet,
  staleGogSet,
  steamSet,
} from "./api/achievementsFixtures";
import { blankAction } from "./api/actions";
import { type AddonProgress, AddonsProvider } from "./api/addons";
import { createAddonsFixture } from "./api/addonsFixtures";
import { type AppUpdateApi, AppUpdateProvider } from "./api/appUpdate";
import type { BrowseApi } from "./api/browse";
import { createBrowseFixtureApi } from "./api/browseFixtures";
import { createEmulationFixtureApi } from "./api/emulationFixtures";
import { createFixtureApi, createFixtureGames } from "./api/fixtures";
import { createInstallFixtureApi } from "./api/installFixtures";
import { type NotificationsApi, NotificationsProvider } from "./api/notifications";
import {
  createNotificationsFixtureApi,
  fixtureNotification,
  fixtureNotifications,
} from "./api/notificationsFixtures";
import { createLibraryQueryClient, LibraryProvider } from "./api/provider";
import type { RommApi, RommOverview } from "./api/romm";
import { StorageProvider } from "./api/StorageProvider";
import { SharesProvider } from "./api/shares";
import { createSharesFixtureApi, FIXTURE_LINK, fixtureShare } from "./api/sharesFixtures";
import type { RootSummary, StorageApi } from "./api/storage";
import { SyncProvider, type Transfer } from "./api/sync";
import { createSyncFixtureApi } from "./api/syncFixtures";
import { fixtureStatsState } from "./api/systemStatsFixtures";
import type { CollectionSummary } from "./api/types";
import { CollectionCard } from "./components/CollectionCard";
import { FolderBrowser } from "./components/pathBrowser/FolderBrowser";
import { PadSource } from "./input";
import { DrawnTree } from "./plugins/drawn";
import { PluginBoundary } from "./plugins/PluginBoundary";
import { PluginRegistry, PluginRegistryContext } from "./plugins/registry";
import type { UiPlugin } from "./plugins/types";
import { AppShell } from "./shell/AppShell";
import { type AppWindow, AppWindowContext } from "./shell/appWindow";
import { type ConsoleApi, type Mode, ModeProvider } from "./shell/console/mode";
import { AppCrash } from "./shell/ErrorFallback";
import { fakeWindow } from "./shell/windowFixture";
import {
  fakeLayout,
  fakePad,
  fakeViewport,
  playedGames,
  renderApp as renderWholeApp,
} from "./test/harness";
import { renderOnboarding } from "./test/onboardingFixture";
import { ActivityProvider } from "./views/activity/ActivityProvider";
import { ActivityView } from "./views/activity/ActivityView";
import { GameView } from "./views/game/GameView";
import { Screenshots } from "./views/game/Screenshots";
import { Videos } from "./views/game/Videos";
import { HomeView } from "./views/home/HomeView";
import { LibraryCard } from "./views/library/LibraryCard";
import { LibraryRow } from "./views/library/LibraryRow";
import { LibraryView } from "./views/library/LibraryView";
import { WebMosaic } from "./views/library/web/WebMosaic";
import { EmulationProvider } from "./views/settings/emulators/EmulationProvider";
import { RommPanel } from "./views/settings/romm/RommPanel";
import { RommApiContext } from "./views/settings/romm/useRomm";
import { StatisticsDashboard } from "./views/settings/statistics/StatisticsDashboard";
import { WhereToBuy } from "./views/store/WhereToBuy";

afterEach(cleanup);

const games = createFixtureGames(3).map((game, i) => ({
  ...game,
  favorite: i === 0,
  lastPlayedAt: i === 0 ? "2026-10-01T10:00:00Z" : null,
  website: i === 0 ? "https://games.example/fixture" : null,
}));
const first = games[0] as (typeof games)[number];

/** The first game's managed copy is on an offline root. */
const storage: StorageApi = {
  list: async (): Promise<RootSummary[]> => [
    {
      root: { id: "offline", path: "/fixture/offline", label: null, isDefault: true },
      status: "offline",
      capabilities: {
        writable: false,
        executable: false,
        symlinks: false,
        caseSensitive: true,
        freeBytes: null,
        totalBytes: null,
        filesystem: null,
        warnings: [],
      },
      games: [
        {
          entryId: "managed-copy",
          gameId: first.id,
          title: first.title,
          relativeDir: "game",
          sizeBytes: 0,
          available: false,
          moveBlocked: null,
        },
      ],
      folderTemplate: "{title}",
      gamesBytes: 0,
      otherBytes: null,
    },
  ],
  ...Object.fromEntries(
    [
      "preferences",
      "add",
      "edit",
      "setDefault",
      "remove",
      "moveAndRemove",
      "chooseDirectory",
      "move",
      "uninstall",
    ].map((name) => [name, async () => Promise.reject(new Error(`unexpected ${name}`))]),
  ),
} as StorageApi;

/** Plugins contribute a view, a card badge and a topbar item; their own
 * `data-part`s sit in the shell's slots and are not shell anatomy. */
function registry() {
  const plugin: UiPlugin = {
    id: "dev.anatomy",
    name: "Anatomy",
    version: "0.1.0",
    entry: "caskade-plugin://dev.anatomy/ui.js",
    contributes: [
      { slot: "views", id: "stats" },
      { slot: "game.card.badges", id: "hltb" },
      { slot: "topbar.items", id: "clock" },
    ],
  };
  const registry = new PluginRegistry();
  registry.register(plugin, "views", { id: "stats", title: "Stats", component: () => null });
  registry.register(plugin, "game.card.badges", {
    id: "hltb",
    component: () => <span data-part="hltb">12h</span>,
  });
  registry.register(plugin, "topbar.items", {
    id: "clock",
    component: () => <span data-part="clock">12:00</span>,
  });
  return registry;
}

/** A pinned smart collection and an empty manual one: every part and kind. */
const collectionCards: CollectionSummary[] = [
  {
    id: "smart",
    name: "Ready to Play",
    description: "Everything installed.",
    pinned: true,
    kind: { type: "smart", query: { filter: "installed", sort: "title" } },
    updatedAt: null,
    games: 2,
    playtimeSeconds: 3600,
    covers: ["cover.png"],
  },
  {
    id: "manual",
    name: "Couch Co-op",
    description: "",
    pinned: false,
    kind: { type: "manual" },
    updatedAt: new Date().toISOString(),
    games: 0,
    playtimeSeconds: 0,
    covers: [],
  },
];

function Showcase() {
  return (
    <>
      <HomeView />
      <div data-cs="panel">
        <span data-part="view-specific">A view's own content</span>
      </div>
      {collectionCards.map((collection) => (
        <CollectionCard key={collection.id} collection={collection} />
      ))}
      <WhereToBuy
        links={[
          {
            store: "steam",
            name: "Steam",
            url: "https://store.example.invalid/game",
            platforms: [],
            countries: [],
          },
        ]}
      />
      {games.map((game) => (
        <LibraryCard key={game.id} game={game} />
      ))}
      {/* A favourite (huge), the most played (large) and one more (small). */}
      <WebMosaic games={games.map((game) => ({ ...game, id: `web-${game.id}` }))} />
      <table aria-label="Library rows">
        <tbody>
          {[...games, { ...games[1], id: "emulated", status: "emulated" as const }].map((game) => (
            <LibraryRow key={game.id} game={game as (typeof games)[number]} />
          ))}
        </tbody>
      </table>
    </>
  );
}

/** A check has found a new Caskade, so the status bar offers it. */
const appUpdates: AppUpdateApi = {
  status: async () => ({
    currentVersion: "0.1.0",
    unavailable: null,
    phase: "available",
    release: { version: "0.2.0", notes: null, date: null },
    checkedAt: 0,
    downloaded: 0,
    total: null,
    error: null,
  }),
  check: async () => Promise.reject(new Error("not in this test")),
  install: async () => Promise.reject(new Error("not in this test")),
  restart: async () => {},
  subscribe: () => () => {},
};

const consoleApi: ConsoleApi = {
  initial: async () => "desktop",
  set: async () => {},
  subscribe: async () => () => {},
};

function renderApp(
  path: string,
  {
    mode = "desktop",
    frame = "custom",
    library = games,
    notifications = createNotificationsFixtureApi(),
  }: {
    mode?: Mode;
    frame?: AppWindow["frame"];
    library?: typeof games;
    notifications?: NotificationsApi;
  } = {},
) {
  const pad = fakePad();
  const root = createRootRoute({ component: AppShell });
  const routeTree = root.addChildren([
    createRoute({ getParentRoute: () => root, path: "/", component: Showcase }),
    createRoute({
      getParentRoute: () => root,
      path: "/games/$gameId",
      component: () => <GameView gameId={first.id} />,
    }),
    createRoute({
      getParentRoute: () => root,
      path: "/library",
      component: () => <LibraryView filter="all" />,
    }),
    createRoute({
      getParentRoute: () => root,
      path: "/platforms",
      component: () => <LibraryView filter="emulated" sort="title" />,
    }),
    createRoute({
      getParentRoute: () => root,
      path: "/titles",
      component: () => <LibraryView filter="all" sort="title" />,
    }),
    createRoute({
      getParentRoute: () => root,
      path: "/list",
      component: () => <LibraryView filter="all" sort="title" layout="list" />,
    }),
    createRoute({
      getParentRoute: () => root,
      path: "/activity",
      component: () => <ActivityView tab="notifications" open="batch" />,
    }),
    createRoute({
      getParentRoute: () => root,
      path: "/jobs",
      component: () => <ActivityView tab="jobs" />,
    }),
  ]);
  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  const api = createFixtureApi(library);
  const result = render(
    <PadSource value={pad.subscribe}>
      <LibraryProvider api={api} client={createLibraryQueryClient()}>
        <AppUpdateProvider api={appUpdates}>
          <NotificationsProvider api={notifications}>
            <StorageProvider api={storage}>
              <ActivityProvider api={createInstallFixtureApi()} storageApi={storage}>
                <PluginRegistryContext.Provider value={registry()}>
                  <ModeProvider api={consoleApi} initial={mode}>
                    <AppWindowContext value={{ ...fakeWindow(), frame }}>
                      <RouterProvider router={router} />
                    </AppWindowContext>
                  </ModeProvider>
                </PluginRegistryContext.Provider>
              </ActivityProvider>
            </StorageProvider>
          </NotificationsProvider>
        </AppUpdateProvider>
      </LibraryProvider>
    </PadSource>,
  );
  return { ...result, pad, api, router };
}

/** Every catalogued root in the document (or only those named `only`):
 * problems, parts and state seen. */
function survey(seen: Map<string, Set<string>>, only?: string) {
  const problems: string[] = [];
  for (const root of anatomyRoots(document.body)) {
    const name = root.getAttribute("data-cs") ?? "";
    if (only && name !== only) continue;
    const entry: AnatomyEntry = anatomy[name as keyof typeof anatomy];
    problems.push(...checkAnatomy(root));
    const found = seen.get(name) ?? new Set<string>();
    found.add("(root)");
    for (const part of ownedParts(root)) {
      const value = part.getAttribute("data-part") ?? "";
      if (value in entry.parts) found.add(value);
    }
    for (const attribute of Object.keys(entry.states)) {
      const value = root.getAttribute(attribute);
      if (value !== null) found.add(`${attribute}=${value}`);
    }
    seen.set(name, found);
  }
  return problems;
}

// One mount per test, so no single test runs four app mounts against the
// default timeout. The scenarios feed `seen` in order (Vitest 5 has no
// `describe.sequential`; these options pin it), and the last test checks coverage.
describe("the shell's catalogued anatomy, and the app-only parts of shared components", {
  concurrent: false,
  shuffle: false,
}, () => {
  const seen = new Map<string, Set<string>>();

  it("renders the desktop shell, maximized, with search suggestions", async () => {
    renderApp("/");
    await screen.findAllByText("Managed copy unavailable");
    await screen.findByText(/Continue Playing/i);
    await screen.findByRole("link", { name: /Activity, 1 active job\b/ });
    await screen.findByRole("button", { name: "Player, account" });
    // The window controls restored, then maximized for the rest.
    expect(survey(seen)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Maximize" }));
    await screen.findByRole("button", { name: "Restore" });
    // Typing opens the search suggestions.
    const search = screen.getByRole("combobox", { name: "Search games" });
    act(() => search.focus());
    fireEvent.change(search, { target: { value: first.title.slice(0, 4) } });
    // Arrow down opens the list once the suggestions are in.
    await waitFor(() => {
      fireEvent.keyDown(search, { key: "ArrowDown" });
      expect(document.querySelector('[data-cs="search-popover"]')).not.toBeNull();
    });
    await screen.findByRole("option", { name: new RegExp(first.title) });
    expect(survey(seen)).toEqual([]);
  });

  it("renders a game page", async () => {
    renderApp(`/games/${first.id}`);
    await screen.findByRole("heading", { level: 1, name: first.title });
    expect(survey(seen)).toEqual([]);
  });

  it("renders the keyboard search overlay", async () => {
    fakeViewport();
    onTestFinished(() => {
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    });
    renderApp("/library?filter=all");
    await screen.findByRole("heading", { name: "Library" });
    fireEvent.keyDown(document.body, { key: "F" });
    await screen.findByRole("textbox", { name: "Search games" });
    await waitFor(() =>
      expect(
        document.querySelector('[data-cs="type-search"] [data-part="count"]')?.textContent,
      ).toMatch(/\d/),
    );
    expect(survey(seen)).toEqual([]);
  });

  it("renders the emulated library's platforms, known and unknown", async () => {
    const emulated = ["Nintendo 64", "Fairchild Channel F"].map((platform, i) => ({
      ...first,
      id: `emulated-${i}`,
      entries: [],
      platform,
      status: "emulated" as const,
      playtimeSeconds: 3600,
      lastPlayedAt: "2026-10-01T10:00:00Z",
    }));
    renderApp("/platforms", { library: emulated });
    await screen.findByRole("link", { name: "Fairchild Channel F, 1 game" });
    expect(survey(seen)).toEqual([]);
  });

  it("renders a pad's quick look and on-screen keyboard", async () => {
    fakeViewport();
    // Boxes for spatial navigation and the overlays.
    const layout = fakeLayout();
    onTestFinished(() => {
      layout();
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    });
    const look = {
      ...first,
      id: "look",
      platform: "Nintendo 64",
      releaseDate: "1998-11-21",
      genre: "Adventure",
      tags: ["Classic"],
      description: "A fixture with everything a quick look shows.",
      entries: [],
    };
    const app = renderApp("/library", { mode: "console", library: [look] });
    await waitFor(() =>
      expect(document.activeElement?.closest('[data-cs="game-card"]')).not.toBeNull(),
    );
    app.pad.press("details");
    await screen.findByRole("dialog");
    await waitFor(() =>
      expect(
        document.querySelector('[data-cs="quick-look"] [data-part="released"]'),
      ).not.toBeNull(),
    );
    expect(survey(seen)).toEqual([]);
    app.pad.press("back");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

    app.pad.press("search");
    await waitFor(() => expect(document.querySelector('[data-cs="osk"]')).not.toBeNull());
    expect(survey(seen)).toEqual([]);
    app.pad.press("tab.next");
    expect(survey(seen)).toEqual([]);
    app.pad.press("tab.next");
    const shift = screen.getByText("⇧");
    act(() => shift.click());
    expect(survey(seen)).toEqual([]);
    act(() => shift.click());
    expect(survey(seen)).toEqual([]);
  });

  it("renders the alphabet bar beside a long library sorted by title", async () => {
    fakeViewport();
    onTestFinished(() => {
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    });
    const titled = Array.from({ length: 60 }, (_, i) => ({
      ...first,
      id: `titled-${i}`,
      title: `${"ABCXYZ"[i % 6]} Game ${i}`,
    }));
    renderApp("/titles", { library: titled });
    await screen.findByRole("toolbar", { name: "Jump to letter" });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "X" })).toHaveProperty("disabled", false),
    );
    expect(survey(seen)).toEqual([]);
  });

  it("puts the console button in the bar without drawn window controls (macOS)", async () => {
    renderApp("/", { frame: "overlay" });
    await screen.findByRole("button", { name: "Console mode" });
    expect(survey(seen)).toEqual([]);
  });

  it("renders console mode's shell, its hints and the shelves' LB/RB", async () => {
    renderApp("/", { mode: "console" });
    await screen.findByText("Select");
    await screen.findByRole("link", { name: "Stats" });
    await screen.findByRole("button", { name: "Player, account" });
    expect(survey(seen)).toEqual([]);
  });

  it("renders console mode's curtain over a running game", async () => {
    const app = renderApp("/", { mode: "console" });
    await screen.findByText("Select");
    await act(() => app.api.launchGame("entry-1"));
    await waitFor(() =>
      expect(document.querySelector('[data-cs="game-curtain"] [data-part="hint"]')).not.toBeNull(),
    );
    expect(survey(seen)).toEqual([]);
  });

  it("renders a console card for a game running on another device", async () => {
    const undoLayout = fakeLayout();
    fakeViewport();
    onTestFinished(() => {
      undoLayout();
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    });
    const app = renderWholeApp("/library", { mode: "console", games: playedGames(4) });
    app.library.saves.setPlaying([
      {
        deviceId: "deck",
        deviceName: "Steam Deck",
        gameId: "fixture-1",
        title: "Fixture Game 0002",
        accountId: "",
        startedAtMs: 0,
      },
    ]);
    await waitFor(() =>
      expect(
        document.querySelector('[data-cs="game-card"] [data-part="playing-elsewhere"]'),
      ).not.toBeNull(),
    );
    expect(survey(seen)).toEqual([]);
  });

  it("renders Home's games last played on other devices", async () => {
    const games = playedGames(3).map((game) => ({
      ...game,
      lastPlayedOn: { device: "den", name: "Den" },
    }));
    renderWholeApp("/", { games });
    await waitFor(() =>
      expect(
        document.querySelector('[data-cs="game-card"] [data-part="played-on"]'),
      ).not.toBeNull(),
    );
    expect(document.querySelector('[data-cs="hero"] [data-part="played-on"]')).not.toBeNull();
    expect(survey(seen)).toEqual([]);
  });

  it("renders first-run setup, step by step", async () => {
    renderOnboarding();
    const start = await screen.findByRole("button", { name: "Get started" });
    expect(survey(seen)).toEqual([]);
    fireEvent.click(start);
    await screen.findByRole("radio", { name: "System (English)" });
    expect(survey(seen)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    await screen.findByText("/home/player/Games/Caskade");
    expect(survey(seen)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    await screen.findByRole("switch", { name: "Retro games" });
    expect(survey(seen)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    await screen.findByRole("radio", { name: /Console mode/ });
    expect(survey(seen)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    await screen.findByRole("radio", { name: "Rocket" });
    expect(survey(seen)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Set up" }));
    await screen.findByRole("button", { name: "Open my library" }, { timeout: 5000 });
    expect(survey(seen)).toEqual([]);
  });

  it("renders the folder browser: both kinds, an empty folder and an error", async () => {
    const tree = createBrowseFixtureApi();
    // A place that has gone since it was listed.
    const api: BrowseApi = {
      list: async (path, hidden) => {
        if (path === "/gone") throw new Error("/gone does not exist");
        const listing = await tree.list(path, hidden);
        return { ...listing, places: [...listing.places, { label: "Gone", path: "/gone" }] };
      },
    };
    const files = render(
      <FolderBrowser
        request={{
          kind: "file",
          title: "Install from file",
          extensions: ["zip"],
          startAt: "/home/player/Downloads",
        }}
        api={api}
        onClose={() => {}}
      />,
    );
    await screen.findByRole("button", { name: "starfield-patch.zip" });
    expect(screen.getByRole("button", { name: "notes.txt" })).toHaveProperty("disabled", true);
    expect(survey(seen)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Gone" }));
    await screen.findByText("/gone does not exist");
    expect(survey(seen)).toEqual([]);
    files.unmount();
    render(
      <FolderBrowser
        request={{
          kind: "folder",
          title: "Choose a folder",
          startAt: "/home/player/Games/Caskade",
        }}
        api={api}
        onClose={() => {}}
      />,
    );
    await screen.findByText("This folder is empty.");
    expect(survey(seen)).toEqual([]);
  });

  it("renders a game's screenshot gallery", async () => {
    const shots = [1, 2].map((n) => ({ thumb: `shot-${n}-thumb.webp`, full: `shot-${n}.webp` }));
    render(<Screenshots title={first.title} screenshots={shots} />);
    fireEvent.click(screen.getByRole("button", { name: `${first.title}, screenshot 1` }));
    await screen.findByRole("dialog", { name: `${first.title}, screenshots` });
    expect(survey(seen)).toEqual([]);
  });

  it("renders a game's videos", () => {
    render(
      <QueryClientProvider client={new QueryClient()}>
        <Videos title={first.title} videos={[{ videoId: "c0i88t0Kacs", name: "Trailer" }]} />
      </QueryClientProvider>,
    );
    expect(survey(seen)).toEqual([]);
  });

  it("renders the notification feed: every kind, an open batch, pages and empty", async () => {
    // Every category and state on the first page, then enough older ones
    // for a second page.
    const items = [
      ...fixtureNotifications(),
      fixtureNotification({
        id: "updates",
        category: "updatesAvailable",
        title: "No updates left",
        count: 0,
        sources: [],
        version: 4,
        unread: false,
        resolved: true,
        updatedAtMs: 1_000_000,
      }),
      fixtureNotification({
        id: "unlocked",
        category: "achievementsUnlocked",
        title: "2 achievements unlocked in Celeste",
        body: "Forsaken City, Old Site (Steam, Ada)",
        count: 1,
        sources: [],
        target: { type: "game", gameId: "celeste" },
        version: 40,
        unread: false,
        updatedAtMs: 900_000,
      }),
      ...Array.from({ length: 18 }, (_, i) =>
        fixtureNotification({
          id: `old-${i}`,
          title: `Old batch ${i}`,
          version: 5 + i,
          unread: false,
          updatedAtMs: i,
        }),
      ),
    ];
    const notifications = createNotificationsFixtureApi(items);
    const check = () => ["notifications", "notification"].flatMap((name) => survey(seen, name));
    for (const mode of ["desktop", "console"] as const) {
      renderApp("/activity", { mode, notifications });
      await screen.findByRole("heading", { name: "10 games added" });
      await screen.findByRole("link", { name: "Fixture Game 4" });
      await screen.findByRole("button", { name: "Older" });
      await waitFor(() =>
        expect(document.querySelector('[data-part="notification-count"]')).not.toBeNull(),
      );
      expect(check()).toEqual([]);
      expect(survey(seen, mode === "desktop" ? "topbar" : "console-topbar")).toEqual([]);
      cleanup();
    }
    renderApp("/activity", { notifications: createNotificationsFixtureApi([]) });
    await screen.findByText(/No notifications yet/);
    expect(check()).toEqual([]);
  });

  it("renders the error shown in place of a failed view", () => {
    render(<AppCrash error={new Error("Bad game data")} />);
    expect(survey(seen)).toEqual([]);
  });

  it("renders a plugin's drawn UI, every component, and its placeholder (#109)", () => {
    const errors = console.error;
    console.error = () => {};
    try {
      render(
        <>
          <DrawnTree
            busy={false}
            act={() => {}}
            nodes={[
              { type: "heading", text: "Most played", level: 2 },
              { type: "paragraph", text: "Where your playtime goes." },
              { type: "note", text: "From howlongtobeat.com" },
              { type: "alert", text: "It couldn't be reached." },
              {
                type: "stack",
                direction: "row",
                gap: "small",
                children: [
                  { type: "text", text: "Short" },
                  { type: "badge", text: "New", tone: "accent" },
                  { type: "button", label: "Refresh", action: "refresh", variant: "primary" },
                ],
              },
              {
                type: "section",
                label: "Times",
                children: [
                  { type: "stats", items: [{ label: "Main story", value: "8 h" }] },
                  { type: "tile", value: "53 h", label: "in total" },
                ],
              },
              {
                type: "bars",
                label: "Hours played",
                items: [{ label: "Celeste", value: 40, text: "40 h" }],
              },
              {
                type: "table",
                caption: "Most played",
                columns: ["Game", "Hours"],
                rows: [["Celeste", "40"]],
                rowHeaders: true,
              },
              { type: "list", ordered: false, items: [{ type: "text", text: "One" }] },
              { type: "progress", label: "Downloading", value: 2, max: 8 },
              {
                type: "segmented",
                label: "Show as",
                action: "view",
                value: "chart",
                options: [
                  { value: "chart", label: "Chart" },
                  { value: "table", label: "Table" },
                ],
              },
            ]}
          />
          <PluginBoundary plugin={{ id: "dev.caskade.crashy", name: "Crashy" }}>
            <Broken />
          </PluginBoundary>
        </>,
      );
    } finally {
      console.error = errors;
    }
    expect(survey(seen)).toEqual([]);
  });

  it("renders Settings' section list", async () => {
    renderWholeApp("/settings?tab=emulators");
    await screen.findByRole("tab", { name: "Emulation", selected: true });
    // The panels' own content is theirs; only the section list is the shell's.
    expect(survey(seen, "settings-nav")).toEqual([]);
  });

  it("renders About and the Legal view", async () => {
    renderWholeApp("/settings?tab=about");
    await screen.findByRole("link", { name: /by IGDB/ });
    expect(survey(seen, "about")).toEqual([]);
    cleanup();
    renderWholeApp("/settings?tab=about&section=licenses");
    await screen.findByRole("article", { name: "Licenses" });
    expect(survey(seen, "legal")).toEqual([]);
  });

  it("renders the RomM server list with a server that needs signing in here", async () => {
    const overview: RommOverview = {
      settings: {
        servers: [
          {
            id: "home",
            name: "Home",
            url: "https://romm.local",
            auth: "token",
            username: "",
            allowInvalidCerts: false,
            importAll: true,
            platforms: [],
            collections: [],
            syncOnStartup: true,
            syncInterval: "6h",
          },
        ],
        cache: { dir: "", limitGb: 100, keepFavorites: true, verify: true },
      },
      servers: [
        {
          id: "home",
          state: "sign_in_needed",
          version: null,
          error: null,
          lastSyncMs: null,
          syncing: false,
          games: 0,
          hasSecret: false,
        },
      ],
      cache: { dir: "/games/romm-cache", used: 0, limit: null, games: [] },
      downloads: [],
    };
    const romm = {
      overview: async () => structuredClone(overview),
      subscribeDownloads: async () => () => {},
    } as unknown as RommApi;
    render(
      <QueryClientProvider client={new QueryClient()}>
        <EmulationProvider api={createEmulationFixtureApi()}>
          <RommApiContext.Provider value={romm}>
            <RommPanel />
          </RommApiContext.Provider>
        </EmulationProvider>
      </QueryClientProvider>,
    );
    await screen.findByText("Enter API token");
    expect(survey(seen, "romm")).toEqual([]);
    expect(survey(seen, "romm-servers")).toEqual([]);
    cleanup();
  });

  it("renders the Statistics dashboard in each state", () => {
    for (const status of ["loading", "error"] as const) {
      render(<StatisticsDashboard stats={fixtureStatsState("normal", { status, samples: [] })} />);
      expect(survey(seen, "system-stats")).toEqual([]);
      cleanup();
    }
    for (const [scenario, status] of [
      ["normal", "live"],
      ["unavailable", "stale"],
    ] as const) {
      render(<StatisticsDashboard stats={fixtureStatsState(scenario, { status })} />);
      expect(survey(seen, "system-stats")).toEqual([]);
      cleanup();
    }
  });

  it("renders the account picker, its PIN pad in each state, and the account menu", async () => {
    const ada = {
      id: "ada",
      name: "Ada",
      role: "admin" as const,
      avatar: "avatar:rocket",
      pin: "2468",
    };
    // An avatar a later release removed: initials.
    const kid = { id: "kid", name: "Kid", avatar: "avatar:retired" };
    const check = () =>
      ["account-picker", "pin-pad", "account-menu", "avatar"].flatMap((name) => survey(seen, name));
    const pinStatus = () =>
      document.querySelector('[data-cs="pin-pad"]')?.getAttribute("data-status");

    renderWholeApp("/", { accounts: [ada, kid] });
    await screen.findByRole("heading", { name: "Who's playing?" });
    expect(check()).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Ada, PIN locked" }));
    const field = await screen.findByLabelText("Enter Ada's PIN");
    expect(check()).toEqual([]);
    fireEvent.change(field, { target: { value: "1111" } });
    fireEvent.submit(field.closest("form") as HTMLFormElement);
    await waitFor(() => expect(pinStatus()).toBe("wrong"));
    expect(check()).toEqual([]);
    cleanup();

    renderWholeApp("/", { accounts: [{ ...ada, lockedUntilMs: Date.now() + 60_000 }, kid] });
    fireEvent.click(await screen.findByRole("button", { name: "Ada, PIN locked" }));
    await waitFor(() => expect(pinStatus()).toBe("locked"));
    expect(check()).toEqual([]);
    cleanup();

    renderWholeApp("/", { accounts: [ada, kid], activeAccount: "ada" });
    fireEvent.click(await screen.findByRole("button", { name: "Ada, account" }));
    await screen.findByRole("menuitem", { name: "Switch account" });
    expect(check()).toEqual([]);
  });

  it("renders Settings → Accounts, its editor and My profile", async () => {
    const family = [
      { id: "ada", name: "Ada", role: "admin" as const, pin: "2468" },
      { id: "kid", name: "Kid", avatar: "avatar:moon", maxAge: 12 },
    ];
    const check = () =>
      ["account-list", "account-editor", "account-profile"].flatMap((name) => survey(seen, name));
    renderWholeApp("/settings?tab=accounts", { accounts: family, activeAccount: "ada" });
    fireEvent.click(await screen.findByRole("button", { name: "Edit Kid" }));
    await screen.findByRole("dialog", { name: "Edit Kid" });
    expect(check()).toEqual([]);
    cleanup();
    renderWholeApp("/settings?tab=profile", { accounts: family, activeAccount: "kid" });
    await screen.findByRole("heading", { name: "My profile" });
    expect(check()).toEqual([]);
  });

  it("renders the game editor: a value of the user's, a refused save and the discard question", async () => {
    const app = renderApp(`/games/${first.id}`);
    await app.api.edits.save(first.id, { genres: { op: "set", values: ["Mine"] } }, null);
    act(() =>
      app.api.edits.addAutomaticArtwork(first.id, "cover", {
        id: "cover",
        source: "igdb:co1",
        url: "asset://cover",
        width: 600,
        height: 900,
      }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Edit details" }));
    const dialog = await screen.findByRole("dialog", { name: /^Edit / });
    await waitFor(() => expect(dialog.querySelector('[data-part="manual"]')).not.toBeNull());
    expect(survey(seen)).toEqual([]);
    // An invalid date, and a description a refresh changed meanwhile.
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Description" }), {
      target: { value: "Mine." },
    });
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Release date" }), {
      target: { value: "soon" },
    });
    act(() =>
      app.api.edits.refresh(first.id, (game) => {
        game.description = "Theirs.";
      }),
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await within(dialog).findByRole("button", { name: "Keep mine" });
    expect(survey(seen)).toEqual([]);
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await screen.findByRole("alertdialog", { name: "Discard your changes?" });
    expect(survey(seen)).toEqual([]);
  });

  it("renders a release's launch actions, its tools and a tool that didn't start", async () => {
    const app = renderApp(`/games/${first.id}`);
    // The first play action of a release with no launch becomes its own.
    const own = await app.api.actions.save(first.id, {
      ...blankAction("", "play"),
      name: "Play",
      target: { type: "executable", path: "/games/fixture/game", args: [], cwd: null, env: {} },
    });
    // As if Caskade installed it: its Windows programs run through Proton.
    const list = app.api.actions.list.bind(app.api.actions);
    vi.spyOn(app.api.actions, "list").mockImplementation(async (gameId) =>
      (await list(gameId)).map((release) => ({ ...release, proton: true })),
    );
    await app.api.setDefaultEntry?.(first.id, own.entryId);
    const add = (action: Partial<typeof own>) =>
      app.api.actions.save(first.id, { ...own, id: "", ...action });
    await add({ name: "Modded" });
    await add({ name: "Browser build", target: { type: "web", url: "https://example.com/" } });
    const tool = await add({
      name: "Configure",
      kind: "utility",
      target: {
        type: "executable",
        path: "/games/configure",
        args: ["--safe"],
        cwd: null,
        env: { MODS: "on" },
      },
    });
    // The game page: what Play starts, and the release's tools.
    await screen.findByRole("button", { name: "Configure" });
    expect(survey(seen, "play-choice")).toEqual([]);
    // A tool that can't start says why.
    vi.spyOn(app.api.actions, "runTool").mockRejectedValueOnce(new Error("no such program"));
    fireEvent.click(screen.getByRole("button", { name: "Configure" }));
    await screen.findByRole("alertdialog", { name: "Configure didn't start" });
    expect(survey(seen, "tool-failed")).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "OK" }));

    fireEvent.click(await screen.findByRole("button", { name: "Edit details" }));
    const dialog = await screen.findByRole("dialog", { name: /^Edit / });
    fireEvent.click(within(dialog).getByRole("tab", { name: /Actions/ }));
    // The built-in action, locked, with the default choice above and its
    // Proton build.
    await within(dialog).findByRole("option", { name: /Modded/ });
    await within(dialog).findAllByRole("button", { name: /Proton build/ });
    expect(survey(seen, "game-actions")).toEqual([]);
    // An emulator, web or plugin target is described, not edited.
    fireEvent.click(within(dialog).getByRole("option", { name: /Browser build/ }));
    expect(survey(seen, "game-actions")).toEqual([]);
    // A tool's arguments and variables, its delete, and a refused save.
    fireEvent.click(within(dialog).getByRole("option", { name: new RegExp(tool.name) }));
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Name" }), {
      target: { value: "" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save action" }));
    await within(dialog).findByRole("alert");
    expect(survey(seen, "game-actions")).toEqual([]);
  });

  it("renders a scope's lifecycle scripts, a test's result and the launch order", async () => {
    renderApp(`/games/${first.id}`);
    fireEvent.click(await screen.findByRole("button", { name: "Edit details" }));
    const dialog = await screen.findByRole("dialog", { name: /^Edit / });
    fireEvent.click(within(dialog).getByRole("tab", { name: /Scripts/ }));
    const panel = within(dialog)
      .getAllByRole("tabpanel")
      .find((each) => each.querySelector('[data-cs="scripts"]')) as HTMLElement;
    fireEvent.click(await within(panel).findByRole("button", { name: "Add script" }));
    fireEvent.change(within(panel).getByRole("textbox", { name: "Script" }), {
      target: { value: "echo ready" },
    });
    fireEvent.click(within(panel).getByRole("button", { name: "Save script" }));
    await within(panel).findByRole("option", { name: /New script/ });
    fireEvent.click(within(panel).getByRole("button", { name: "Test" }));
    await within(panel).findByText(/^Finished/);
    expect(survey(seen, "scripts")).toEqual([]);
    // A program with an argument, then a refused save.
    fireEvent.click(within(panel).getByRole("radio", { name: "Program" }));
    fireEvent.click(within(panel).getByRole("button", { name: "Add argument" }));
    fireEvent.change(within(panel).getByRole("textbox", { name: "Name" }), {
      target: { value: " " },
    });
    fireEvent.click(within(panel).getByRole("button", { name: "Save script" }));
    await within(panel).findByRole("alert");
    expect(survey(seen, "scripts")).toEqual([]);
  });

  it("renders Settings → Devices, its pairing dialogs and the status bar's devices", async () => {
    const sync = createSyncFixtureApi();
    const now = Date.now();
    const transfer = (state: Transfer["state"], index: number): Transfer => ({
      id: `t${index}`,
      title: "Hades",
      deviceName: "Steam Deck",
      direction: index % 2 === 0 ? "incoming" : "outgoing",
      doneBytes: 1_000_000,
      totalBytes: 4_000_000,
      bytesPerSecond: 500_000,
      link: "lan",
      state,
    });
    sync.setTransfers((["running", "waitingDirect", "done", "failed"] as const).map(transfer));
    // Shares in every state, someone receiving one, and one received here.
    const shares = createSharesFixtureApi({
      shares: (["preparing", "ready", "failed", "stopped", "expired"] as const).map((state, i) =>
        fixtureShare({
          id: `s${i}`,
          title: i === 0 ? "Celeste" : `Share ${i}`,
          expiresAtMs: i % 2 === 0 ? null : now + 86_400_000,
          state:
            state === "preparing"
              ? { state, done: 1, total: 2 }
              : state === "failed"
                ? { state, reason: "a file went" }
                : { state },
          recipients: [
            { name: "Sam", done: 1, finished: false, connected: true, serving: true },
            { name: "Kim", done: 2, finished: true, connected: false, serving: false },
            { name: "Lee", done: 0, finished: false, connected: false, serving: false },
          ],
        }),
      ),
      received: (["receiving", "serving", "done"] as const).map((state) => ({
        job: `j-${state}`,
        title: "Hades",
        sharer: "Alex",
        receiving: state === "receiving",
        keepHelping: state !== "done",
        serving: state !== "done",
        helping: 1,
      })),
    });
    const names = [
      "peer-link",
      "paired-device",
      "transfer",
      "share",
      "share-recipient",
      "received-share",
      "status-bar",
      "pair-device",
    ];
    const check = () => names.flatMap((name) => survey(seen, name));
    renderWholeApp("/settings?tab=devices", {
      wrap: (children: ReactNode) => (
        <SyncProvider api={sync}>
          <SharesProvider api={shares}>{children}</SharesProvider>
        </SyncProvider>
      ),
    });
    await screen.findByText(/^Celeste/);
    await waitFor(() =>
      expect(document.querySelector('[data-cs="status-bar"] [data-part="sync"]')).toBeTruthy(),
    );
    expect(check()).toEqual([]);
    // A pairing's code, then a link opened from outside.
    fireEvent.click(await screen.findByRole("button", { name: "Pair…" }));
    await screen.findByRole("dialog");
    expect(check()).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "They don't match" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    sync.openLink({ deviceId: "7".repeat(64), expiresAtMs: now + 60_000, expired: false });
    await screen.findByRole("dialog");
    expect(check()).toEqual([]);
  });

  it("renders Share with someone… with its QR code, and Receive a shared game…", async () => {
    // The library's grid is virtual: it needs a viewport.
    const undoLayout = fakeLayout();
    fakeViewport();
    onTestFinished(() => {
      undoLayout();
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    });
    const shares = createSharesFixtureApi();
    const app = renderWholeApp("/library", {
      mode: "console",
      games: playedGames(3),
      wrap: (children: ReactNode) => <SharesProvider api={shares}>{children}</SharesProvider>,
    });
    const names = ["share-game", "share-recipient", "receive-share", "share-info"];
    const check = () => names.flatMap((name) => survey(seen, name));
    // A game's menu → Share with someone… → Make a link; then someone opens it.
    // Console mode opens on the first game; Start opens its menu.
    await waitFor(() =>
      expect(document.activeElement?.closest('[data-cs="game-card"]')).toBeTruthy(),
    );
    app.pad.press("menu");
    const menu = await screen.findByRole("menu");
    await waitFor(() =>
      expect(
        within(menu)
          .getAllByRole("menuitem")
          .map((item) => item.textContent),
      ).toContain("Share with someone…A link for someone else's Caskade"),
    );
    fireEvent.click(within(menu).getByRole("menuitem", { name: /Share with someone…/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Make a link" }));
    await screen.findByRole("img", { name: "The link as a QR code" });
    const [share] = await shares.list();
    shares.receivedBy(share?.id ?? "", {
      name: "Sam",
      done: 1,
      finished: false,
      connected: true,
      serving: false,
    });
    await screen.findByText("Sam");
    expect(check()).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

    // Add game → Receive a shared game…: a link reached directly, then one
    // only a relay reaches.
    fireEvent.click(screen.getByRole("button", { name: "Add game" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: /Receive a shared game…/ }));
    const field = await screen.findByRole("textbox", { name: "Share link" });
    for (const [link, said] of [
      [FIXTURE_LINK, "Alex's Desk shares Hades with you."],
      [`${FIXTURE_LINK}relay`, "Alex's Desk shares Hades with you."],
      [`${FIXTURE_LINK}save`, "Alex's Desk shares a save of Hades with you."],
      [`${FIXTURE_LINK}collection`, "Alex's Desk shares the collection Co-op nights with you."],
    ] as const) {
      fireEvent.change(field, { target: { value: link } });
      fireEvent.click(screen.getByRole("button", { name: "Look" }));
      await screen.findByText(said);
      expect(check()).toEqual([]);
    }
  });

  it("renders a release's save locations, its warnings and a refused save", async () => {
    const { api } = renderApp(`/games/${first.id}`);
    const entryId = first.entries[0]?.id ?? "";
    api.saves.seed({
      entryId,
      revision: 0,
      preferences: { manifestTitle: null, locations: null },
      matched: { type: "matched", title: first.title, method: "steam" },
      automatic: [{ spec: { kind: "folder", path: "/saves/fixture" }, tags: [], fileCount: 1 }],
      locations: [{ spec: { kind: "folder", path: "/saves/fixture" }, tags: [], fileCount: 1 }],
      warnings: ["This release is not installed on this device."],
    });
    fireEvent.click(await screen.findByRole("button", { name: "Edit details" }));
    const dialog = await screen.findByRole("dialog", { name: /^Edit / });
    fireEvent.click(within(dialog).getByRole("tab", { name: /Saves/ }));
    const tab = dialog.querySelector('[data-cs="game-saves"]') as HTMLElement;
    await within(tab).findByText("/saves/fixture");
    expect(survey(seen, "game-saves")).toEqual([]);
    // No locations chosen, and a save another editor got to first.
    fireEvent.click(within(tab).getByRole("button", { name: "Remove /saves/fixture" }));
    await api.saves.edit(entryId, { manifestTitle: null, locations: null }, 0);
    fireEvent.click(within(tab).getByRole("button", { name: "Save locations" }));
    await within(tab).findByRole("alert");
    expect(survey(seen, "game-saves")).toEqual([]);
  });

  it("renders selection mode and the bulk editor's steps", async () => {
    fakeViewport();
    onTestFinished(() => {
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    });
    const titled = Array.from({ length: 3 }, (_, i) => ({
      ...first,
      id: `pick-${i}`,
      title: `Pick ${i}`,
    }));
    renderApp("/titles", { library: titled });
    fireEvent.click(await screen.findByRole("button", { name: "Select" }));
    fireEvent.click(screen.getByRole("link", { name: /^Pick 0/ }));
    const bar = screen.getByRole("toolbar", { name: "Selected games" });
    expect(survey(seen)).toEqual([]);
    fireEvent.click(within(bar).getByRole("button", { name: "Edit selected…" }));
    const dialog = await screen.findByRole("dialog", { name: "Edit 1 game" });
    fireEvent.click(within(dialog).getByRole("checkbox", { name: "Change Tags" }));
    const tags = within(dialog).getByRole("textbox", { name: "Add to Tags" });
    fireEvent.change(tags, { target: { value: "co-op" } });
    fireEvent.keyDown(tags, { key: "Enter" });
    expect(survey(seen)).toEqual([]);
    fireEvent.click(within(dialog).getByRole("checkbox", { name: "Change Release date" }));
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Release date" }), {
      target: { value: "soon" },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Preview" }));
    await within(dialog).findByRole("region", { name: "Not saved" });
    expect(survey(seen)).toEqual([]);
    fireEvent.click(within(dialog).getByRole("button", { name: "Back" }));
    fireEvent.click(within(dialog).getByRole("checkbox", { name: "Change Release date" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Preview" }));
    await within(dialog).findByRole("region", { name: "What changes" });
    expect(survey(seen)).toEqual([]);
    fireEvent.click(within(dialog).getByRole("button", { name: "Apply" }));
    await within(dialog).findByRole("region", { name: "Saved" });
    expect(survey(seen)).toEqual([]);
  });

  it("renders a chosen row in the list layout", async () => {
    fakeViewport();
    onTestFinished(() => {
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    });
    renderApp("/list");
    fireEvent.click(await screen.findByRole("button", { name: "Select" }));
    const [row] = await screen.findAllByRole("link", { name: new RegExp(first.title) });
    if (!row) throw new Error("no row");
    fireEvent.click(row);
    await waitFor(() =>
      expect(document.querySelector('[data-cs="game-row"][data-selected="true"]')).not.toBeNull(),
    );
    expect(survey(seen)).toEqual([]);
  });

  for (const [where, path, root] of [
    ["a card", "/library", "game-card"],
    ["a row", "/list", "game-row"],
  ] as const)
    it(`renders a game starting from a double click on ${where}`, async () => {
      fakeViewport();
      onTestFinished(() => {
        vi.restoreAllMocks();
        vi.unstubAllGlobals();
      });
      const app = renderApp(path);
      // The launch never answers, so it stays starting.
      const launch = vi
        .spyOn(app.api, "launchGame")
        .mockImplementation(() => new Promise(() => {}));
      // The first game has nothing that can launch; the second has.
      const second = games[1] as (typeof games)[number];
      const links = await screen.findAllByRole("link", { name: new RegExp(second.title) });
      const link = links.find((each) => each.closest(`[data-cs="${root}"]`));
      if (!link) throw new Error(`no ${root}`);
      fireEvent.doubleClick(link);
      await waitFor(() => expect(launch).toHaveBeenCalled());
      expect(document.querySelector(`[data-cs="${root}"] [data-part="starting"]`)).not.toBeNull();
      expect(survey(seen)).toEqual([]);
    });

  it("renders adding a game by hand: a target, a refusal and duplicates", async () => {
    renderApp("/library");
    const open = async (item: string) => {
      fireEvent.click(await screen.findByRole("button", { name: "Add game" }));
      fireEvent.click(await screen.findByRole("menuitem", { name: item }));
      return screen.findByRole("dialog");
    };
    let dialog = await open("Add a game by title…");
    expect(survey(seen)).toEqual([]);
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    dialog = await open("Add a game on this computer…");
    const input = within(dialog).getByRole("textbox", { name: /^Program, shortcut or URI/ });
    fireEvent.change(input, { target: { value: "/games/missing.exe" } });
    fireEvent.blur(input);
    await within(dialog).findByRole("alert");
    expect(survey(seen)).toEqual([]);
    fireEvent.change(input, { target: { value: "/games/Fixture/fixture.sh" } });
    fireEvent.blur(input);
    await within(dialog).findByText("/games/Fixture/fixture.sh");
    fireEvent.change(within(dialog).getByRole("textbox", { name: /^Title/ }), {
      target: { value: first.title },
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Add game" }));
    await within(dialog).findByRole("button", { name: "Add to this game" });
    expect(survey(seen)).toEqual([]);
  });

  it("renders refreshing chosen games' details and its Activity card", async () => {
    fakeViewport();
    onTestFinished(() => {
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    });
    const picks = ["pick-0", "fail-1"].map((id, i) => ({ ...first, id, title: `Pick ${i}` }));
    const app = renderApp("/titles", { library: picks });
    fireEvent.click(await screen.findByRole("button", { name: "Select" }));
    fireEvent.click(screen.getByRole("link", { name: /^Pick 0/ }));
    fireEvent.click(screen.getByRole("link", { name: /^Pick 1/ }));
    const bar = screen.getByRole("toolbar", { name: "Selected games" });
    fireEvent.click(within(bar).getByRole("button", { name: "Refresh details…" }));
    const dialog = await screen.findByRole("dialog");
    expect(survey(seen)).toEqual([]);
    fireEvent.click(within(dialog).getByRole("button", { name: "Refresh" }));
    await within(dialog).findByRole("link", { name: "Follow it in Activity" });
    act(() => app.router.history.push("/jobs"));
    const card = await screen.findByRole("region", { name: "Refreshing game details" });
    expect(survey(seen)).toEqual([]);
    fireEvent.click(within(card).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(card.dataset.status).toBe("cancelled"));
    expect(survey(seen)).toEqual([]);
    fireEvent.click(within(card).getByRole("button", { name: /^Continue/ }));
    await waitFor(() => expect(card.dataset.status).toBe("running"));
    act(() => app.api.edits.finishRefresh());
    await waitFor(() => expect(card.dataset.status).toBe("done"));
    expect(survey(seen)).toEqual([]);
  });

  it("renders the add-on catalogue and an add-on's details in each state", async () => {
    const addons = createAddonsFixture();
    const progress: ((next: AddonProgress) => void)[] = [];
    const subscribe = addons.subscribeProgress;
    addons.subscribeProgress = (listener) => {
      progress.push(listener);
      return subscribe(listener);
    };
    // One loaded screenshot; the others can't load.
    vi.spyOn(addons, "media").mockImplementation(async (id) =>
      id.includes("howlongtobeat") ? "data:image/webp;base64,UklGRg==" : null,
    );
    renderWholeApp("/settings?tab=plugins", {
      wrap: (children) => <AddonsProvider api={addons}>{children}</AddonsProvider>,
    });
    const check = () => ["addon-catalogue", "addon-details"].flatMap((name) => survey(seen, name));
    const list = await screen.findByRole("list", { name: "Catalogue" });
    await within(list).findByText("Paper");
    const open = async (name: string) => {
      fireEvent.click(within(list).getByRole("button", { name: `Details of ${name}` }));
      return screen.findByRole("dialog", { name });
    };
    const close = async (dialog: HTMLElement) => {
      fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));
      await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    };
    expect(check()).toEqual([]);

    let dialog = await open("From the future");
    expect(check()).toEqual([]);
    await close(dialog);

    dialog = await open("Paper");
    expect(check()).toEqual([]);
    await close(dialog);

    dialog = await open("HowLongToBeat");
    await within(dialog).findByRole("img", { name: "HowLongToBeat screenshot" });
    act(() => {
      for (const listener of progress)
        listener({ id: "dev.caskade.howlongtobeat", phase: "downloading", received: 1, total: 2 });
    });
    fireEvent.click(within(dialog).getByRole("button", { name: "Uninstall…" }));
    await within(dialog).findByRole("checkbox");
    expect(check()).toEqual([]);
    await close(dialog);

    dialog = await open("Playtime stats");
    fireEvent.click(within(dialog).getByRole("button", { name: "Install" }));
    const review = await waitFor(() => {
      const found = document.querySelector<HTMLElement>('[data-cs="plugin-review"]');
      if (!found) throw new Error("no review yet");
      return found;
    });
    fireEvent.click(
      within(review).getByRole("button", { name: "Install and allow", hidden: true }),
    );
    await within(dialog).findByText("0.2.0 is installed.");
    expect(check()).toEqual([]);
    cleanup();

    const failing = createAddonsFixture();
    failing.setOffline(true);
    renderWholeApp("/settings?tab=plugins", {
      wrap: (children) => <AddonsProvider api={failing}>{children}</AddonsProvider>,
    });
    fireEvent.click(await screen.findByRole("button", { name: "Details of Playtime stats" }));
    dialog = await screen.findByRole("dialog", { name: "Playtime stats" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Install" }));
    await within(dialog).findByRole("alert");
    expect(check()).toEqual([]);
  });

  it("renders a game's achievements in every state, and the hero's summary", async () => {
    const check = () =>
      ["achievements", "achievement-summary"].flatMap((name) => survey(seen, name));
    const waiting = staleGogSet();
    waiting.sync.retryAtMs = Date.now() + 60 * 60 * 1000;
    const sets = [
      steamSet(16),
      waiting,
      retroSet(),
      emptySet("not_fetched", "steam", "Steam", "New"),
      emptySet("no_achievements", "steam", "Steam", "None"),
      emptySet("unsupported", "gog", "GOG", "Gog"),
      emptySet("private", "steam", "Steam", "Shy"),
      emptySet("sign_in_needed", "steam", "Steam", "Gone"),
    ];
    const api = createAchievementsFixture(() => sets);
    let answer: (value: AchievementSet[]) => void = () => {};
    vi.spyOn(api, "sets").mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          answer = resolve;
        }),
    );
    renderWholeApp(`/games/${first.id}`, {
      wrap: (children) => <AchievementsProvider api={api}>{children}</AchievementsProvider>,
    });
    // Loading, then each set in turn.
    await waitFor(() => expect(document.querySelector('[data-cs="achievements"]')).not.toBeNull());
    expect(check()).toEqual([]);
    await act(async () => answer(structuredClone(sets)));
    const section = await screen.findByRole("region", { name: "Achievements" });
    expect(check()).toEqual([]);
    fireEvent.click(within(section).getByRole("switch", { name: /hidden achievement/ }));
    for (const radio of within(section).getAllByRole("radio", { name: / · / })) {
      fireEvent.click(radio);
      expect(check()).toEqual([]);
    }
    // A refresh under way can be stopped.
    vi.spyOn(api, "refresh").mockImplementationOnce(() => new Promise(() => {}));
    fireEvent.click(within(section).getByRole("button", { name: "Refresh achievements" }));
    await within(section).findByRole("button", { name: "Stop refreshing" });
    expect(check()).toEqual([]);
    cleanup();

    // A failed read.
    const failing = createAchievementsFixture();
    vi.spyOn(failing, "sets").mockRejectedValue(new Error("the library is busy"));
    renderWholeApp(`/games/${first.id}`, {
      wrap: (children) => <AchievementsProvider api={failing}>{children}</AchievementsProvider>,
    });
    await screen.findByText(/Achievements couldn't be read/);
    expect(check()).toEqual([]);
  });

  it("covers everything the shell owns, and the shared components' app-only extras", () => {
    const missing: string[] = [];
    for (const [name, entry] of Object.entries(anatomy) as [string, AnatomyEntry][]) {
      const found = seen.get(name) ?? new Set<string>();
      const appOnly = new Set(entry.appOnly ?? []);
      const owned = entry.source === "app";
      if (owned && !found.has("(root)")) missing.push(`${name}: root`);
      for (const part of Object.keys(entry.parts))
        if ((owned || appOnly.has(part)) && !found.has(part)) missing.push(`${name}: part ${part}`);
      for (const [attribute, values] of Object.entries(entry.states))
        if (owned || appOnly.has(attribute))
          for (const value of values)
            if (!found.has(`${attribute}=${value}`)) missing.push(`${name}: ${attribute}=${value}`);
    }
    expect(missing).toEqual([]);
  });
});

/** A plugin contribution that fails to draw (#109). */
function Broken(): never {
  throw new Error("crashy failed to draw");
}
