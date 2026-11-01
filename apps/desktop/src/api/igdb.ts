/** IGDB metadata through Caskade's metadata server. The core caches every
 * answer on disk, so asking again for an id costs no request; only
 * `refresh` goes back to the server. Ids are strings.
 *
 * A library game's match (`GameIgdbApi`) is the core's: it finds one in
 * the background and fills the game in from it; the game page shows it
 * and lets the user change it.
 *
 * The Store's catalog (`CatalogApi`, #163) browses all of IGDB. The core
 * keeps its pages a few minutes and answers with the last copy, marked
 * `stale`, when the server can't be reached. */
import {
  keepPreviousData,
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { createContext, createElement, type ReactNode, useContext } from "react";
import {
  type CatalogReleases_Deserialize,
  type CatalogSearch_Deserialize,
  commands,
  type GameAgeRatings_Serialize,
  type GameIgdb_Serialize,
  type IgdbAgeRating,
  type IgdbCatalogFilters_Serialize,
  type IgdbCatalogPage_Serialize,
  type IgdbExternalId_Serialize,
  type IgdbGame_Serialize,
  type IgdbImage_Serialize,
  type IgdbImages_Serialize,
  type IgdbListedRelease_Serialize,
  type IgdbLookup_Serialize,
  type IgdbOwned_Serialize,
  type IgdbPlatform_Serialize,
  type IgdbReleaseDate_Serialize,
  type IgdbStoreLink_Serialize,
  type IgdbWebsite_Serialize,
  type MatchSource,
  type CatalogSort as NativeCatalogSort,
  type ReleasePrecision_Serialize,
} from "./bindings";
import { libraryKeys } from "./provider";
import { unwrap } from "./reply";

export type IgdbGame = IgdbGame_Serialize;
export type IgdbImage = IgdbImage_Serialize;
export type IgdbImages = IgdbImages_Serialize;
export type IgdbPlatform = IgdbPlatform_Serialize;
export type IgdbExternalId = IgdbExternalId_Serialize;
export type IgdbLookup = IgdbLookup_Serialize;
export type GameIgdb = GameIgdb_Serialize;
export type { IgdbAgeRating, MatchSource };
export type GameAgeRatings = GameAgeRatings_Serialize;
export type IgdbReleaseDate = IgdbReleaseDate_Serialize;
export type ReleasePrecision = ReleasePrecision_Serialize;
export type IgdbStoreLink = IgdbStoreLink_Serialize;
export type IgdbWebsite = IgdbWebsite_Serialize;
export type CatalogPage = IgdbCatalogPage_Serialize;
export type ListedRelease = IgdbListedRelease_Serialize;
export type CatalogFilters = IgdbCatalogFilters_Serialize;
export type IgdbOwned = IgdbOwned_Serialize;
export type CatalogSearch = CatalogSearch_Deserialize;
export type CatalogReleases = CatalogReleases_Deserialize;

export interface IgdbApi {
  /** Games in the order asked for; ids IGDB has no game for are in `notFound`, ids the server
   * couldn't answer yet in `pending`. Fails only when the server can't be reached and none of
   * the ids are cached. */
  games(ids: string[]): Promise<IgdbLookup>;
  /** Fetches `ids` again, replacing cached answers older than ten minutes; at most 500 ids. */
  refresh(ids: string[]): Promise<IgdbLookup>;
}

function native<T>(request: () => Promise<T>): Promise<T> {
  if (!isTauri()) return Promise.reject(new Error("Open Caskade desktop to look up games."));
  return request();
}

export const nativeIgdbApi: IgdbApi = {
  games: (ids) => native(() => unwrap(commands.igdbGames(ids))),
  refresh: (ids) => native(() => unwrap(commands.igdbRefresh(ids))),
};

export interface GameIgdbApi {
  /** `null` until matching has looked at the game. */
  game(gameId: string): Promise<GameIgdb | null>;
  /** Matches the game to `igdbId` by hand, or to no game with `null`. */
  set(gameId: string, igdbId: string | null): Promise<GameIgdb>;
  /** Forgets the match and matches automatically again. */
  rematch(gameId: string): Promise<GameIgdb | null>;
  /** Fetches the IGDB game again and fills the game in from it. */
  refresh(gameId: string): Promise<GameIgdb | null>;
  /** IGDB's best matches for a title, best first, to choose one from. */
  search(query: string): Promise<IgdbGame[]>;
  /** Moves an entry out of the game it was merged into (a release that
   * isn't the same game) into a game of its own. The new game's id. */
  separate(entryId: string): Promise<string>;
  /** The game's age ratings and the minimum age they come to (#204). */
  ageRatings(gameId: string): Promise<GameAgeRatings | null>;
}

/** How many games a search shows. */
const SEARCH_LIMIT = 10;

export const nativeGameIgdbApi: GameIgdbApi = {
  game: (gameId) => native(() => unwrap(commands.gameIgdb(gameId))),
  set: (gameId, igdbId) => native(() => unwrap(commands.gameIgdbSet(gameId, igdbId))),
  rematch: (gameId) => native(() => unwrap(commands.gameIgdbRematch(gameId))),
  refresh: (gameId) => native(() => unwrap(commands.gameIgdbRefresh(gameId))),
  search: (query) => native(() => unwrap(commands.igdbSearch(query, SEARCH_LIMIT))),
  separate: (entryId) => native(() => unwrap(commands.gameSeparateEntry(entryId))),
  ageRatings: (gameId) => native(() => unwrap(commands.gameAgeRatings(gameId))),
};

const GameIgdbContext = createContext<GameIgdbApi>(nativeGameIgdbApi);

/** Tests and previews supply their own API; the app uses the native one. */
export function GameIgdbProvider({ api, children }: { api: GameIgdbApi; children: ReactNode }) {
  return createElement(GameIgdbContext.Provider, { value: api }, children);
}

/** Under `library`, so the core's library events (a match being applied)
 * refresh it. */
export const gameIgdbKey = (gameId: string) => [...libraryKeys.all, "igdb", gameId];

/** A local read, so a failure isn't worth retrying: library refreshes
 * (a launch's, say) wait for this query too. */
export function useGameIgdb(gameId: string) {
  const api = useContext(GameIgdbContext);
  return useQuery({
    queryKey: gameIgdbKey(gameId),
    queryFn: () => api.game(gameId),
    retry: false,
  });
}

/** Searches ask the metadata server, and so IGDB, every time: keep each
 * answer a while. Off until there is something to search for. */
export function useIgdbSearch(query: string) {
  const api = useContext(GameIgdbContext);
  const text = query.trim();
  return useQuery({
    queryKey: ["igdb", "search", text],
    queryFn: () => api.search(text),
    enabled: text !== "",
    retry: false,
    staleTime: 5 * 60_000,
  });
}

/** Under `library`, so a match being applied refreshes it. */
export function useGameAgeRatings(gameId: string) {
  const api = useContext(GameIgdbContext);
  return useQuery({
    queryKey: [...libraryKeys.all, "age-ratings", gameId],
    queryFn: () => api.ageRatings(gameId),
    retry: false,
  });
}

export type GameIgdbChange =
  | { type: "set"; igdbId: string | null }
  | { type: "rematch" }
  | { type: "refresh" };

/** Changes to a game's match in flight: its page waits for them rather
 * than show the game as gone when one merged it into another. */
export const gameIgdbChangeKey = (gameId: string) => ["gameIgdbChange", gameId];

/** A change fills the game in again, so the whole library refreshes. A
 * change that merges the game into another that is the same game answers
 * for that one, and `onMerged` gets its id. */
export function useChangeGameIgdb(gameId: string, onMerged?: (gameId: string) => void) {
  const api = useContext(GameIgdbContext);
  const client = useQueryClient();
  return useMutation({
    mutationKey: gameIgdbChangeKey(gameId),
    mutationFn: (change: GameIgdbChange) =>
      change.type === "set"
        ? api.set(gameId, change.igdbId)
        : change.type === "rematch"
          ? api.rematch(gameId)
          : api.refresh(gameId),
    onSuccess: (match) => {
      client.setQueryData(gameIgdbKey(match?.gameId ?? gameId), match);
      if (match && match.gameId !== gameId) onMerged?.(match.gameId);
      return client.invalidateQueries({ queryKey: libraryKeys.all });
    },
  });
}

/** Separates one of a game's entries into a game of its own. */
export function useSeparateEntry() {
  const api = useContext(GameIgdbContext);
  const client = useQueryClient();
  return useMutation({
    mutationFn: (entryId: string) => api.separate(entryId),
    onSuccess: () => client.invalidateQueries({ queryKey: libraryKeys.all }),
  });
}

// ── The Store's catalog (#163) ──

export interface CatalogApi {
  /** A page of games matching the search; without a query, the most-rated. */
  search(search: CatalogSearch): Promise<CatalogPage>;
  /** A page of the last 90 days' releases, newest first. */
  recent(releases: CatalogReleases): Promise<CatalogPage>;
  /** A page of releases to come, soonest first; with `tbd`, undated ones. */
  upcoming(releases: CatalogReleases): Promise<CatalogPage>;
  filters(): Promise<CatalogFilters>;
  /** Games for the Store's pages: answers older than a day are fetched again. */
  games(ids: string[]): Promise<IgdbLookup>;
  /** The library games matched to these IGDB games. */
  owned(ids: string[]): Promise<IgdbOwned[]>;
}

const ipcCatalogApi: CatalogApi = {
  search: (search) => native(() => unwrap(commands.igdbCatalogSearch(search))),
  recent: (releases) => native(() => unwrap(commands.igdbCatalogRecent(releases))),
  upcoming: (releases) => native(() => unwrap(commands.igdbCatalogUpcoming(releases))),
  filters: () => native(() => unwrap(commands.igdbCatalogFilters())),
  games: (ids) => native(() => unwrap(commands.igdbCatalogGames(ids))),
  owned: (ids) => native(() => unwrap(commands.igdbOwned(ids))),
};

/** Dev builds started with `VITE_CASKADE_FIXTURES=1` browse a fixture
 * catalog instead (README.md). */
export const nativeCatalogApi: CatalogApi =
  import.meta.env.DEV && import.meta.env.VITE_CASKADE_FIXTURES === "1"
    ? lazyCatalogFixtures()
    : ipcCatalogApi;

function lazyCatalogFixtures(): CatalogApi {
  let loading: Promise<CatalogApi> | undefined;
  const load = () => {
    loading ??= import("./catalogFixtures").then((module) => module.createCatalogFixtureApi());
    return loading;
  };
  return {
    search: (search) => load().then((api) => api.search(search)),
    recent: (releases) => load().then((api) => api.recent(releases)),
    upcoming: (releases) => load().then((api) => api.upcoming(releases)),
    filters: () => load().then((api) => api.filters()),
    games: (ids) => load().then((api) => api.games(ids)),
    owned: (ids) => load().then((api) => api.owned(ids)),
  };
}

const CatalogContext = createContext<CatalogApi>(nativeCatalogApi);

/** Tests and previews supply their own catalog; the app uses the core's. */
export function CatalogProvider({ api, children }: { api: CatalogApi; children: ReactNode }) {
  return createElement(CatalogContext.Provider, { value: api }, children);
}

/** Games per catalog page. */
export const CATALOG_PAGE = 24;

export type CatalogShelf = "recent" | "upcoming" | "tbd";

export type CatalogSort = NativeCatalogSort;

export interface CatalogQuery {
  sort?: CatalogSort;
  query?: string;
  platform?: string;
  genre?: string;
  /** A release year: first released within it. */
  year?: number;
  /** First released within the last year, to today (without `year`). */
  pastYear?: boolean;
}

/** The UTC day a year before today, `YYYY-MM-DD`. */
function yearAgo(): string {
  const day = new Date();
  day.setUTCFullYear(day.getUTCFullYear() - 1);
  return day.toISOString().slice(0, 10);
}

/** The catalog asks the metadata server: keep answers a while, and keep
 * the last results on screen while the next search loads. A superseded
 * search's answer never shows: its key is no longer the one rendered. */
const catalogCache = { retry: false, staleTime: 5 * 60_000 } as const;

export function useCatalogSearch(search: CatalogQuery) {
  const api = useContext(CatalogContext);
  const query = search.query?.trim() || null;
  const year = search.year;
  const since = !year && search.pastYear ? yearAgo() : null;
  return useInfiniteQuery({
    queryKey: [
      "igdb",
      "catalog",
      "search",
      query,
      search.platform,
      search.genre,
      year,
      since,
      search.sort,
    ],
    queryFn: ({ pageParam }) =>
      api.search({
        query,
        sort: search.sort,
        offset: pageParam,
        limit: CATALOG_PAGE,
        platforms: search.platform ? [search.platform] : [],
        genres: search.genre ? [search.genre] : [],
        releasedFrom: year ? `${year}-01-01` : since,
        releasedTo: year ? `${year}-12-31` : null,
      }),
    initialPageParam: 0,
    getNextPageParam: (page) => page.nextOffset ?? undefined,
    placeholderData: keepPreviousData,
    ...catalogCache,
  });
}

/** Recent or upcoming releases, `limit` a page (a shelf asks for fewer). */
export function useCatalogReleases(
  shelf: CatalogShelf,
  { platform, genre, limit = CATALOG_PAGE }: { platform?: string; genre?: string; limit?: number },
) {
  const api = useContext(CatalogContext);
  return useInfiniteQuery({
    queryKey: ["igdb", "catalog", shelf, platform, genre, limit],
    queryFn: ({ pageParam }) => {
      const releases: CatalogReleases = {
        offset: pageParam,
        limit,
        platforms: platform ? [platform] : [],
        genres: genre ? [genre] : [],
        tbd: shelf === "tbd",
      };
      return shelf === "recent" ? api.recent(releases) : api.upcoming(releases);
    },
    initialPageParam: 0,
    getNextPageParam: (page) => page.nextOffset ?? undefined,
    placeholderData: keepPreviousData,
    ...catalogCache,
  });
}

export function useCatalogFilters() {
  const api = useContext(CatalogContext);
  return useQuery({
    queryKey: ["igdb", "catalog", "filters"],
    queryFn: () => api.filters(),
    retry: false,
    staleTime: 24 * 60 * 60_000,
  });
}

/** Catalog games by id, in the order asked for; off without ids. */
export function useCatalogGames(ids: string[]) {
  const api = useContext(CatalogContext);
  return useQuery({
    queryKey: ["igdb", "catalog", "games", ids],
    queryFn: () => api.games(ids),
    enabled: ids.length > 0,
    ...catalogCache,
  });
}

/** Which of `ids` are in the library: under `library`, so library changes
 * (an install, a new match) refresh it. */
export function useCatalogOwned(ids: string[]) {
  const api = useContext(CatalogContext);
  const sorted = [...new Set(ids)].sort();
  return useQuery({
    queryKey: [...libraryKeys.all, "igdb-owned", sorted],
    queryFn: () => api.owned(sorted),
    enabled: sorted.length > 0,
    retry: false,
  });
}
