import { clearMocks, mockConvertFileSrc, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, expect, it, vi } from "vitest";
import { events, type GameSummary_Serialize } from "./bindings";
import { createNativeApi } from "./native";

afterEach(() => {
  clearMocks();
  vi.unstubAllGlobals();
});
const hash = "a".repeat(64);
const game: GameSummary_Serialize = {
  adminPinNeeded: true,
  id: "game",
  title: "Original fixture",
  sortTitle: "Original fixture",
  description: "",
  genres: ["Strategy"],
  tags: [],
  releaseDate: "2024-02-29",
  platforms: ["PC"],
  favorite: false,
  hidden: false,
  rating: null,
  cover: {
    id: "art",
    gameId: "game",
    contentHash: hash,
    kind: "cover",
    source: "fixture",
    cachedPath: `${hash}/original.webp`,
    width: 1000,
    height: 1500,
    thumbnails: [96, 320, 960].map((size) => ({
      size,
      path: `${hash}/thumb-${size}.webp`,
      width: size,
      height: size,
    })),
  },
  icon: {
    id: "icon",
    gameId: "game",
    contentHash: "b".repeat(64),
    kind: "icon",
    source: "fixture",
    cachedPath: `${"b".repeat(64)}/original.webp`,
    width: 64,
    height: 64,
    thumbnails: [96, 320, 960].map((size) => ({
      size,
      path: `${"b".repeat(64)}/thumb-${size}.webp`,
      width: 64,
      height: 64,
    })),
  },
  installed: true,
  playtimeMs: 3_600_000,
  lastPlayedAtMs: null,
};
it.each(["linux", "windows", "macos"])(
  "uses generated commands and platform-correct thumbnail URLs on %s",
  async (os) => {
    vi.stubGlobal("isTauri", true);
    mockConvertFileSrc(os);
    const invoked = vi.fn((command: string) =>
      command === "list_games" ? { items: [game], total: 1 } : null,
    );
    mockIPC(invoked);
    const api = createNativeApi();
    const result = await api.listGames({ filter: "all", sort: "title", offset: 0, limit: 100 });
    expect(invoked).toHaveBeenCalledWith("list_games", {
      query: { filter: "all", sort: "title", query: null, platform: null, offset: 0, limit: 100 },
    });
    expect(result.items[0]?.playtimeSeconds).toBe(3600);
    expect(result.items[0]?.adminPinNeeded).toBe(true);
    expect(result.items[0]?.coverGrid).toBe(
      `${os === "windows" ? "http://caskade-asset.localhost" : "caskade-asset://localhost"}/${hash}%2Fthumb-320.webp`,
    );
    expect(result.items[0]?.coverList).toContain("thumb-96.webp");
    expect(result.items[0]?.icon).toContain(`${"b".repeat(64)}`);
    expect(result.items[0]?.icon).toContain("thumb-96.webp");
    expect(JSON.stringify(result)).not.toContain("original.webp");
    expect(await api.getGame("absent")).toBeNull();
  },
);
it("normalizes generated library/start/exit events and unlistens every subscription", async () => {
  vi.stubGlobal("isTauri", true);
  mockIPC(() => null, { shouldMockEvents: true });
  const listener = vi.fn();
  const cleanup = await createNativeApi().subscribe(listener);
  const session = {
    gameId: "g",
    entryId: "e",
    sessionId: "s",
    startedAtMs: 100,
    endedAtMs: null,
    durationMs: 0,
  };
  await events.gameStarted.emit(session);
  await events.gameExited.emit({ ...session, endedAtMs: 1000, durationMs: 900 });
  await events.libraryChanged.emit({ change: "updated", gameId: "g" });
  // Switching accounts refreshes the library queries, collections included.
  await events.accountChanged.emit({ activeId: "other" });
  expect(listener.mock.calls.map(([event]) => event.type)).toEqual([
    "game.started",
    "game.exited",
    "library.changed",
    "account.changed",
  ]);
  cleanup();
  await events.gameStarted.emit(session);
  expect(listener).toHaveBeenCalledTimes(4);
});
it("cleans up other listeners when one native event registration fails", async () => {
  vi.stubGlobal("isTauri", true);
  mockIPC(() => null, { shouldMockEvents: true });
  vi.spyOn(events.gameStarted, "listen").mockRejectedValueOnce(new Error("registration failed"));
  const listener = vi.fn();
  await expect(createNativeApi().subscribe(listener)).rejects.toThrow("Could not subscribe");
  await events.libraryChanged.emit({ change: "updated", gameId: "g" });
  expect(listener).not.toHaveBeenCalled();
});
it("surfaces IPC errors and refuses native operations in an ordinary browser", async () => {
  vi.stubGlobal("isTauri", true);
  mockIPC(() => Promise.reject("Database unavailable"));
  await expect(createNativeApi().libraryStats()).rejects.toThrow("Database unavailable");
  vi.stubGlobal("isTauri", false);
  await expect(createNativeApi().getGame("game")).rejects.toThrow("Open Caskade desktop");
});
