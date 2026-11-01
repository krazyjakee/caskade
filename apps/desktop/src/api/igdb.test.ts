import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, expect, it, vi } from "vitest";
import { nativeCatalogApi, nativeGameIgdbApi, nativeIgdbApi } from "./igdb";

afterEach(() => {
  clearMocks();
  vi.unstubAllGlobals();
});

const lookup = { games: [], notFound: ["999999999"], pending: [] };

it("passes string ids to the core and unwraps its answers", async () => {
  vi.stubGlobal("isTauri", true);
  const invoke = vi.fn((_command: string, _args?: unknown) => lookup);
  mockIPC(invoke);
  expect(await nativeIgdbApi.games(["1942", "999999999"])).toEqual(lookup);
  await nativeIgdbApi.refresh(["1942"]);
  await nativeGameIgdbApi.search("witcher");
  expect(invoke.mock.calls.map(([command, args]) => [command, args])).toEqual([
    ["igdb_games", { ids: ["1942", "999999999"] }],
    ["igdb_refresh", { ids: ["1942"] }],
    ["igdb_search", { query: "witcher", limit: 10 }],
  ]);
});

it("surfaces core errors and refuses outside the desktop app", async () => {
  vi.stubGlobal("isTauri", true);
  mockIPC(() => {
    throw "metadata server unreachable";
  });
  await expect(nativeIgdbApi.games(["1"])).rejects.toThrow("metadata server unreachable");
  vi.stubGlobal("isTauri", false);
  await expect(nativeIgdbApi.games(["1"])).rejects.toThrow("Open Caskade desktop");
});

it("passes catalog requests to the core and unwraps its pages", async () => {
  vi.stubGlobal("isTauri", true);
  const page = { games: [], releases: [], nextOffset: null, stale: false };
  const invoke = vi.fn((_command: string, _args?: unknown) => page);
  mockIPC(invoke);
  expect(await nativeCatalogApi.search({ query: "zelda", offset: 24, platforms: ["6"] })).toEqual(
    page,
  );
  await nativeCatalogApi.upcoming({ tbd: true });
  await nativeCatalogApi.owned(["1001"]);
  expect(invoke.mock.calls.map(([command, args]) => [command, args])).toEqual([
    ["igdb_catalog_search", { search: { query: "zelda", offset: 24, platforms: ["6"] } }],
    ["igdb_catalog_upcoming", { releases: { tbd: true } }],
    ["igdb_owned", { ids: ["1001"] }],
  ]);
});
