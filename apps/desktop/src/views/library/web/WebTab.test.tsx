import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { createFixtureApi, createFixtureGames } from "../../../api/fixtures";
import { createInstallFixtureApi } from "../../../api/installFixtures";
import { createLibraryQueryClient, LibraryProvider } from "../../../api/provider";
import type { RootSummary, StorageApi } from "../../../api/storage";
import type { GameDetails } from "../../../api/types";
import { GameMenuHost } from "../../../components/gameMenu/GameMenuHost";
import { emptyStorage, renderApp } from "../../../test/harness";
import { ActivityProvider } from "../../activity/ActivityProvider";
import { LibraryView } from "../LibraryView";
import { AddWebGame } from "./AddWebGame";
import { tileSizes } from "./tiles";

afterEach(cleanup);

const DAY = 86_400_000;
const NOW = Date.parse("2026-10-28T12:00:00Z");

function webGame(id: string, overrides: Partial<GameDetails> = {}): GameDetails {
  const [template] = createFixtureGames(1);
  if (!template) throw new Error("no fixture");
  return {
    ...template,
    id,
    title: `Web ${id}`,
    favorite: false,
    playtimeSeconds: 0,
    lastPlayedAt: null,
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

it("makes a tile large for a favourite, a recent game or a top-quarter play time", () => {
  const games = [
    webGame("a"),
    webGame("fav", { favorite: true }),
    webGame("recent", { lastPlayedAt: new Date(NOW - 2 * DAY).toISOString() }),
    webGame("old", { lastPlayedAt: new Date(NOW - 30 * DAY).toISOString(), playtimeSeconds: 60 }),
    webGame("most", { playtimeSeconds: 9000 }),
    webGame("b"),
    webGame("c"),
    webGame("d"),
  ];
  const sizes = Object.fromEntries(tileSizes(games, NOW));
  expect(sizes).toEqual({
    a: "small",
    fav: "huge",
    recent: "large",
    // 8 games: the top quarter is the two most played.
    old: "large",
    most: "large",
    b: "small",
    c: "small",
    d: "small",
  });
  expect(Object.fromEntries(tileSizes([webGame("a"), webGame("b")], NOW))).toEqual({
    a: "small",
    b: "small",
  });
});

function renderWebTab(games: GameDetails[]) {
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
        <LibraryView filter="web" />
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
  const api = createFixtureApi(games);
  render(
    <LibraryProvider api={api} client={createLibraryQueryClient()}>
      <RouterProvider router={router} />
    </LibraryProvider>,
  );
  return api;
}

it("keeps web games out of the other tabs", async () => {
  const [pc] = createFixtureGames(1);
  if (!pc) throw new Error("no fixture");
  const api = createFixtureApi([pc, webGame("needle", { favorite: true })]);
  const titles = async (filter: "all" | "favorites" | "web") =>
    (await api.listGames({ filter, sort: "title", offset: 0, limit: 10 })).items.map(
      (game) => game.title,
    );
  expect(await titles("all")).toEqual([pc.title]);
  expect(await titles("favorites")).toEqual(pc.favorite ? [pc.title] : []);
  expect(await titles("web")).toEqual(["Web needle"]);
  const stats = await api.libraryStats();
  expect([stats.total, stats.web]).toEqual([1, 1]);
});

it("shows web games as a mosaic and adds one by its page", async () => {
  const [pc] = createFixtureGames(1);
  if (!pc) throw new Error("no fixture");
  renderWebTab([pc, webGame("needle", { favorite: true }), webGame("hay")]);

  const mosaic = await screen.findByRole("list", { name: "Web games" });
  const tiles = within(mosaic).getAllByRole("link");
  expect(tiles.map((tile) => tile.getAttribute("aria-label"))).toEqual(["Web hay", "Web needle"]);
  expect(
    within(mosaic)
      .getByRole("link", { name: "Web needle" })
      .closest("[data-tile]")
      ?.getAttribute("data-tile"),
  ).toBe("huge");
  expect(screen.queryByRole("link", { name: "grid view" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Add game" })).toBeNull();

  fireEvent.click(screen.getByRole("button", { name: "Add web game" }));
  const dialog = await screen.findByRole("dialog");
  fireEvent.change(within(dialog).getByLabelText(/Game page/), {
    target: { value: "https://www.poki.example/g/penalty#play" },
  });
  fireEvent.click(within(dialog).getByRole("button", { name: "Add" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(await within(mosaic).findByRole("link", { name: "poki.example" })).toBeTruthy();
});

it("says how to add the first web game", async () => {
  const [pc] = createFixtureGames(1);
  if (!pc) throw new Error("no fixture");
  renderWebTab([pc]);
  expect(await screen.findByText("No web games yet. Add one with Add web game.")).toBeTruthy();
});

function root(id: string): RootSummary {
  return {
    root: { id, path: `/fixture/${id}`, isDefault: id === "fast", label: id },
    status: "online",
    capabilities: {
      writable: true,
      executable: true,
      symlinks: true,
      caseSensitive: true,
      freeBytes: 1_000_000_000,
      totalBytes: 2_000_000_000,
      filesystem: "fixture",
      warnings: [],
    },
    folderTemplate: "{title}",
    games: [],
    gamesBytes: 0,
    otherBytes: 0,
  };
}

it("asks where to keep a web game played offline, then opens its capture", async () => {
  const api = createFixtureApi([]);
  const save = vi.spyOn(api, "saveWebGameOffline");
  const storage: StorageApi = {
    list: async () => [root("fast"), root("slow")],
    preferences: async () => ({ alwaysUseDefault: false }),
    add: vi.fn(),
    edit: vi.fn(),
    setDefault: vi.fn(),
    remove: vi.fn(),
    moveAndRemove: vi.fn(),
    chooseDirectory: vi.fn(),
    move: vi.fn(),
    uninstall: vi.fn(),
  };
  render(
    <LibraryProvider api={api} client={createLibraryQueryClient()}>
      <ActivityProvider api={createInstallFixtureApi([])} storageApi={storage}>
        <AddWebGame />
      </ActivityProvider>
    </LibraryProvider>,
  );
  fireEvent.click(screen.getByRole("button", { name: "Add web game" }));
  const dialog = await screen.findByRole("dialog");
  fireEvent.change(within(dialog).getByLabelText(/Game page/), {
    target: { value: "https://maddy.itch.io/celeste" },
  });
  fireEvent.click(within(dialog).getByRole("checkbox", { name: "Play offline" }));
  fireEvent.click(within(dialog).getByRole("button", { name: "Add" }));
  fireEvent.click(await screen.findByRole("radio", { name: /slow/ }));
  expect(save).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Install here" }));
  await waitFor(() => expect(save).toHaveBeenCalledExactlyOnceWith("fixture-web-0", "slow"));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

it("plays a web game offline from its menu, and removes the offline copy", async () => {
  const app = renderApp("/games/needle", {
    games: [webGame("needle")],
    storage: { ...emptyStorage, list: async () => [root("fast"), root("slow")] },
  });
  const save = vi.spyOn(app.library, "saveWebGameOffline");
  const remove = vi.spyOn(app.library, "removeWebOffline");
  const heading = await screen.findByRole("heading", { level: 1, name: "Web needle" });
  const menuItem = async (name: string) =>
    within(await screen.findByRole("menu")).findByRole("menuitem", { name });
  fireEvent.contextMenu(heading, { clientX: 120, clientY: 80, button: 2 });
  expect(screen.queryByRole("menuitem", { name: "Remove offline copy" })).toBeNull();
  fireEvent.click(await menuItem("Play offline…"));
  fireEvent.click(await screen.findByRole("radio", { name: /slow/ }));
  fireEvent.click(screen.getByRole("button", { name: "Install here" }));
  await waitFor(() => expect(save).toHaveBeenCalledExactlyOnceWith("needle", "slow"));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

  fireEvent.contextMenu(heading, { clientX: 120, clientY: 80, button: 2 });
  expect(screen.queryByRole("menuitem", { name: "Play offline…" })).toBeNull();
  fireEvent.click(await menuItem("Remove offline copy"));
  await waitFor(() => expect(remove).toHaveBeenCalledExactlyOnceWith("needle"));
});

it("removes a web game from its menu, where other games are hidden", async () => {
  const app = renderApp("/games/needle", { games: [webGame("needle")] });
  const remove = vi.spyOn(app.library, "removeWebGame");
  const heading = await screen.findByRole("heading", { level: 1, name: "Web needle" });
  fireEvent.contextMenu(heading, { clientX: 120, clientY: 80, button: 2 });
  const menu = await screen.findByRole("menu");
  const item = await within(menu).findByRole("menuitem", { name: /Remove…/ });
  expect(within(menu).queryByRole("menuitem", { name: /Hide from library/ })).toBeNull();
  fireEvent.click(item);
  const confirm = await screen.findByRole("alertdialog", { name: "Remove Web needle?" });
  expect(remove).not.toHaveBeenCalled();
  fireEvent.click(within(confirm).getByRole("button", { name: "Remove" }));
  await waitFor(() => expect(remove).toHaveBeenCalledExactlyOnceWith("needle"));
  expect(await app.library.getGame("needle")).toBeNull();
  await waitFor(() => expect(screen.queryByRole("heading", { name: "Web needle" })).toBeNull());
});
