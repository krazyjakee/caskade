import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { type CatalogFixtureOptions, createCatalogFixtureApi } from "../../api/catalogFixtures";
import { CatalogProvider } from "../../api/igdb";
import { type VideoApi, VideoApiProvider } from "../../api/videos";
import { renderApp } from "../../test/harness";

beforeEach(() => {
  // The fixture catalog's dates are relative to this day.
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date("2026-10-30T12:00:00Z"));
  // Shelves size their cards from their width.
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(900);
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
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function show(path: string, options: CatalogFixtureOptions = {}, mode?: "console") {
  const catalog = createCatalogFixtureApi(options);
  const videos = { play: vi.fn<VideoApi["play"]>(async () => {}) };
  const app = renderApp(path, {
    mode,
    wrap: (inner) => (
      <CatalogProvider api={catalog}>
        <VideoApiProvider api={videos}>{inner}</VideoApiProvider>
      </CatalogProvider>
    ),
  });
  return { ...app, catalog, videos };
}

const results = () =>
  Array.from(
    document.querySelectorAll('[data-catalog="results"] [data-cs="game-card"] [data-part="title"]'),
  ).map((title) => title.textContent);

const card = (title: string) =>
  Array.from(document.querySelectorAll<HTMLElement>('[data-cs="game-card"]')).find(
    (element) => element.querySelector('[data-part="title"]')?.textContent === title,
  );

it("browses the past year's most-rated games beside the release shelves, a page at a time", async () => {
  const app = show("/store");
  await waitFor(() => expect(results()).toHaveLength(24));
  // Lantern Tide (2024) is rated higher but came out more than a year ago.
  expect(results()[0]).toBe("Copper Orchard");
  expect(results()).not.toContain("Lantern Tide");
  expect(app.catalog.searches[0]).toMatchObject({ releasedFrom: "2025-10-30", releasedTo: null });
  // No count is claimed: the catalog has no total.
  expect(screen.getByRole("heading", { name: "Most rated this past year" })).toBeTruthy();
  await screen.findByRole("heading", { name: "Recently released" });
  await screen.findByRole("heading", { name: "Upcoming" });

  fireEvent.click(screen.getByRole("button", { name: "Load more" }));
  await waitFor(() => expect(results()).toHaveLength(41));
  expect(screen.queryByRole("button", { name: "Load more" })).toBeNull();
  expect(app.catalog.calls).toContain("search::24");
});

it("searches once typing stops, only for the last text, and keeps it in the URL", async () => {
  const app = show("/store");
  await waitFor(() => expect(results()).toHaveLength(24));
  const field = screen.getByRole("searchbox", { name: "Search the catalog" });
  fireEvent.change(field, { target: { value: "lan" } });
  fireEvent.change(field, { target: { value: "copper" } });
  await waitFor(() => expect(results()).toEqual(["Copper Orchard"]));
  expect(app.router.state.location.search).toMatchObject({ tab: "browse", q: "copper" });
  expect(app.catalog.calls.filter((call) => call.startsWith("search:"))).toEqual([
    "search::0",
    "search:copper:0",
  ]);
  // Narrowed, the shelves give way to the results.
  expect(screen.queryByRole("heading", { name: "Recently released" })).toBeNull();
});

it("says when nothing matches, when the server can't be reached and when results are saved", async () => {
  show("/store?q=nothing-like-it");
  await screen.findByText("No games match “nothing-like-it”.");
  cleanup();

  show("/store?q=lantern", { offline: true });
  const alert = await screen.findByText(/Could not reach the game catalog/);
  expect(alert.textContent).toContain("can't reach the metadata server");
  expect(screen.getByRole("button", { name: "Try again" })).toBeTruthy();
  cleanup();

  show("/store?q=lantern", { stale: true });
  await screen.findByText(/Showing saved results/);
  expect(results()).toEqual(["Lantern Tide"]);
});

it("marks the library's games, installed or not, and only those", async () => {
  show("/store", {
    owned: [
      { igdbId: "1001", gameId: "lantern", installed: true },
      { igdbId: "1002", gameId: "copper", installed: false },
    ],
  });
  await waitFor(() => expect(results()).toContain("Copper Orchard"));
  await waitFor(() => expect(card("Starfall Ledger")).toBeTruthy());
  await waitFor(() => expect(card("Lantern Tide")?.dataset.status).toBe("installed"));
  expect(card("Lantern Tide")?.textContent).toContain("Installed");
  expect(card("Copper Orchard")?.dataset.status).toBe("not-installed");
  expect(card("Copper Orchard")?.textContent).toContain("In library");
  const other = card("Starfall Ledger");
  expect(other?.dataset.status).toBeUndefined();
  expect(other?.querySelector('[data-part="owned"]')).toBeNull();
});

it("lists upcoming releases with only as much of the date as is known, and TBD apart", async () => {
  const app = show("/store?tab=upcoming");
  await waitFor(() => expect(results()).toHaveLength(3));
  expect(card("Lantern Tide")?.textContent).toContain("Q1 2027");
  // The release that listed it: Switch 2, not the game's PC release.
  expect(card("Lantern Tide")?.textContent).toContain("Switch 2");
  expect(card("Starfall Ledger")?.textContent).toContain("Jun 2027");
  expect(card("Ashen Relay")?.textContent).toContain("2026 · Cancelled");

  fireEvent.click(screen.getByRole("radio", { name: "TBD" }));
  await waitFor(() => expect(results()).toEqual(["Quiet Harbour"]));
  expect(app.router.state.location.search).toMatchObject({ tab: "upcoming", tbd: true });
  expect(card("Quiet Harbour")?.textContent).toContain("TBD");
});

it("opens a catalog game without adding it to the library, and Back returns to the results", async () => {
  const app = show("/store?q=lantern", {
    owned: [{ igdbId: "1001", gameId: "lantern", installed: true }],
  });
  const library = () =>
    app.library.listGames({ filter: "all", sort: "title", offset: 0, limit: 1 });
  const before = (await library()).total;
  await waitFor(() => expect(results()).toEqual(["Lantern Tide"]));
  fireEvent.click(within(card("Lantern Tide") as HTMLElement).getByRole("link"));

  await screen.findByRole("heading", { name: "Lantern Tide", level: 1 });
  expect(app.router.state.location.pathname).toBe("/store/games/1001");
  // Where to buy: each storefront, opened outside the app.
  const steam = screen.getByRole("link", { name: /Steam/ });
  expect(steam.getAttribute("href")).toBe("https://store.steampowered.com/app/1001");
  expect(steam.getAttribute("target")).toBe("_blank");
  expect(screen.getByRole("link", { name: /PlayStation Store/ })).toBeTruthy();
  // Its releases per platform, and its rating.
  expect(screen.getByRole("cell", { name: "Q1 2027" })).toBeTruthy();
  expect(screen.getByText("PEGI 16").getAttribute("title")).toBe("PEGI 16 · ESRB T");
  // In the library already.
  expect(screen.getByRole("link", { name: "Open in library" }).getAttribute("href")).toBe(
    "/games/lantern",
  );
  // Similar games, without itself or the one IGDB sent nothing for.
  await waitFor(() => expect(card("Copper Orchard")).toBeTruthy());
  expect(card("Starfall Ledger")).toBeTruthy();
  expect(document.querySelectorAll('[data-cs="shelf-row"] [data-cs="game-card"]')).toHaveLength(2);
  // Nothing was added to the library.
  expect((await library()).total).toBe(before);

  // A similar game goes back to this one, and this one to the results.
  fireEvent.click(within(card("Copper Orchard") as HTMLElement).getByRole("link"));
  await screen.findByRole("heading", { name: "Copper Orchard", level: 1 });
  await screen.findByText("IGDB lists no similar games.");
  fireEvent.click(await screen.findByRole("link", { name: /Back to Lantern Tide/ }));
  await screen.findByRole("heading", { name: "Lantern Tide", level: 1 });
  fireEvent.click(screen.getByRole("link", { name: /Back to Store/ }));
  await waitFor(() => expect(app.router.state.location.pathname).toBe("/store"));
  expect(app.router.state.location.search).toMatchObject({ q: "lantern" });
});

it("shows a game's screenshots in the gallery and plays its videos in the video window", async () => {
  const app = show("/store/games/1001");
  await screen.findByRole("heading", { name: "Lantern Tide", level: 1 });
  const shot = screen.getByRole<HTMLImageElement>("img", { name: "Lantern Tide, screenshot 2" });
  expect(shot.src).toBe("https://images.igdb.com/igdb/image/upload/t_screenshot_med/sc1001b.jpg");
  fireEvent.click(shot);
  const gallery = await screen.findByRole("dialog", { name: "Lantern Tide, screenshots" });
  expect(
    within(gallery).getByRole<HTMLImageElement>("img", { name: /screenshot 2 of 2/ }).src,
  ).toBe("https://images.igdb.com/igdb/image/upload/t_screenshot_huge/sc1001b.jpg");
  fireEvent.click(within(gallery).getByRole("button", { name: "Close" }));

  const videos = screen.getByRole("region", { name: "Videos" });
  fireEvent.click(within(videos).getByRole("button", { name: "Play Launch Trailer" }));
  await waitFor(() =>
    expect(app.videos.play).toHaveBeenCalledWith("lanternTr01", "Lantern Tide · Launch Trailer"),
  );
  // IGDB gives the second no name.
  fireEvent.click(within(videos).getByRole("button", { name: "Play Video 2" }));
  await waitFor(() =>
    expect(app.videos.play).toHaveBeenLastCalledWith("lanternGp02", "Lantern Tide · Video 2"),
  );
});

it("leaves out videos when IGDB has none", async () => {
  show("/store/games/1004");
  await screen.findByRole("heading", { name: "Quiet Harbour", level: 1 });
  expect(screen.queryByRole("region", { name: "Videos" })).toBeNull();
  expect(screen.getByText("No screenshots")).toBeTruthy();
});

it("says so when a game has no store links, and when IGDB has no such game", async () => {
  show("/store/games/1004");
  await screen.findByRole("heading", { name: "Quiet Harbour", level: 1 });
  expect(screen.getByText("IGDB lists no stores for this game.")).toBeTruthy();
  expect(screen.getByRole("link", { name: /Back to Store/ })).toBeTruthy();
  cleanup();

  show("/store/games/9999");
  await screen.findByText("IGDB has no game 9999.");
});

it("switches the Store's tabs with LB/RB in console mode", async () => {
  const app = show("/store", {}, "console");
  await waitFor(() => expect(results()).toHaveLength(24));
  app.pad.press("tab.next");
  await waitFor(() => expect(app.router.state.location.search).toMatchObject({ tab: "recent" }));
  await waitFor(() => expect(results()).toEqual(["Copper Orchard"]));
  app.pad.press("tab.prev");
  await waitFor(() => expect(app.router.state.location.search).toMatchObject({ tab: "browse" }));
});

it.each([undefined, "console"] as const)(
  "keeps popular browse filters and paging in the URL (%s)",
  async (mode) => {
    const app = show("/store?sort=popular", { popularity: { "1001": 0.2, "1002": 0.8 } }, mode);
    await waitFor(() => expect(results()).toHaveLength(24));
    expect(results().slice(0, 2)).toEqual(["Copper Orchard", "Lantern Tide"]);
    expect(app.catalog.searches[0]).toMatchObject({
      sort: "popular",
      offset: 0,
      releasedFrom: null,
    });
    fireEvent.click(screen.getByRole("button", { name: "Load more" }));
    await waitFor(() =>
      expect(
        app.catalog.searches.some((search) => search.offset === 24 && search.sort === "popular"),
      ).toBe(true),
    );
    await app.router.navigate({
      to: "/store",
      search: {
        tab: "browse",
        sort: "popular",
        q: "lantern",
        platform: "6",
        genre: "12",
        year: 2024,
      },
    });
    await waitFor(() =>
      expect(app.catalog.searches.at(-1)).toMatchObject({
        sort: "popular",
        offset: 0,
        query: "lantern",
        platforms: ["6"],
        genres: ["12"],
        releasedFrom: "2024-01-01",
        releasedTo: "2024-12-31",
      }),
    );
    expect(app.router.state.location.search).toMatchObject({
      sort: "popular",
      q: "lantern",
      platform: "6",
      genre: "12",
      year: 2024,
    });
  },
);

it("says quietly when the catalog can't sort by popularity yet, and still lists games (#253)", async () => {
  const app = show("/store?sort=popular", { sortsUnavailable: true });
  await waitFor(() => expect(results()).toHaveLength(24));
  const note = await screen.findByText(
    "The game catalog can't sort this way yet, so these are in its usual order.",
  );
  expect(note.getAttribute("role")).toBe("status");
  expect(screen.queryByRole("alert")).toBeNull();
  // The choice stays, for when the server has it.
  expect(app.router.state.location.search).toMatchObject({ sort: "popular" });
});
