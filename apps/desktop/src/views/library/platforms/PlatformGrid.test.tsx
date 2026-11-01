import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it } from "vitest";
import { createFixtureGames } from "../../../api/fixtures";
import type { GameDetails } from "../../../api/types";
import { fakeViewport, renderApp } from "../../../test/harness";
import { presentPlatform, sortPlatforms } from "./catalog";

beforeEach(() => {
  localStorage.clear();
  fakeViewport();
});
afterEach(cleanup);

/** Emulated games: `[title, platform, playtime hours, last played day]`. */
function emulated(rows: [string, string, number, number | null][]): GameDetails[] {
  const [template] = createFixtureGames(1);
  if (!template) throw new Error("no fixture");
  return rows.map(([title, platform, hours, day], index) => ({
    ...template,
    id: `emu-${index}`,
    title,
    platform,
    status: "emulated",
    playtimeSeconds: hours * 3600,
    lastPlayedAt: day === null ? null : new Date(Date.UTC(2026, 9, day)).toISOString(),
    entries: template.entries.map((entry) => ({ ...entry, id: `emu-entry-${index}` })),
  }));
}

const games = emulated([
  ["Chrono Quest", "Super Nintendo Entertainment System", 2, 3],
  ["Mario Kart Thing", "Nintendo 64", 10, 1],
  ["Star Pilot", "Nintendo 64", 1, null],
  ["Ridge Racer-ish", "Sony PlayStation", 0, null],
]);
/** The titles of the game cards on screen. */
const titles = () =>
  Array.from(
    document.querySelectorAll('[data-cs="game-card"] [data-part="title"]'),
    (title) => title.textContent,
  );
const cards = () =>
  Array.from(
    document.querySelectorAll<HTMLElement>('[data-cs="platform-card"]'),
    (card) => card.querySelector('[data-part="title"]')?.textContent,
  );

it("shows the emulated games' platforms in the library's sort", async () => {
  renderApp("/library?filter=emulated&sort=recent", { games });
  await waitFor(() => expect(cards()).toEqual(["SNES", "Nintendo 64", "PlayStation"]));
  const n64 = screen.getByRole("link", { name: "Nintendo 64, 2 games" });
  within(n64).getByText("Nintendo · 64-bit console");
  within(n64).getByText("11.0h");
});

it("orders the platforms by playtime and by title", async () => {
  const app = renderApp("/library?filter=emulated&sort=playtime", { games });
  await waitFor(() => expect(cards()).toEqual(["Nintendo 64", "SNES", "PlayStation"]));
  await app.router.navigate({ to: "/library", search: { filter: "emulated", sort: "title" } });
  await waitFor(() => expect(cards()).toEqual(["Nintendo 64", "PlayStation", "SNES"]));
});

it("opens a platform's games, and All platforms goes back to them", async () => {
  const app = renderApp("/library?filter=emulated&sort=title", { games });
  fireEvent.click(await screen.findByRole("link", { name: "Nintendo 64, 2 games" }));
  await waitFor(() => expect(titles()).toEqual(["Mario Kart Thing", "Star Pilot"]));
  expect(app.router.state.location.search).toMatchObject({ platform: "Nintendo 64" });
  fireEvent.click(screen.getByRole("link", { name: "All platforms" }));
  await waitFor(() => expect(cards()).toHaveLength(3));
});

it("goes straight to the games with one platform, or with a search", async () => {
  const one = games.filter((game) => game.platform === "Nintendo 64");
  renderApp("/library?filter=emulated", { games: one });
  await waitFor(() => expect(titles()).toEqual(["Mario Kart Thing", "Star Pilot"]));
  expect(cards()).toEqual([]);
  cleanup();
  renderApp("/library?filter=emulated&q=chrono", { games });
  await waitFor(() => expect(titles()).toEqual(["Chrono Quest"]));
  expect(cards()).toEqual([]);
});

it("knows the catalogue's platforms by any of their names, and draws others generically", () => {
  for (const name of ["Sega Genesis", "Mega Drive", "sega_genesis", "MD"])
    expect(presentPlatform(name).title).toBe("Genesis");
  expect(presentPlatform("Sega Genesis").art).toMatch(/sega_genesis/);
  expect(presentPlatform("PS4")).toMatchObject({ title: "PlayStation 4", known: true });
  expect(presentPlatform("PS4").art).toMatch(/sony_playstation4/);
  const odd = presentPlatform("Tiger Game.com");
  expect(odd).toMatchObject({ title: "Tiger Game.com", subtitle: "", known: false });
  expect(odd.art).toMatch(/generic/);
});

it("keeps never-played platforms in title order after the played ones", () => {
  const platform = (name: string, lastPlayedAt: string | null, playtimeSeconds = 0) => ({
    name,
    games: 1,
    playtimeSeconds,
    lastPlayedAt,
  });
  const sorted = sortPlatforms(
    [
      platform("Sony PlayStation", null),
      platform("Nintendo 64", "2026-10-01T00:00:00Z"),
      platform("Arcade", null),
      platform("Sega Saturn", "2026-10-05T00:00:00Z"),
    ],
    "recent",
  );
  expect(sorted.map((p) => p.name)).toEqual([
    "Sega Saturn",
    "Nintendo 64",
    "Arcade",
    "Sony PlayStation",
  ]);
});
