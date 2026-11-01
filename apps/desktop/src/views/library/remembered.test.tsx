import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { fakeViewport, playedGames, renderApp } from "../../test/harness";
import { rememberedLibraryView, resolveLibraryView } from "./remembered";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

it("comes back to the filter, sort and layout the library was left with", async () => {
  fakeViewport();
  const app = renderApp("/library", { games: playedGames(6) });
  await screen.findByRole("heading", { name: "Library" });
  expect(app.router.state.location.search).toMatchObject({
    filter: "recent",
    sort: "recent",
    layout: "grid",
  });
  fireEvent.click(screen.getByRole("link", { name: /^Installed/ }));
  fireEvent.click(await screen.findByRole("link", { name: "list view" }));
  fireEvent.click(await screen.findByRole("link", { name: "Playtime" }));
  await waitFor(() =>
    expect(app.router.state.location.search).toMatchObject({
      filter: "installed",
      sort: "playtime",
      layout: "list",
    }),
  );

  // Away to a game, and back through the sidebar.
  await app.router.navigate({ to: "/games/$gameId", params: { gameId: "fixture-1" } });
  fireEvent.click(await screen.findByRole("link", { name: "Library" }));
  await screen.findByRole("table", { name: "Games" });
  expect(app.router.state.location.search).toMatchObject({
    filter: "installed",
    sort: "playtime",
    layout: "list",
  });

  // A search shows every game but doesn't replace the remembered filter.
  await app.router.navigate({ to: "/library", search: { filter: "all", q: "Fixture" } });
  await app.router.navigate({ to: "/" });
  fireEvent.click(await screen.findByRole("link", { name: "Library" }));
  await waitFor(() =>
    expect(app.router.state.location.search).toMatchObject({ filter: "installed" }),
  );
  expect(app.router.state.location.search).not.toHaveProperty("q");
});

it("keeps the remembered view across restarts and ignores what it can't read", () => {
  expect(rememberedLibraryView()).toEqual({ filter: "recent", sort: "recent", layout: "grid" });
  localStorage.setItem(
    "caskade.library.view",
    JSON.stringify({ filter: "favorites", sort: "sideways", layout: "list" }),
  );
  expect(rememberedLibraryView()).toEqual({ filter: "favorites", sort: "recent", layout: "list" });
  // The URL wins over the remembered view, field by field.
  expect(resolveLibraryView({ sort: "title", layout: "mosaic" })).toEqual({
    filter: "favorites",
    sort: "title",
    layout: "list",
  });
  localStorage.setItem("caskade.library.view", "{not json");
  expect(rememberedLibraryView()).toEqual({ filter: "recent", sort: "recent", layout: "grid" });
});

it.each([undefined, "console"] as const)(
  "chooses and remembers Popular in %s mode",
  async (mode) => {
    fakeViewport();
    const app = renderApp("/library?filter=all", { games: playedGames(6), mode });
    await screen.findByRole("heading", { name: "Library" });
    fireEvent.click(screen.getByRole("button", { name: /Sort games/ }));
    fireEvent.click(await screen.findByRole("option", { name: "Popular" }));
    await waitFor(() =>
      expect(app.router.state.location.search).toMatchObject({ sort: "popular" }),
    );
    expect(rememberedLibraryView().sort).toBe("popular");
  },
);
