import { i18n } from "@caskade/i18n";
import { expect, it } from "vitest";
import { catalogGame, release } from "../../api/catalogFixtures";
import type { CatalogPage } from "../../api/igdb";
import { formatReleaseDate } from "../../api/presentation";
import {
  listedGames,
  platformsLine,
  releaseKind,
  releaseWhen,
  releaseWhere,
  sortedReleases,
} from "./releases";

it("shows only as much of a date as IGDB knows", () => {
  // The UTC day as filed, never shifted into the local zone.
  expect(releaseWhen(release("2024-03-01"))).toBe(formatReleaseDate("2024-03-01"));
  expect(releaseWhen(release("2024-03-01"))).toMatch(/\b1\b/);
  expect(releaseWhen(release("2027-06-01", { precision: "month", month: 6, year: 2027 }))).toBe(
    "Jun 2027",
  );
  expect(releaseWhen(release("2027-01-01", { precision: "quarter", quarter: 1 }))).toBe("Q1 2027");
  expect(releaseWhen(release("2026-12-31", { precision: "year" }))).toBe("2026");
  expect(releaseWhen(release(null))).toBe("TBD");
  // Missing parts fall back to IGDB's own text.
  expect(releaseWhen(release(null, { precision: "quarter", year: null, human: "Q3 2026" }))).toBe(
    "Q3 2026",
  );
});

it("names days and months in the app's language", async () => {
  await i18n.loadAndActivate({ locale: "sv", messages: {} });
  expect(releaseWhen(release("2024-03-01"))).toBe("1 mars 2024");
  expect(releaseWhen(release("2027-06-01", { precision: "month", month: 6, year: 2027 }))).toBe(
    "juni 2027",
  );
});

it("tells cancelled and early-access releases apart from the rest", () => {
  expect(releaseKind(release("2026-12-01", { status: "Cancelled" }))).toBe("cancelled");
  expect(releaseKind(release("2026-12-01", { status: "Early Access" }))).toBe("early-access");
  expect(releaseKind(release("2026-12-01", { status: "Full Release" }))).toBe("released");
  expect(releaseKind(release("2026-12-01"))).toBeNull();
});

it("names a release's platform and region, and a game's platforms", () => {
  const ps5 = { id: "167", name: "PlayStation 5", abbreviation: "PS5" };
  expect(releaseWhere(release("2026-12-01", { platform: ps5, region: "north_america" }))).toBe(
    "PS5 · North America",
  );
  expect(releaseWhere(release("2026-12-01", { platform: null, region: "worldwide" }))).toBe("");
  const many = catalogGame("1", "Many", {
    platforms: ["A", "B", "C", "D", "E"].map((name) => ({ id: name, name, abbreviation: null })),
  });
  expect(platformsLine(many)).toBe("A · B · C +2");
});

it("keeps each game once across pages, with the release that listed it first", () => {
  const a = catalogGame("1", "A");
  const b = catalogGame("2", "B");
  const page = (games: (typeof a)[], releases: CatalogPage["releases"]): CatalogPage => ({
    games,
    releases,
    nextOffset: null,
    stale: false,
    sortUnavailable: false,
  });
  const pc = release("2026-11-01");
  const ps5 = release("2026-11-02", {
    platform: { id: "167", name: "PlayStation 5", abbreviation: "PS5" },
  });
  const listed = listedGames([
    page([a], [{ game: "1", release: pc }]),
    page(
      [a, b],
      [
        { game: "1", release: ps5 },
        { game: "2", release: ps5 },
      ],
    ),
  ]);
  expect(listed.map(({ game, release }) => [game.id, release?.date])).toEqual([
    ["1", "2026-11-01"],
    ["2", "2026-11-02"],
  ]);
});

it("lists a game's dated releases first, by date", () => {
  const game = catalogGame("1", "A", {
    releaseDates: [release(null), release("2027-01-01"), release("2024-01-01")],
  });
  expect(sortedReleases(game).map((r) => r.date)).toEqual(["2024-01-01", "2027-01-01", null]);
});
