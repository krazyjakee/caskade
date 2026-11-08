import { cleanup, screen, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { createFixtureGames } from "../../api/fixtures";
import { playedGames, renderApp } from "../../test/harness";

afterEach(cleanup);

const titles = async (shelf: string) => {
  const section = await screen.findByRole("region", { name: shelf });
  return within(section)
    .getAllByRole("link")
    .flatMap((link) => link.querySelector('[data-part="title"]')?.textContent ?? []);
};

it("shows the last added games, and the newest released ones without the unreleased", async () => {
  const games = createFixtureGames(4).map((game, index) => ({
    ...game,
    releaseDate: ["2001-03-21", null, "2099-01-01", "2019-06-27"][index] ?? null,
  }));
  const app = renderApp("/", { games });

  expect(await titles("Recently Added")).toEqual([
    "Fixture Game 0004",
    "Fixture Game 0003",
    "Fixture Game 0002",
    "Fixture Game 0001",
  ]);
  expect(await titles("Recently Released")).toEqual(["Fixture Game 0004", "Fixture Game 0001"]);

  const viewAll = within(screen.getByRole("region", { name: "Recently Released" })).getByRole(
    "link",
    { name: "View all →" },
  );
  expect(viewAll.getAttribute("href")).toBe("/library?filter=all&sort=released");
  expect(app.router.state.location.pathname).toBe("/");
});

it("leaves out the release shelf when no game has a release date", async () => {
  const games = createFixtureGames(3).map((game) => ({ ...game, releaseDate: null }));
  renderApp("/", { games });
  await screen.findByRole("region", { name: "Recently Added" });
  expect(screen.queryByRole("region", { name: "Recently Released" })).toBeNull();
});

it("leaves the hero's game out of Recently Played", async () => {
  renderApp("/", { games: playedGames(3) });
  const hero = await screen.findByRole("heading", { name: "Fixture Game 0003" });
  expect(hero.closest('[data-cs="hero"]')).toBeTruthy();
  expect(await titles("Recently Played")).toEqual(["Fixture Game 0002", "Fixture Game 0001"]);
});

it("leaves out Recently Played when the hero is the only game played", async () => {
  renderApp("/", { games: playedGames(1) });
  await screen.findByRole("heading", { name: "Fixture Game 0001" });
  await screen.findByRole("region", { name: "Recently Added" });
  expect(screen.queryByRole("region", { name: "Recently Played" })).toBeNull();
});

it("names the other device a game was last played on, and nothing for this one", async () => {
  const playedOn = [null, { device: "handheld", name: null }, { device: "den", name: "Den" }];
  const games = playedGames(3).map((game, index) => ({
    ...game,
    lastPlayedOn: playedOn[index] ?? null,
  }));
  renderApp("/", { games });
  const heading = await screen.findByRole("heading", { name: "Fixture Game 0003" });
  const hero = heading.closest('[data-cs="hero"]');
  expect(hero?.querySelector('[data-part="played-on"]')?.textContent).toBe("Last played on Den");

  const shelf = await screen.findByRole("region", { name: "Recently Played" });
  const card = (title: string) =>
    within(shelf)
      .getAllByRole("link")
      .find((link) => link.querySelector('[data-part="title"]')?.textContent === title);
  expect(card("Fixture Game 0002")?.querySelector('[data-part="played-on"]')?.textContent).toBe(
    "Last played on another device",
  );
  expect(card("Fixture Game 0001")?.querySelector('[data-part="played-on"]')).toBeNull();
});
