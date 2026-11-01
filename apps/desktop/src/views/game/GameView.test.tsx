import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { createFixtureApi, createFixtureGames } from "../../api/fixtures";
import { type GameIgdbApi, GameIgdbProvider } from "../../api/igdb";
import { createLibraryQueryClient, LibraryProvider } from "../../api/provider";
import { HomeView } from "../home/HomeView";
import { GameView } from "./GameView";

afterEach(cleanup);
it("launches from details and refreshes session history, dashboard and playtime without reload", async () => {
  const root = createRootRoute({ component: Outlet });
  const home = createRoute({ getParentRoute: () => root, path: "/", component: HomeView });
  const detail = createRoute({
    getParentRoute: () => root,
    path: "/games/$gameId",
    component: () => <GameView gameId="fixture-1" />,
  });
  const library = createRoute({
    getParentRoute: () => root,
    path: "/library",
    component: () => null,
  });
  const router = createRouter({
    routeTree: root.addChildren([home, detail, library]),
    history: createMemoryHistory({ initialEntries: ["/games/fixture-1"] }),
  });
  const api = createFixtureApi();
  render(
    <LibraryProvider api={api} client={createLibraryQueryClient()}>
      <RouterProvider router={router} />
    </LibraryProvider>,
  );
  await screen.findByRole("heading", { name: "Fixture Game 0002" });
  fireEvent.click(screen.getByRole("button", { name: "Play" }));
  await screen.findByText("Playing now");
  // The running entry offers Stop in place of Play (#150).
  expect(screen.getByText("Playing")).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Play" })).toBeNull();
  const [session] = await api.listSessions("fixture-1");
  if (!session) throw new Error("session missing");
  act(() => api.finishSession(session.id, 3600));
  await waitFor(() => expect(screen.getByText("1.3h played")).toBeTruthy());
  await screen.findByRole("button", { name: "Play" });

  // Stop ends a running game; its session keeps the playtime.
  fireEvent.click(screen.getByRole("button", { name: "Play" }));
  fireEvent.click(await screen.findByRole("button", { name: "Stop" }));
  await screen.findByRole("button", { name: "Play" });
  const sessions = await api.listSessions("fixture-1");
  expect(sessions.every((saved) => saved.endedAt !== null)).toBe(true);
  await act(async () => {
    await router.navigate({ to: "/" });
  });
  await screen.findByRole("heading", { name: "Recently Played" });
  await screen.findByRole("heading", { name: "Fixture Game 0002" });
  expect(screen.getByText("Continue Playing")).toBeTruthy();
});

it("asks for an emulator and a disc, then launches with the answers", async () => {
  const root = createRootRoute({ component: Outlet });
  const detail = createRoute({
    getParentRoute: () => root,
    path: "/games/$gameId",
    component: () => <GameView gameId="fixture-1" />,
  });
  const router = createRouter({
    routeTree: root.addChildren([detail]),
    history: createMemoryHistory({ initialEntries: ["/games/fixture-1"] }),
  });
  const api = createFixtureApi(createFixtureGames(), {
    "entry-1": {
      entryId: "entry-1",
      platformId: "sony_playstation",
      profiles: [
        {
          emulatorId: "ra",
          emulatorName: "RetroArch",
          profileId: "psx",
          profileName: "Beetle PSX HW",
          installed: true,
        },
        {
          emulatorId: "duck",
          emulatorName: "DuckStation",
          profileId: "default",
          profileName: "Default",
          installed: false,
        },
      ],
      discs: [
        { index: 0, name: "Disc 1" },
        { index: 1, name: "Disc 2" },
      ],
    },
  });
  const launch = vi.spyOn(api, "launchGame");
  render(
    <LibraryProvider api={api} client={createLibraryQueryClient()}>
      <RouterProvider router={router} />
    </LibraryProvider>,
  );
  await screen.findByRole("heading", { name: "Fixture Game 0002" });
  fireEvent.click(screen.getByRole("button", { name: "Play" }));
  const dialog = await screen.findByRole("dialog", { name: "Choose how to play" });
  const play = within(dialog).getByRole("button", { name: "Play" }) as HTMLButtonElement;
  expect(play.disabled).toBe(true);
  expect(
    (within(dialog).getByRole("radio", { name: /DuckStation/ }) as HTMLInputElement).disabled,
  ).toBe(true);
  fireEvent.click(within(dialog).getByRole("radio", { name: /RetroArch/ }));
  fireEvent.click(within(dialog).getByRole("radio", { name: "Disc 2" }));
  fireEvent.click(play);
  await screen.findByText("Playing now");
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(launch).toHaveBeenLastCalledWith("entry-1", {
    profile: { emulatorId: "ra", profileId: "psx" },
    disc: 1,
  });
});

it("asks for the page's images once it opens", async () => {
  const root = createRootRoute({ component: Outlet });
  const detail = createRoute({
    getParentRoute: () => root,
    path: "/games/$gameId",
    component: () => <GameView gameId="fixture-1" />,
  });
  const router = createRouter({
    routeTree: root.addChildren([detail]),
    history: createMemoryHistory({ initialEntries: ["/games/fixture-1"] }),
  });
  const api = createFixtureApi();
  const pageImages = vi.fn(async () => {});
  render(
    <LibraryProvider api={{ ...api, pageImages }} client={createLibraryQueryClient()}>
      <RouterProvider router={router} />
    </LibraryProvider>,
  );
  await screen.findByRole("heading", { name: "Fixture Game 0002" });
  expect(pageImages).toHaveBeenCalledExactlyOnceWith("fixture-1");
});

it("plays the release picked at the top, and remembers it", async () => {
  const root = createRootRoute({ component: Outlet });
  const detail = createRoute({
    getParentRoute: () => root,
    path: "/games/$gameId",
    component: () => <GameView gameId="fixture-1" />,
  });
  const router = createRouter({
    routeTree: root.addChildren([detail]),
    history: createMemoryHistory({ initialEntries: ["/games/fixture-1"] }),
  });
  const games = createFixtureGames(2);
  const game = games[1];
  if (!game) throw new Error("No fixture game");
  game.entries = [
    { ...(game.entries[0] as (typeof game.entries)[number]), id: "steam-1", source: "steam" },
    { ...(game.entries[0] as (typeof game.entries)[number]), id: "gog-1", source: "gog" },
  ];
  const api = createFixtureApi(games);
  const launch = vi.spyOn(api, "launchGame");
  render(
    <LibraryProvider api={api} client={createLibraryQueryClient()}>
      <RouterProvider router={router} />
    </LibraryProvider>,
  );
  await screen.findByRole("heading", { name: "Fixture Game 0002" });
  // The game's own Play comes first; each release keeps one of its own.
  expect(screen.getAllByRole("button", { name: "Play" })).toHaveLength(3);

  fireEvent.click(screen.getByRole("button", { name: /Release/ }));
  fireEvent.click(await screen.findByRole("option", { name: "GOG" }));
  await waitFor(() =>
    expect(api.getGame("fixture-1")).resolves.toMatchObject({ defaultEntryId: "gog-1" }),
  );
  await waitFor(() =>
    expect(within(screen.getByRole("button", { name: /Release/ })).getByText("GOG")).toBeTruthy(),
  );

  fireEvent.click(screen.getAllByRole("button", { name: "Play" })[0] as HTMLElement);
  await waitFor(() => expect(launch).toHaveBeenCalledWith("gog-1", undefined));
});

it("links a web game to its live page", async () => {
  const root = createRootRoute({ component: Outlet });
  const detail = createRoute({
    getParentRoute: () => root,
    path: "/games/$gameId",
    component: () => <GameView gameId="fixture-1" />,
  });
  const router = createRouter({
    routeTree: root.addChildren([detail]),
    history: createMemoryHistory({ initialEntries: ["/games/fixture-1"] }),
  });
  const games = createFixtureGames();
  const web = games[1];
  if (!web) throw new Error("fixture missing");
  web.website = "https://www.armorgames.com/play/1716/gemcraft";
  render(
    <LibraryProvider api={createFixtureApi(games)} client={createLibraryQueryClient()}>
      <RouterProvider router={router} />
    </LibraryProvider>,
  );
  const link = await screen.findByRole("link", { name: /Official site/ });
  expect(link.getAttribute("href")).toBe("https://www.armorgames.com/play/1716/gemcraft");
  expect(link.getAttribute("target")).toBe("_blank");
  expect(link.textContent).toContain("armorgames.com");
});

it("opens the game a new IGDB match merged this one into", async () => {
  const root = createRootRoute({ component: Outlet });
  const detail = createRoute({
    getParentRoute: () => root,
    path: "/games/$gameId",
    component: function Detail() {
      return <GameView gameId={detail.useParams().gameId} />;
    },
  });
  const router = createRouter({
    routeTree: root.addChildren([detail]),
    history: createMemoryHistory({ initialEntries: ["/games/fixture-1"] }),
  });
  const games = createFixtureGames();
  for (const entry of games[1]?.entries ?? []) entry.source = "caskade-storage";
  const api = createFixtureApi(games);
  const { removeGame } = api;
  if (!removeGame) throw new Error("fixture API can't remove games");
  // The match merges fixture-1 into fixture-2, and answers once the
  // merged game is filled in.
  let filledIn = () => {};
  const igdb: GameIgdbApi = {
    game: async () => null,
    async set() {
      await removeGame("fixture-1");
      await new Promise<void>((resolve) => {
        filledIn = resolve;
      });
      return { gameId: "fixture-2", igdbId: "72", source: "manual", pending: false, game: null };
    },
    rematch: async () => null,
    refresh: async () => null,
    search: async () => [],
    separate: async () => "",
    ageRatings: async () => null,
  };
  render(
    <LibraryProvider api={api} client={createLibraryQueryClient()}>
      <GameIgdbProvider api={igdb}>
        <RouterProvider router={router} />
      </GameIgdbProvider>
    </LibraryProvider>,
  );
  await screen.findByRole("heading", { name: "Fixture Game 0002" });
  fireEvent.click(await screen.findByRole("button", { name: "Set IGDB game" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Search IGDB" }), {
    target: { value: "72" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Use IGDB ID 72" }));
  // The game is gone while the merged one is filled in: the page waits.
  await screen.findByText("Loading game…");
  expect(screen.queryByText("Game not found")).toBeNull();

  act(() => filledIn());
  await screen.findByRole("heading", { name: "Fixture Game 0003" });
  expect(router.state.location.pathname).toBe("/games/fixture-2");
  expect(screen.queryByText("Game not found")).toBeNull();
});
