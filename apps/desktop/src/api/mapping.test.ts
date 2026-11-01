import { expect, it } from "vitest";
import {
  collectionKindRecord,
  mapCollection,
  mapCollectionKind,
  mapDetails,
  mapSession,
  mapSummary,
} from "./mapping";

it("converts real DTO milliseconds and exact thumbnail projections without storing absolute managed paths", () => {
  const hash = "a".repeat(64);
  const art = {
    kind: "cover",
    thumbnails: [96, 320, 960].map((size) => ({ size, path: `${hash}/thumb-${size}.webp` })),
  };
  const game = {
    id: "opaque-id",
    title: "Original Fixture",
    genres: ["Strategy"],
    platforms: ["PC"],
    favorite: true,
    tags: ["Original"],
    description: "Description",
    releaseDate: "2011-04-18",
  };
  const screenshot = { ...art, kind: "screenshot", cachedPath: `${hash}/original.webp` };
  const details = mapDetails(
    {
      game,
      artwork: [art, screenshot],
      playtimeMs: 3_600_000,
      lastPlayedAtMs: 1_700_000_000_000,
      entries: [
        {
          id: "entry",
          sourceId: "caskade",
          state: { type: "installed" },
          size: 100,
          location: { type: "managed", rootId: "stable-root", relDir: "Original" },
          launch: { type: "executable" },
        },
      ],
    },
    (path) => `caskade-asset://localhost/${path}`,
  );
  expect(details.playtimeSeconds).toBe(3600);
  expect(details.lastPlayedAt).toBe("2023-11-14T22:13:20.000Z");
  expect(details.coverGrid).toContain("/thumb-320.webp");
  expect(details.coverList).toContain("/thumb-96.webp");
  expect(details.hero).toContain("/thumb-960.webp");
  expect(details.releaseDate).toBe("2011-04-18");
  expect(details.screenshots).toEqual([
    {
      thumb: `caskade-asset://localhost/${hash}/thumb-320.webp`,
      full: `caskade-asset://localhost/${hash}/original.webp`,
    },
  ]);
  expect(details.entries[0]?.location).toBe("Storage root stable-root · Original");
  expect(details.entries[0]?.canLaunch).toBe(true);
  const summary = mapSummary(
    { ...game, installed: false, cover: null, playtimeMs: 0, lastPlayedAtMs: null },
    (path) => path,
  );
  expect(summary.coverGrid).toBeNull();
  expect(summary.lastPlayedAt).toBeNull();
  expect(
    mapSession({
      id: "s",
      gameId: "g",
      entryId: "e",
      startedAtMs: 1_700_000_000_000,
      endedAtMs: null,
      durationMs: 3000,
    }).durationSeconds,
  ).toBe(3);
});

it("maps a smart collection's query to the core and back without its page", () => {
  const kind = {
    type: "smart" as const,
    query: { filter: "installed" as const, sort: "playtime" as const, platform: "Nintendo 64" },
  };
  const record = collectionKindRecord(kind);
  expect(record).toEqual({
    type: "smart",
    query: {
      filter: "installed",
      sort: "playtime",
      query: null,
      platform: "Nintendo 64",
      offset: 0,
      limit: 50,
    },
  });
  expect(mapCollectionKind(record)).toEqual(kind);
  expect(mapCollectionKind({ type: "manual" })).toEqual({ type: "manual" });
});

it("maps a collection's summary, with no time for one saved before times were kept", () => {
  const summary = mapCollection(
    {
      id: "c",
      name: "Shelf",
      description: "",
      pinned: true,
      kind: { type: "manual" },
      updatedAtMs: 0,
      games: 2,
      playtimeMs: 90_000,
      covers: [
        { thumbnails: [{ size: 320, path: `${"b".repeat(64)}/thumb-320.webp` }] },
        { thumbnails: [] },
      ],
    },
    (path) => `asset://${path}`,
  );
  expect(summary).toMatchObject({
    updatedAt: null,
    playtimeSeconds: 90,
    covers: [`asset://${"b".repeat(64)}/thumb-320.webp`],
  });
});

it("gives a web game its page as its website, offline copy or not", () => {
  const game = {
    id: "g",
    title: "GemCraft",
    genres: [],
    platforms: ["browser"],
    favorite: false,
    tags: [],
    description: "",
  };
  const details = (launch: { type: string; url?: string }) =>
    mapDetails(
      {
        game,
        artwork: [],
        playtimeMs: 0,
        lastPlayedAtMs: null,
        entries: [
          {
            id: "web",
            sourceId: "web",
            state: { type: "installed" },
            size: null,
            location: { type: "managed", rootId: "root", relDir: "GemCraft" },
            launch,
          },
        ],
      },
      (path) => path,
    );
  const page = "https://armorgames.com/play/1716/gemcraft";
  expect(details({ type: "web", url: page }).website).toBe(page);
  expect(details({ type: "executable" }).website).toBeNull();
});
