import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { type GameIgdb, type GameIgdbApi, GameIgdbProvider, type IgdbGame } from "../../api/igdb";
import { coreSettingsDefaults, createSettingsFixture } from "../../test/settingsFixture";
import { SettingsProvider } from "../settings/useSettings";
import { IgdbPanel } from "./IgdbPanel";

afterEach(cleanup);

const portal: IgdbGame = {
  id: "72",
  slug: "portal-2",
  url: "https://www.igdb.com/games/portal-2",
  title: "Portal 2",
  description: "Science!",
  storyline: null,
  genres: ["Puzzle", "Platform"],
  tags: [],
  platforms: [{ id: "6", name: "PC (Microsoft Windows)", abbreviation: "PC" }],
  releaseDate: "2011-04-18",
  aliases: [],
  totalRating: 91.6,
  externalIds: [],
  gameType: 0,
  versionParent: null,
  images: { cover: null, hero: null, logo: null, artworks: [], screenshots: [] },
  ageRatings: [],
  releaseDates: [],
  storeLinks: [],
  websites: [],
  similarGames: [],
  status: null,
  videos: [],
};

const portal1: IgdbGame = {
  ...portal,
  id: "71",
  slug: "portal",
  url: "https://www.igdb.com/games/portal",
  title: "Portal",
  releaseDate: "2007-10-09",
  platforms: [
    { id: "12", name: "Xbox 360", abbreviation: "X360" },
    { id: null, name: "PC", abbreviation: null },
  ],
};

/** An API over one game's match: id 72 is Portal 2, nothing else exists.
 * Searches for "portal" find Portal 2 and Portal. With `mergedInto`, a
 * match by hand merges the game into that one. */
function fakeApi(start: GameIgdb | null, mergedInto?: string) {
  let match = structuredClone(start);
  const calls: string[] = [];
  const api: GameIgdbApi = {
    async game() {
      return structuredClone(match);
    },
    async set(_gameId, igdbId) {
      calls.push(`set:${igdbId}`);
      if (igdbId !== null && igdbId !== "72") throw new Error(`IGDB has no game ${igdbId}`);
      match = {
        gameId: igdbId === null ? "g1" : (mergedInto ?? "g1"),
        igdbId,
        source: "manual",
        pending: false,
        game: igdbId === null ? null : portal,
      };
      return structuredClone(match);
    },
    async rematch() {
      calls.push("rematch");
      match = { gameId: "g1", igdbId: "72", source: "store", pending: false, game: portal };
      return structuredClone(match);
    },
    async refresh() {
      calls.push("refresh");
      return structuredClone(match);
    },
    async search(query) {
      calls.push(`search:${query}`);
      return query.toLowerCase().includes("portal") ? [portal, portal1] : [];
    },
    async separate() {
      calls.push("separate");
      return "apart";
    },
    async ageRatings() {
      return null;
    },
  };
  return { api, calls };
}

function show(api: GameIgdbApi, title = "Portal 2", onMerged?: (gameId: string) => void) {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <GameIgdbProvider api={api}>
        <IgdbPanel gameId="g1" title={title} onMerged={onMerged} />
      </GameIgdbProvider>
    </QueryClientProvider>,
  );
}

it("shows a store match's IGDB facts and refreshes it", async () => {
  const { api, calls } = fakeApi({
    gameId: "g1",
    igdbId: "72",
    source: "store",
    pending: false,
    game: portal,
  });
  show(api);
  const link = await screen.findByRole("link", { name: "Portal 2" });
  expect(link.getAttribute("href")).toBe("https://www.igdb.com/games/portal-2");
  const facts = screen.getByText("IGDB rating").closest("dl") as HTMLElement;
  expect(within(facts).getByText("92 / 100")).toBeTruthy();
  expect(within(facts).getByText("Puzzle · Platform")).toBeTruthy();
  expect(within(facts).getByText("PC (Microsoft Windows)")).toBeTruthy();
  expect(screen.getByText(/Matched by store id · IGDB ID 72/)).toBeTruthy();
  // Matching automatically is already what happens.
  expect(screen.queryByRole("button", { name: "Match automatically" })).toBeNull();

  fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
  await screen.findByRole("button", { name: "Refresh" });
  expect(calls).toEqual(["refresh"]);
});

it("searches IGDB by the game's title and uses the chosen result", async () => {
  const { api, calls } = fakeApi({
    gameId: "g1",
    igdbId: null,
    source: "none",
    pending: false,
    game: null,
  });
  show(api, "Portal");
  await screen.findByText("Caskade found no IGDB game for it.");

  fireEvent.click(screen.getByRole("button", { name: "Set IGDB game" }));
  const input = screen.getByRole("textbox", { name: "Search IGDB" }) as HTMLInputElement;
  expect(input.value).toBe("Portal");
  const results = await screen.findByRole("list", { name: "IGDB games" });
  await within(results).findByRole("button", { name: "Use Portal 2" });
  expect(within(results).getByText("2007 · X360 · PC")).toBeTruthy();
  expect(within(results).getByRole("link", { name: "Portal" }).getAttribute("href")).toBe(
    "https://www.igdb.com/games/portal",
  );

  fireEvent.change(input, { target: { value: "half-life" } });
  await screen.findByText("No IGDB games match “half-life”.");

  fireEvent.change(input, { target: { value: "portal 2" } });
  fireEvent.click(await screen.findByRole("button", { name: "Use Portal 2" }));
  await screen.findByRole("link", { name: "Portal 2" });
  expect(screen.getByText(/Chosen by you · IGDB ID 72/)).toBeTruthy();
  expect(calls).toEqual(["search:Portal", "search:half-life", "search:portal 2", "set:72"]);
});

it("changes the match by id, refusing what IGDB lacks", async () => {
  const { api, calls } = fakeApi({
    gameId: "g1",
    igdbId: "72",
    source: "store",
    pending: false,
    game: portal,
  });
  show(api);
  fireEvent.click(await screen.findByRole("button", { name: "Change IGDB game" }));
  const input = screen.getByRole("textbox", { name: "Search IGDB" });
  // The current match is marked, not offered.
  const results = await screen.findByRole("list", { name: "IGDB games" });
  await within(results).findByText("Current match");
  expect(within(results).queryByRole("button", { name: "Use Portal 2" })).toBeNull();

  fireEvent.change(input, { target: { value: "5" } });
  fireEvent.click(screen.getByRole("button", { name: "Use IGDB ID 5" }));
  expect((await screen.findByRole("alert")).textContent).toBe("IGDB has no game 5");

  // Enter uses a typed id.
  fireEvent.change(input, { target: { value: " 71 " } });
  fireEvent.submit(input);
  expect((await screen.findByRole("alert")).textContent).toBe("IGDB has no game 71");
  expect(calls.filter((call) => call.startsWith("set:"))).toEqual(["set:5", "set:71"]);
});

it("follows a match that merges the game into another release", async () => {
  const { api } = fakeApi(
    { gameId: "g1", igdbId: null, source: "none", pending: false, game: null },
    "g2",
  );
  const merged: string[] = [];
  show(api, "Portal 2", (gameId) => merged.push(gameId));
  await screen.findByText("Caskade found no IGDB game for it.");
  fireEvent.click(screen.getByRole("button", { name: "Set IGDB game" }));
  fireEvent.click(await screen.findByRole("button", { name: "Use Portal 2" }));
  await waitFor(() => expect(merged).toEqual(["g2"]));
});

it("drops a match, then matches automatically again", async () => {
  const { api, calls } = fakeApi({
    gameId: "g1",
    igdbId: "72",
    source: "manual",
    pending: false,
    game: portal,
  });
  show(api);
  fireEvent.click(await screen.findByRole("button", { name: "Not on IGDB" }));
  await screen.findByText("You marked this game as not on IGDB.");

  fireEvent.click(screen.getByRole("button", { name: "Match automatically" }));
  await screen.findByText(/Matched by store id/);
  expect(calls).toEqual(["set:null", "rematch"]);
});

it("explains a game matching hasn't reached yet, and an id it can't describe offline", async () => {
  const unseen = fakeApi(null);
  show(unseen.api);
  await screen.findByText(/Not matched yet/);
  fireEvent.click(screen.getByRole("button", { name: "Match now" }));
  await screen.findByRole("link", { name: "Portal 2" });
  expect(unseen.calls).toEqual(["rematch"]);
  cleanup();

  show(fakeApi({ gameId: "g1", igdbId: "72", source: "romm", pending: true, game: null }).api);
  await screen.findByText(/IGDB game 72\. Its details show/);
  expect(
    screen.getByText(/Matched by your RomM server · IGDB ID 72 · not filled in yet/),
  ).toBeTruthy();
});

it("says lookups are off on this device instead of waiting for a match, and turns them on", async () => {
  const { api } = fakeApi(null);
  const settings = createSettingsFixture({
    ...coreSettingsDefaults(),
    "core.metadata": { online: false },
  });
  render(
    <QueryClientProvider client={new QueryClient()}>
      <SettingsProvider api={settings.api}>
        <GameIgdbProvider api={api}>
          <IgdbPanel gameId="g1" title="Portal 2" />
        </GameIgdbProvider>
      </SettingsProvider>
    </QueryClientProvider>,
  );
  await screen.findByText(/Online lookups are off on this device/);
  expect(screen.queryByText(/Caskade matches games with IGDB in the background/)).toBeNull();

  fireEvent.click(screen.getByRole("button", { name: "Turn on online lookups" }));
  await screen.findByText("Not matched yet. Caskade matches games with IGDB in the background.");
  expect(settings.values["core.metadata"]).toEqual({ online: true });
});
