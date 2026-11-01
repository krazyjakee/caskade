import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import {
  act,
  cleanup,
  fireEvent,
  render,
  renderHook,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { createFixtureApi } from "../../api/fixtures";
import { createInstallFixtureApi, fixtureJob } from "../../api/installFixtures";
import { createLibraryQueryClient, LibraryProvider } from "../../api/provider";
import { type GameUpdate, type GameUpdatesApi, GameUpdatesProvider } from "../../api/updates";
import { ActivityProvider } from "../activity/ActivityProvider";
import { GameView } from "./GameView";
import { usePlayWithUpdates } from "./UpdateBeforePlay";

afterEach(cleanup);

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

function updatesApi(followed: GameUpdate[] = [update]) {
  return {
    game: vi.fn(async () => followed),
    check: vi.fn(async () => update),
    beforePlay: vi.fn(async () => (followed.length > 0 ? update : null)),
    skip: vi.fn(async (_: string, version: string | null) => ({
      ...update,
      skipped: version,
      available: version !== update.latest,
    })),
    setPrereleases: vi.fn(async () => update),
    install: vi.fn(async () => "update-job"),
  } satisfies GameUpdatesApi;
}

function renderGame(updates: GameUpdatesApi) {
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
  const library = createFixtureApi();
  const install = createInstallFixtureApi([]);
  render(
    <LibraryProvider api={library} client={createLibraryQueryClient()}>
      <ActivityProvider api={install}>
        <GameUpdatesProvider api={updates}>
          <RouterProvider router={router} />
        </GameUpdatesProvider>
      </ActivityProvider>
    </LibraryProvider>,
  );
  return { library, install };
}

it("offers a newer release before playing, and plays now when asked", async () => {
  const updates = updatesApi();
  renderGame(updates);
  // The entry says what's out.
  await screen.findByText("v1.1 is out");
  expect(screen.getByText(/you have v1\.0/)).toBeTruthy();

  fireEvent.click(screen.getByRole("button", { name: "Play" }));
  await screen.findByRole("heading", { name: "Update before playing?" });
  expect(updates.beforePlay).toHaveBeenCalledWith("entry-1");
  fireEvent.click(screen.getByRole("button", { name: "Play now" }));
  await screen.findByText("Playing now");
  expect(updates.install).not.toHaveBeenCalled();
});

it("asks the core before playing while the followed installs are still loading", async () => {
  const updates = updatesApi();
  // A game menu's Play comes before the game's updates have loaded.
  updates.game.mockImplementation(() => new Promise(() => {}));
  const start = vi.fn();
  const { result } = renderHook(() => usePlayWithUpdates("fixture-1", start), {
    wrapper: ({ children }) => (
      <LibraryProvider api={createFixtureApi()} client={createLibraryQueryClient()}>
        <GameUpdatesProvider api={updates}>{children}</GameUpdatesProvider>
      </LibraryProvider>
    ),
  });
  act(() => result.current.play("entry-1"));
  await waitFor(() => expect(result.current.dialog).toBeTruthy());
  expect(updates.beforePlay).toHaveBeenCalledWith("entry-1");
  expect(start).not.toHaveBeenCalled();
});

it("updates, then plays once the update job completes", async () => {
  const updates = updatesApi();
  const { install } = renderGame(updates);
  await screen.findByText("v1.1 is out");
  fireEvent.click(screen.getByRole("button", { name: "Play" }));
  fireEvent.click(await screen.findByRole("button", { name: "Update and play" }));
  await waitFor(() => expect(updates.install).toHaveBeenCalledWith("entry-1"));
  act(() =>
    install.emit(
      fixtureJob({ id: "update-job", kind: "update", status: "running", phase: "download" }),
    ),
  );
  await screen.findByText("Downloading the update… 40%");
  expect(screen.queryByText("Playing now")).toBeNull();
  act(() =>
    install.emit(
      fixtureJob({ id: "update-job", kind: "update", status: "completed", revision: 2 }),
    ),
  );
  await screen.findByText("Playing now");
});

it("skips a release from the entry", async () => {
  const updates = updatesApi();
  renderGame(updates);
  await screen.findByText("v1.1 is out");
  fireEvent.click(screen.getByRole("button", { name: "Skip this version" }));
  await screen.findByText("You skipped v1.1.");
  expect(updates.skip).toHaveBeenCalledWith("entry-1", "v1.1");
  expect(screen.getByRole("button", { name: "Offer it again" })).toBeTruthy();
});

it("plays installs no provider follows without asking", async () => {
  const updates = updatesApi([]);
  renderGame(updates);
  await screen.findByRole("heading", { name: "Fixture Game 0002" });
  await waitFor(() => expect(updates.game).toHaveBeenCalled());
  fireEvent.click(screen.getByRole("button", { name: "Play" }));
  await screen.findByText("Playing now");
  expect(updates.beforePlay).not.toHaveBeenCalled();
});
