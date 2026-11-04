import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createFixtureApi, createFixtureGames } from "../../api/fixtures";
import { createInstallFixtureApi } from "../../api/installFixtures";
import { createLibraryQueryClient, LibraryProvider } from "../../api/provider";
import { StorageProvider } from "../../api/StorageProvider";
import type { RootSummary, StorageApi } from "../../api/storage";
import type { GameEntry } from "../../api/types";
import { GameMenuHost } from "../../components/gameMenu/GameMenuHost";
import { LibraryNav } from "../../shell/LibraryNav";
import { ActivityProvider } from "../activity/ActivityProvider";
import { LibraryCard } from "./LibraryCard";
import { LibraryRow } from "./LibraryRow";
import { LibraryView } from "./LibraryView";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(900);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(900);
  vi.stubGlobal("scrollTo", vi.fn());
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe(target: Element) {
        this.callback([
          {
            target,
            contentRect: { width: 900, height: 600 },
            borderBoxSize: [{ inlineSize: 900, blockSize: 600 }],
          },
        ]);
      }
      unobserve() {}
      disconnect() {}
      constructor(private callback: (entries: unknown[]) => void) {}
    },
  );
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

it.each(["grid", "list"] as const)(
  "bounds mounted cards for a 1,203-game %s and loads more on scroll",
  async (layout) => {
    const root = createRootRoute({ component: Outlet });
    const route = createRoute({
      getParentRoute: () => root,
      path: "/library",
      component: () => (
        <ActivityProvider api={createInstallFixtureApi()}>
          <LibraryView filter="all" layout={layout} />
        </ActivityProvider>
      ),
    });
    const detail = createRoute({
      getParentRoute: () => root,
      path: "/games/$gameId",
      component: () => null,
    });
    const router = createRouter({
      routeTree: root.addChildren([route, detail]),
      history: createMemoryHistory({ initialEntries: ["/library"] }),
    });
    const api = createFixtureApi(createFixtureGames(1203));
    const result = render(
      <LibraryProvider api={api} client={createLibraryQueryClient()}>
        <RouterProvider router={router} />
      </LibraryProvider>,
    );
    await screen.findAllByText("Fixture Game 0001");
    const mounted = () =>
      result.container.querySelectorAll(layout === "grid" ? '[data-cs="game-card"]' : "tbody tr")
        .length;
    expect(mounted()).toBeGreaterThan(0);
    expect(mounted()).toBeLessThan(100);
    const viewport = result.container.querySelector('[data-cs="virtual-library"]');
    expect(viewport).not.toBeNull();
    if (!viewport) return;
    Object.defineProperty(viewport, "scrollTop", { configurable: true, value: 20_000 });
    fireEvent.scroll(viewport);
    await waitFor(() => expect(screen.queryAllByText("Fixture Game 0001")).toHaveLength(0));
    await waitFor(() => expect(mounted()).toBeGreaterThan(0));
    expect(mounted()).toBeLessThan(100);
  },
);

it("lays the grid out for its box's width before the first paint", async () => {
  // A real ResizeObserver reports after paint; a grid laid out for a
  // default width first would be drawn twice, every card remounted.
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(2100);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  const root = createRootRoute({ component: Outlet });
  const route = createRoute({
    getParentRoute: () => root,
    path: "/library",
    component: () => (
      <ActivityProvider api={createInstallFixtureApi()}>
        <LibraryView filter="all" />
      </ActivityProvider>
    ),
  });
  const router = createRouter({
    routeTree: root.addChildren([route]),
    history: createMemoryHistory({ initialEntries: ["/library"] }),
  });
  const result = render(
    <LibraryProvider
      api={createFixtureApi(createFixtureGames(24))}
      client={createLibraryQueryClient()}
    >
      <RouterProvider router={router} />
    </LibraryProvider>,
  );
  await screen.findAllByText("Fixture Game 0001");
  const rows = result.container.querySelectorAll<HTMLElement>('[data-part="virtual-row"]');
  expect(rows[0]?.style.gridTemplateColumns).toBe("repeat(11,minmax(0,1fr))");
});

it("uses the 320px cover in the grid and the icon, never the cover, in the list", async () => {
  const [game] = createFixtureGames(1);
  if (!game) throw new Error("fixture missing");
  game.coverGrid = `caskade-asset://localhost/${"a".repeat(64)}/thumb-320.webp`;
  game.coverList = `caskade-asset://localhost/${"a".repeat(64)}/thumb-96.webp`;
  game.icon = `caskade-asset://localhost/${"b".repeat(64)}/thumb-96.webp`;
  const root = createRootRoute({
    component: () => (
      <>
        <LibraryCard game={game} />
        <table aria-label="Games">
          <tbody>
            <LibraryRow game={game} />
            <LibraryRow game={{ ...game, id: "no-icon", icon: null }} />
          </tbody>
        </table>
      </>
    ),
  });
  const route = createRoute({
    getParentRoute: () => root,
    path: "/games/$gameId",
    component: () => null,
  });
  const router = createRouter({
    routeTree: root.addChildren([route]),
    history: createMemoryHistory({ initialEntries: ["/"] }),
  });
  const result = render(<RouterProvider router={router} />);
  await screen.findAllByText(game.title);
  expect(
    Array.from(result.container.querySelectorAll("img"), (image) => image.getAttribute("src")),
  ).toEqual([game.coverGrid, game.icon]);
  expect(result.container.innerHTML).not.toContain("original.webp");
});

it("lists games as a table whose column headers sort it", async () => {
  const root = createRootRoute({ component: Outlet });
  const route = createRoute({
    getParentRoute: () => root,
    path: "/library",
    validateSearch: (search: Record<string, unknown>) => ({
      sort: (search.sort as "title" | "recent" | "playtime" | undefined) ?? "recent",
    }),
    component: function List() {
      const { sort } = route.useSearch();
      return (
        <ActivityProvider api={createInstallFixtureApi()}>
          <LibraryView filter="all" sort={sort} layout="list" />
        </ActivityProvider>
      );
    },
  });
  const router = createRouter({
    routeTree: root.addChildren([route]),
    history: createMemoryHistory({ initialEntries: ["/library"] }),
  });
  render(
    <LibraryProvider
      api={createFixtureApi(createFixtureGames(3))}
      client={createLibraryQueryClient()}
    >
      <RouterProvider router={router} />
    </LibraryProvider>,
  );
  const table = await screen.findByRole("table", { name: "Games" });
  await screen.findByRole("link", { name: "Fixture Game 0001" });
  expect(table.getAttribute("aria-rowcount")).toBe("4");
  expect(
    screen
      .getAllByRole("columnheader")
      .map((header) => header.textContent?.replace(/[↑↓]/, "").trim()),
  ).toEqual(["Title", "Status", "Last played", "Playtime", "Size", "Actions"]);
  const recent = screen.getByRole("columnheader", { name: /Last played/ });
  expect(recent.getAttribute("aria-sort")).toBe("descending");
  fireEvent.click(screen.getByRole("link", { name: "Title" }));
  await waitFor(() => expect(router.state.location.search).toMatchObject({ sort: "title" }));
  await waitFor(() =>
    expect(screen.getByRole("columnheader", { name: /Title/ }).getAttribute("aria-sort")).toBe(
      "ascending",
    ),
  );
  expect(
    screen.getByRole("columnheader", { name: /Last played/ }).getAttribute("aria-sort"),
  ).toBeNull();
});

it("puts Recently Played first and All Games last in the sidebar's filters", async () => {
  const root = createRootRoute({ component: Outlet });
  const route = createRoute({
    getParentRoute: () => root,
    path: "/library",
    component: () => (
      <ActivityProvider api={createInstallFixtureApi()}>
        <LibraryNav />
        <LibraryView filter="recent" />
      </ActivityProvider>
    ),
  });
  const router = createRouter({
    routeTree: root.addChildren([route]),
    history: createMemoryHistory({ initialEntries: ["/library?filter=recent"] }),
  });
  render(
    <LibraryProvider
      api={createFixtureApi(createFixtureGames(3))}
      client={createLibraryQueryClient()}
    >
      <RouterProvider router={router} />
    </LibraryProvider>,
  );
  const filters = await screen.findByRole("navigation", { name: "Library filters" });
  const labels = Array.from(filters.querySelectorAll("a"), (link) =>
    link.textContent?.replace(/\d+/, "").trim(),
  );
  expect(labels).toEqual([
    "Recently Played",
    "Installed",
    "Ready to Install",
    "Emulated",
    "Web",
    "Favorites",
    "All Games",
  ]);
  expect(filters.querySelector("[aria-current=page]")?.textContent).toMatch(/^Recently Played/);
  // Nothing played yet: the empty state offers every game.
  await screen.findByText("You haven't played anything yet.");
  expect(screen.getByRole("link", { name: "Show all games" })).toBeTruthy();
});

it("shows unavailable managed copies and opens lazy actions with the keyboard", async () => {
  const [game] = createFixtureGames(1);
  if (!game) throw new Error("fixture missing");
  game.status = "installed";
  game.entries = [
    { ...(game.entries[0] as GameEntry), id: "managed-copy", source: "caskade-storage" },
  ];
  const roots: RootSummary[] = [
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
          gameId: game.id,
          title: game.title,
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
  ];
  const unused = async (): Promise<never> => {
    throw new Error("Unexpected storage command");
  };
  const storage: StorageApi = {
    list: vi.fn(async () => roots),
    preferences: unused,
    add: unused,
    edit: unused,
    setDefault: unused,
    remove: unused,
    moveAndRemove: unused,
    chooseDirectory: unused,
    move: unused,
    uninstall: unused,
  };
  const root = createRootRoute({
    component: () => (
      <GameMenuHost>
        <LibraryCard game={game} />
      </GameMenuHost>
    ),
  });
  const detail = createRoute({
    getParentRoute: () => root,
    path: "/games/$gameId",
    component: () => null,
  });
  const router = createRouter({
    routeTree: root.addChildren([detail]),
    history: createMemoryHistory({ initialEntries: ["/"] }),
  });
  const view = render(
    <LibraryProvider api={createFixtureApi([game])} client={createLibraryQueryClient()}>
      <StorageProvider api={storage}>
        <RouterProvider router={router} />
      </StorageProvider>
    </LibraryProvider>,
  );
  await screen.findByText("Managed copy unavailable");
  const card = view.container.querySelector('[data-cs="game-card"]');
  expect(card?.getAttribute("data-storage-status")).toBe("unavailable");
  expect(card?.getAttribute("data-status")).toBe("installed");
  expect(storage.list).toHaveBeenCalledOnce();
  expect(screen.queryByRole("menuitem", { name: "Move installed files…" })).toBeNull();
  const link = screen.getByRole("link");
  link.focus();
  fireEvent.keyDown(link, { key: "F10", shiftKey: true });
  await screen.findByRole("menuitem", { name: "Move installed files…" });
  expect(storage.list).toHaveBeenCalledOnce();
  fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
});

it("hides a game from its card's actions", async () => {
  const root = createRootRoute({
    component: () => (
      <GameMenuHost>
        <Outlet />
      </GameMenuHost>
    ),
  });
  const route = createRoute({
    getParentRoute: () => root,
    path: "/library",
    component: () => (
      <ActivityProvider api={createInstallFixtureApi()}>
        <LibraryView filter="all" />
      </ActivityProvider>
    ),
  });
  const detail = createRoute({
    getParentRoute: () => root,
    path: "/games/$gameId",
    component: () => null,
  });
  const router = createRouter({
    routeTree: root.addChildren([route, detail]),
    history: createMemoryHistory({ initialEntries: ["/library"] }),
  });
  const api = createFixtureApi(createFixtureGames(3));
  render(
    <LibraryProvider api={api} client={createLibraryQueryClient()}>
      <RouterProvider router={router} />
    </LibraryProvider>,
  );
  fireEvent.contextMenu(await screen.findByRole("link", { name: /Fixture Game 0002/ }), {
    clientX: 40,
    clientY: 60,
  });
  fireEvent.click(await screen.findByRole("menuitem", { name: /Hide from library/ }));
  await waitFor(() => expect(screen.queryByText("Fixture Game 0002")).toBeNull());
  expect(screen.getByText("Fixture Game 0003")).toBeTruthy();
  expect(screen.queryByRole("menu")).toBeNull();
  expect((await api.hiddenGames?.())?.map((game) => game.title)).toEqual(["Fixture Game 0002"]);
});

it("loads only the pages around a jump deep into a large library", async () => {
  const api = createFixtureApi(createFixtureGames(8000));
  const list = api.listGames.bind(api);
  const offsets: number[] = [];
  api.listGames = (input) => {
    offsets.push(input.offset);
    return list(input);
  };
  const root = createRootRoute({ component: Outlet });
  const route = createRoute({
    getParentRoute: () => root,
    path: "/library",
    component: () => (
      <ActivityProvider api={createInstallFixtureApi()}>
        <LibraryView filter="all" sort="title" />
      </ActivityProvider>
    ),
  });
  const router = createRouter({
    routeTree: root.addChildren([route]),
    history: createMemoryHistory({ initialEntries: ["/library"] }),
  });
  const result = render(
    <LibraryProvider api={api} client={createLibraryQueryClient()}>
      <RouterProvider router={router} />
    </LibraryProvider>,
  );
  await screen.findAllByText("Fixture Game 0001");
  expect(offsets).toEqual([0]);

  // Four columns of 312px rows: row 1,000 starts at game 4,001.
  const viewport = result.container.querySelector('[data-cs="virtual-library"]');
  if (!viewport) throw new Error("no viewport");
  Object.defineProperty(viewport, "scrollTop", { configurable: true, value: 312_000 });
  fireEvent.scroll(viewport);
  await screen.findAllByText("Fixture Game 4001");
  expect(offsets).toEqual([0, 3900, 4000]);
});
