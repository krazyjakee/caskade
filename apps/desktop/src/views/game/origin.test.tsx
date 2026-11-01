/** The game page's Back link (#195): it goes back to where the game was
 * opened, named after that page, and to the Library without one. Each test
 * clicks the visible link. */
import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { fakeViewport, playedGames, renderApp } from "../../test/harness";
import { backTarget, MAX_GAMES, returnPlace, STORE_GAMES, validReturn } from "./origin";

beforeEach(() => {
  fakeViewport();
  vi.stubGlobal("scrollTo", vi.fn());
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const back = (name: string) => screen.findByRole("link", { name });
const title = (name: string) => screen.findByRole("heading", { level: 1, name });

/** Opens a card's summary panel and its View more. */
async function viewMore(card: RegExp) {
  const [link] = await screen.findAllByRole("link", { name: card });
  if (!link) throw new Error("card missing");
  fireEvent.click(link);
  const panel = await screen.findByRole("dialog");
  fireEvent.click(within(panel).getByRole("button", { name: "View more" }));
}

it("keeps only app pages, and never more than a few games deep", () => {
  expect(validReturn("/library?filter=installed&q=a%20b")).toBe(
    "/library?filter=installed&q=a%20b",
  );
  expect(validReturn("/")).toBe("/");
  expect(validReturn("/collections/collection-weekend")).toBe("/collections/collection-weekend");
  for (const outside of [
    "https://evil.example/",
    "//evil.example",
    "/\\evil.example",
    "javascript:alert(1)",
    "library",
    "/settings?tab=general",
    "/nowhere",
    42,
    undefined,
  ])
    expect(validReturn(outside), String(outside)).toBeUndefined();

  // A game opened from a game keeps that game's own way back.
  const nested = `/games/b?from=${encodeURIComponent("/games/a?from=%2Flibrary")}`;
  expect(validReturn(nested)).toBe(nested);
  // A game's link that names itself skips itself.
  expect(backTarget("b", nested)).toBe("/games/a?from=%2Flibrary");
  // A bad link inside the chain ends it there.
  expect(validReturn("/games/a?from=https%3A%2F%2Fevil.example")).toBe("/games/a");

  // Deeper than MAX_GAMES: the oldest games go, the browsing page stays.
  let chain = "/";
  for (let i = 0; i < MAX_GAMES + 2; i++) chain = `/games/g${i}?from=${encodeURIComponent(chain)}`;
  const kept: string[] = [];
  let at = validReturn(chain);
  while (at?.startsWith("/games/")) {
    const url = new URL(at, "http://x");
    kept.push(url.pathname);
    at = url.searchParams.get("from") ?? undefined;
  }
  expect(kept).toEqual(Array.from({ length: MAX_GAMES }, (_, i) => `/games/g${MAX_GAMES + 1 - i}`));
  expect(at).toBe("/");
});

it("chains the Store's catalog game pages like library ones (#163)", () => {
  const store = "/store?tab=upcoming&platform=6";
  expect(validReturn(store)).toBe(store);
  const similar = `/store/games/1001?from=${encodeURIComponent(store)}`;
  expect(validReturn(similar)).toBe(similar);
  // A catalog page that names itself skips itself.
  expect(backTarget("1001", similar, STORE_GAMES)).toBe(store);
  // A library game opened from a catalog page goes back to it.
  expect(backTarget("lantern", similar)).toBe(similar);
  expect(returnPlace(similar)).toEqual({ kind: "store-game", id: "1001" });
  expect(returnPlace(store)).toEqual({ kind: "page", name: "Store" });
});

it("goes back to Home from a game opened on Home", async () => {
  const app = renderApp("/", { games: playedGames(6) });
  fireEvent.click(await screen.findByRole("link", { name: "Resume · View details" }));
  await title("Fixture Game 0006");
  fireEvent.click(await back("Back to Home"));
  await waitFor(() => expect(app.router.state.location.pathname).toBe("/"));
  await screen.findByRole("heading", { name: "Welcome back" });
});

it("goes back to the library view the game was opened from, filter, sort and search kept", async () => {
  const app = renderApp("/library?filter=all&sort=title&q=Fixture");
  await viewMore(/Fixture Game 0002/);
  await title("Fixture Game 0002");
  fireEvent.click(await back("Back to Library"));
  await screen.findByRole("heading", { level: 1, name: "Results for “Fixture”" });
  expect(app.router.state.location.pathname).toBe("/library");
  expect(app.router.state.location.search).toMatchObject({
    filter: "all",
    sort: "title",
    q: "Fixture",
  });
});

it("goes back to the collection, by name", async () => {
  const app = renderApp("/collections/collection-weekend");
  await viewMore(/Fixture Game 0003/);
  await title("Fixture Game 0003");
  fireEvent.click(await back("Back to Weekend Picks"));
  await waitFor(() =>
    expect(app.router.state.location.pathname).toBe("/collections/collection-weekend"),
  );
  await screen.findByRole("heading", { level: 1, name: "Weekend Picks" });
});

it("keeps its way back through changes on the page: one click leaves", async () => {
  const app = renderApp("/", { games: playedGames(6) });
  fireEvent.click(await screen.findByRole("link", { name: "Resume · View details" }));
  await title("Fixture Game 0006");
  // An in-place change that keeps the URL but not the history state (a
  // tab, a filter): the way back stays.
  await act(() =>
    app.router.navigate({
      to: "/games/$gameId",
      params: { gameId: "fixture-5" },
      search: true,
      replace: true,
    }),
  );
  fireEvent.click(await back("Back to Home"));
  await waitFor(() => expect(app.router.state.location.pathname).toBe("/"));
});

it("walks back from a game opened on another game's page, then to the library", async () => {
  const app = renderApp("/library?filter=all&sort=title");
  await viewMore(/Fixture Game 0002/);
  await title("Fixture Game 0002");
  // The topbar search, on the game's page: its suggestion opens the
  // summary over the page, and View more the second game.
  const search = screen.getByRole("combobox", { name: "Search games" });
  act(() => search.focus());
  fireEvent.change(search, { target: { value: "0003" } });
  // Arrow down opens the list (typing alone doesn't under jsdom; see
  // anatomy.test.tsx), and the suggestion is picked once it is in.
  const suggestion = await waitFor(() => {
    if (search.getAttribute("aria-expanded") !== "true")
      fireEvent.keyDown(search, { key: "ArrowDown" });
    return screen.getByRole("option", { name: /Fixture Game 0003/ });
  });
  fireEvent.click(suggestion);
  const panel = await screen.findByRole("dialog", { name: "Fixture Game 0003" });
  fireEvent.click(within(panel).getByRole("button", { name: "View more" }));
  await title("Fixture Game 0003");

  fireEvent.click(await back("Back to Fixture Game 0002"));
  await title("Fixture Game 0002");
  fireEvent.click(await back("Back to Library"));
  await screen.findByRole("heading", { level: 1, name: "Library" });
  expect(app.router.state.location.search).toMatchObject({ filter: "all", sort: "title" });
});

it("goes to the Library from a game opened directly, without a loop", async () => {
  const app = renderApp("/games/fixture-1?from=https%3A%2F%2Fevil.example");
  await title("Fixture Game 0002");
  fireEvent.click(await back("Back to Library"));
  await screen.findByRole("heading", { level: 1, name: "Library" });
  // The game's entry was replaced: nothing to bounce back to.
  expect(app.router.history.canGoBack()).toBe(false);
});

it("goes back from the not-found page the same way", async () => {
  const app = renderApp("/games/missing?from=%2F", { games: playedGames(3) });
  await screen.findByRole("heading", { name: "Game not found" });
  fireEvent.click(await back("Back to Home"));
  await waitFor(() => expect(app.router.state.location.pathname).toBe("/"));
});
