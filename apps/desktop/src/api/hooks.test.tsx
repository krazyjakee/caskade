import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createFixtureApi, createFixtureGames } from "./fixtures";
import {
  useGame,
  useGames,
  useLaunchGame,
  useLibraryStats,
  useSearchGames,
  useSessions,
} from "./hooks";
import { createLibraryQueryClient, LibraryProvider } from "./provider";
import type { LibraryApi, LibraryEvent } from "./types";

afterEach(cleanup);
function setup(api: LibraryApi = createFixtureApi()) {
  const client = createLibraryQueryClient();
  client.setDefaultOptions({ queries: { retry: false, gcTime: 0 } });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <LibraryProvider api={api} client={client}>
      {children}
    </LibraryProvider>
  );
  return { api, client, wrapper };
}
describe("library queries", () => {
  it("paginates 1,203 summaries without losing games, then keys filter/sort/search separately", async () => {
    const api = createFixtureApi(createFixtureGames(1203));
    const list = vi.spyOn(api, "listGames");
    const { wrapper } = setup(api);
    const result = renderHook(({ query }) => useGames(query), {
      wrapper,
      initialProps: { query: { filter: "all", sort: "title" } as Parameters<typeof useGames>[0] },
    });
    await waitFor(() => expect(result.result.current.isSuccess).toBe(true));
    while (result.result.current.hasNextPage)
      await act(async () => {
        await result.result.current.fetchNextPage();
      });
    const items = result.result.current.data?.pages.flatMap((p) => p.items) ?? [];
    expect(items).toHaveLength(1203);
    expect(new Set(items.map((g) => g.id)).size).toBe(1203);
    expect(list).toHaveBeenCalledTimes(13);
    result.rerender({ query: { filter: "favorites", sort: "playtime", query: "  0004  " } });
    await waitFor(() =>
      expect(result.result.current.data?.pages[0]?.items[0]?.id).toBe("fixture-3"),
    );
    expect(list).toHaveBeenLastCalledWith({
      filter: "favorites",
      sort: "playtime",
      query: "0004",
      offset: 0,
      limit: 100,
    });
  });
  it("uses bounded backend search and does not query for empty suggestions", async () => {
    const api = createFixtureApi();
    const list = vi.spyOn(api, "listGames");
    const { wrapper } = setup(api);
    const result = renderHook(({ query }) => useSearchGames(query), {
      wrapper,
      initialProps: { query: "" },
    });
    expect(list).not.toHaveBeenCalled();
    result.rerender({ query: "0002" });
    await waitFor(() => expect(result.result.current.data?.total).toBe(1));
    expect(list).toHaveBeenLastCalledWith({
      query: "0002",
      filter: "all",
      sort: "title",
      offset: 0,
      limit: 6,
    });
  });
  it("invalidates active list/stat/detail/session queries after a library event", async () => {
    const api = createFixtureApi();
    const get = vi.spyOn(api, "getGame");
    const list = vi.spyOn(api, "listGames");
    const stats = vi.spyOn(api, "libraryStats");
    const sessions = vi.spyOn(api, "listSessions");
    const { wrapper } = setup(api);
    const result = renderHook(
      () => ({
        game: useGame("fixture-1"),
        games: useGames({ filter: "all", sort: "title" }),
        stats: useLibraryStats(),
        sessions: useSessions(),
      }),
      { wrapper },
    );
    await waitFor(() =>
      expect(
        result.result.current.stats.isSuccess &&
          result.result.current.sessions.isSuccess &&
          result.result.current.game.isSuccess &&
          result.result.current.games.isSuccess,
      ).toBe(true),
    );
    act(() => api.emit({ type: "library.changed" }));
    await waitFor(() =>
      expect([get, list, stats, sessions].every((fn) => fn.mock.calls.length === 2)).toBe(true),
    );
  });
  it("observes launch and exit without reload, including recent order and persisted playtime", async () => {
    const api = createFixtureApi();
    const { wrapper } = setup(api);
    const result = renderHook(
      () => ({
        launch: useLaunchGame(),
        game: useGame("fixture-1"),
        recent: useGames({ filter: "recent", sort: "recent" }),
        sessions: useSessions("fixture-1"),
        stats: useLibraryStats(),
      }),
      { wrapper },
    );
    await waitFor(() =>
      expect(result.result.current.game.isSuccess && result.result.current.recent.isSuccess).toBe(
        true,
      ),
    );
    expect(result.result.current.recent.data?.pages[0]?.total).toBe(0);
    let sessionId = "";
    await act(async () => {
      const launched = await result.result.current.launch.mutateAsync({ entryId: "entry-1" });
      if (launched.type !== "started") throw new Error("launch asked unexpectedly");
      sessionId = launched.ticket.sessionId;
    });
    await waitFor(() =>
      expect(result.result.current.recent.data?.pages[0]?.items[0]?.id).toBe("fixture-1"),
    );
    await waitFor(() => expect(result.result.current.sessions.data?.[0]?.endedAt).toBeNull());
    act(() => api.finishSession(sessionId, 3600));
    await waitFor(() => expect(result.result.current.game.data?.playtimeSeconds).toBe(4800));
    await waitFor(() =>
      expect(result.result.current.sessions.data?.[0]?.durationSeconds).toBe(3600),
    );
  });
  it("cleans up a listener that resolves after unmount", async () => {
    const api = createFixtureApi();
    const cleanupListener = vi.fn();
    let resolve: ((cleanup: () => void) => void) | undefined;
    vi.spyOn(api, "subscribe").mockImplementation(
      (_listener: (event: LibraryEvent) => void) =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    const { wrapper } = setup(api);
    const result = renderHook(() => useGame("fixture-1"), { wrapper });
    result.unmount();
    await act(async () => {
      resolve?.(cleanupListener);
    });
    expect(cleanupListener).toHaveBeenCalledOnce();
  });
  it("reports backend errors without silently serving fixture games", async () => {
    const api = createFixtureApi();
    vi.spyOn(api, "listGames").mockRejectedValue(new Error("Database is unavailable"));
    const { wrapper } = setup(api);
    const result = renderHook(() => useGames({ filter: "all", sort: "title" }), { wrapper });
    await waitFor(() => expect(result.result.current.isError).toBe(true));
    expect(result.result.current.data).toBeUndefined();
    expect(result.result.current.error?.message).toBe("Database is unavailable");
  });
});
