import {
  keepPreviousData,
  useInfiniteQuery,
  useMutation,
  useQueries,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { useCallback, useEffect, useRef, useState } from "react";
import { libraryKeys, useLibraryApi } from "./provider";
import type { GamePage, LaunchChoice, LibraryFilter, LibraryQuery } from "./types";

export const LIBRARY_PAGE_SIZE = 100;
export function normalizeLibraryQuery(query: LibraryQuery): LibraryQuery {
  return {
    filter: query.filter,
    sort: query.sort,
    query: query.query?.trim() || undefined,
    platform: query.platform || undefined,
  };
}
/** `pageSize` is for views that show a few games (Home's shelf), so they
 * don't load a whole Library page. */
export function useGames(input: LibraryQuery, { pageSize = LIBRARY_PAGE_SIZE } = {}) {
  const api = useLibraryApi();
  const query = normalizeLibraryQuery(input);
  return useInfiniteQuery({
    queryKey: [...libraryKeys.games, query, pageSize],
    initialPageParam: 0,
    queryFn: ({ pageParam }) => api.listGames({ ...query, offset: pageParam, limit: pageSize }),
    getNextPageParam: (last, pages) => {
      const count = pages.reduce((sum, page) => sum + page.items.length, 0);
      return last.items.length > 0 && count < last.total ? count : undefined;
    },
  });
}
/** The library's games for a scrolling view, fetched by the page around
 * the rows on screen (`setRange`): a jump anywhere in a large library loads
 * that part of it, not every page before it. */
export function useGameWindow(input: LibraryQuery) {
  const api = useLibraryApi();
  const query = normalizeLibraryQuery(input);
  return usePageWindow({
    key: [...libraryKeys.games, query],
    token: query,
    fetchPage: (offset, limit) => api.listGames({ ...query, offset, limit }),
    // A new search text keeps the last results on screen until a page of
    // its own arrives, so typing in the library doesn't blank the grid.
    keep: (shown) => sameButText(shown, query),
  });
}
/** Games fetched by the page around the rows on screen, under `key`. While
 * nothing has arrived for `token`, the pages shown for an earlier one stay
 * where `keep` says so. */
export function usePageWindow<T>({
  key,
  token,
  fetchPage,
  keep,
}: {
  key: readonly unknown[];
  token: T;
  fetchPage: (offset: number, limit: number) => Promise<GamePage>;
  keep?: (shown: T) => boolean;
}) {
  const size = LIBRARY_PAGE_SIZE;
  const [span, setSpan] = useState<readonly [number, number]>([0, 0]);
  const current = useRef(span);
  current.current = span;
  const timer = useRef(0);
  useEffect(() => () => window.clearTimeout(timer.current), []);
  const setRange = useCallback((start: number, end: number) => {
    // Half a page of margin, so scrolling on finds the next page loaded.
    const next = [
      Math.max(0, Math.floor((start - size / 2) / size)),
      Math.max(0, Math.floor((end + size / 2) / size)),
    ] as const;
    const [first, last] = current.current;
    if (next[0] === first && next[1] === last) return;
    window.clearTimeout(timer.current);
    // A jump (a dragged scrollbar) waits for the scrolling to settle, so
    // the pages it passes over aren't all fetched.
    if (next[0] <= last + 1 && next[1] >= first - 1) setSpan(next);
    else timer.current = window.setTimeout(() => setSpan(next), 120);
  }, []);

  const known = useRef<number | undefined>(undefined);
  const pageCount = known.current === undefined ? Infinity : Math.ceil(known.current / size);
  const last = Math.max(0, Math.min(span[1], pageCount - 1));
  const first = Math.min(span[0], last);
  const pageNumbers = Array.from({ length: last - first + 1 }, (_, i) => first + i);
  const results = useQueries({
    queries: pageNumbers.map((page) => ({
      queryKey: [...key, "page", page],
      queryFn: () => fetchPage(page * size, size),
    })),
  });

  const shown = useRef<{ token: T; pages: Map<number, GamePage> } | null>(null);
  let pages = new Map<number, GamePage>();
  results.forEach((result, i) => {
    const page = pageNumbers[i];
    if (result.data && page !== undefined) pages.set(page, result.data);
  });
  const previous = shown.current;
  if (pages.size > 0) shown.current = { token, pages };
  else if (previous && keep?.(previous.token)) pages = previous.pages;
  const total = pages.values().next().value?.total;
  known.current = total;
  const error = results.find((result) => result.isError)?.error ?? null;
  return {
    /** Undefined until the first page arrives. */
    total,
    /** The game at `index`, or undefined while its page loads. */
    game: (index: number) => pages.get(Math.floor(index / size))?.items[index % size],
    setRange,
    isPending: total === undefined && !error,
    isFetching: results.some((result) => result.isFetching),
    error,
    refetch: () => {
      for (const result of results) if (result.isError) void result.refetch();
    },
  };
}
function sameButText(a: LibraryQuery, b: LibraryQuery): boolean {
  return a.filter === b.filter && a.sort === b.sort && a.platform === b.platform;
}
export function useGame(id: string) {
  const api = useLibraryApi();
  return useQuery({ queryKey: libraryKeys.game(id), queryFn: () => api.getGame(id) });
}
/** Fetches the images only the game's page shows, once per visit (not as
 * a query: library events would ask again). A failure leaves the page
 * without them. True while they download. */
export function usePageImages(gameId: string) {
  const api = useLibraryApi();
  const [loading, setLoading] = useState<string | null>(null);
  useEffect(() => {
    if (!api.pageImages) return;
    let current = true;
    setLoading(gameId);
    void api
      .pageImages(gameId)
      .catch(() => {})
      .finally(() => {
        if (current) setLoading(null);
      });
    return () => {
      current = false;
    };
  }, [api, gameId]);
  return loading === gameId;
}
/** The hidden games, for Settings. */
export function useHiddenGames() {
  const api = useLibraryApi();
  return useQuery({
    queryKey: [...libraryKeys.all, "hidden"],
    queryFn: () => api.hiddenGames?.() ?? Promise.resolve([]),
    enabled: Boolean(api.hiddenGames),
  });
}
/** Hides a game, or shows it again; `null` where the library can't. */
export function useSetHidden() {
  const api = useLibraryApi();
  const client = useQueryClient();
  const mutation = useMutation({
    mutationFn: ({ gameId, hidden }: { gameId: string; hidden: boolean }) => {
      if (!api.setHidden) throw new Error("Hiding games isn't available here");
      return api.setHidden(gameId, hidden);
    },
    onSuccess: () => client.invalidateQueries({ queryKey: libraryKeys.all }),
  });
  return api.setHidden ? mutation : null;
}
/** Marks a game as a favourite, or not; `null` where the library can't. */
export function useSetFavorite() {
  const api = useLibraryApi();
  const client = useQueryClient();
  const mutation = useMutation({
    mutationFn: ({ gameId, favorite }: { gameId: string; favorite: boolean }) => {
      if (!api.setFavorite) throw new Error("Favourites aren't available here");
      return api.setFavorite(gameId, favorite);
    },
    onSuccess: () => client.invalidateQueries({ queryKey: libraryKeys.all }),
  });
  return api.setFavorite ? mutation : null;
}
/** Chooses the entry a game's Play starts; `null` where the library can't. */
export function useSetDefaultEntry() {
  const api = useLibraryApi();
  const client = useQueryClient();
  const mutation = useMutation({
    mutationFn: ({ gameId, entryId }: { gameId: string; entryId: string | null }) => {
      if (!api.setDefaultEntry) throw new Error("Choosing a release isn't available here");
      return api.setDefaultEntry(gameId, entryId);
    },
    onSuccess: (_, { gameId }) => client.invalidateQueries({ queryKey: libraryKeys.game(gameId) }),
  });
  return api.setDefaultEntry ? mutation : null;
}
/** Opens an entry's folder in the file manager; `null` where it can't. */
export function useRevealEntry() {
  const api = useLibraryApi();
  const mutation = useMutation({
    mutationFn: (entryId: string) => {
      if (!api.revealEntry) throw new Error("Opening folders isn't available here");
      return api.revealEntry(entryId);
    },
  });
  return api.revealEntry ? mutation : null;
}
/** Where each initial letter starts in `input`'s games sorted by title;
 * `enabled: false` skips asking. */
export function useTitleLetters(input: LibraryQuery, { enabled = true } = {}) {
  const api = useLibraryApi();
  const query = { ...normalizeLibraryQuery(input), sort: "title" as const };
  return useQuery({
    queryKey: [...libraryKeys.all, "letters", query],
    queryFn: () => api.titleLetters?.(query) ?? Promise.resolve([]),
    enabled,
  });
}
/** The platforms of the games in `filter`; `enabled: false` skips asking. */
export function useLibraryPlatforms(filter: LibraryFilter, { enabled = true } = {}) {
  const api = useLibraryApi();
  return useQuery({
    queryKey: [...libraryKeys.all, "platforms", filter],
    queryFn: () => api.libraryPlatforms?.(filter) ?? Promise.resolve([]),
    enabled,
  });
}
export function useLibraryStats() {
  const api = useLibraryApi();
  return useQuery({ queryKey: libraryKeys.stats, queryFn: () => api.libraryStats() });
}
export function useSearchGames(query: string) {
  const api = useLibraryApi();
  const text = query.trim();
  return useQuery({
    queryKey: [...libraryKeys.games, "suggestions", text],
    queryFn: () =>
      api.listGames({ query: text, filter: "all", sort: "title", offset: 0, limit: 6 }),
    enabled: text.length > 0,
    // The last suggestions stay up while the next ones load.
    placeholderData: keepPreviousData,
  });
}
/** How many games match `input`, without loading them; `enabled: false`
 * skips asking. The last count stays while the next one loads. */
export function useMatchCount(input: LibraryQuery, { enabled = true } = {}) {
  const api = useLibraryApi();
  const query = normalizeLibraryQuery(input);
  return useQuery({
    queryKey: [...libraryKeys.games, query, "count"],
    queryFn: async () => (await api.listGames({ ...query, offset: 0, limit: 1 })).total,
    enabled,
    placeholderData: keepPreviousData,
  });
}
export function useSessions(gameId?: string) {
  const api = useLibraryApi();
  return useQuery({
    queryKey: libraryKeys.sessions(gameId),
    queryFn: () => api.listSessions(gameId),
  });
}
/** How many games are running. */
export function useRunningCount() {
  const api = useLibraryApi();
  return useQuery({
    queryKey: [...libraryKeys.sessions(), "running"],
    queryFn: async () =>
      api.runningCount
        ? api.runningCount()
        : (await api.listSessions()).filter((session) => session.endedAt === null).length,
  });
}
export function useStopGame() {
  const api = useLibraryApi();
  const client = useQueryClient();
  return useMutation({
    mutationFn: async (sessionId: string) => {
      if (!api.stopGame) throw new Error("Stopping games isn't available here");
      await api.stopGame(sessionId);
    },
    onSuccess: () => client.invalidateQueries({ queryKey: libraryKeys.all }),
  });
}
export function useLaunchGame() {
  const api = useLibraryApi();
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ entryId, choice }: { entryId: string; choice?: LaunchChoice }) =>
      api.launchGame(entryId, choice),
    onSuccess: () => client.invalidateQueries({ queryKey: libraryKeys.all }),
  });
}
