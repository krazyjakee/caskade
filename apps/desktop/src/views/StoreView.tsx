/** The Store (#163): a discovery catalog of all of IGDB through the metadata
 * server, not only the library's games. Browse searches it (or lists the
 * past year's most-rated) with platform, genre and release-year filters;
 * Recently released lists the last 90 days' releases, newest first; Upcoming lists
 * releases to come, soonest first, or the undated (TBD) ones. Every view's
 * state is in the URL, so Back from a game's page comes back to it.
 *
 * Cards show the library's own status (Installed, In library) for games
 * matched to the same IGDB game; store links never claim a price or that a
 * game is on sale. The Store installs nothing itself: installs started from
 * a game's page later go through the install commands and their `install`
 * permission (#205). */
import { Button, SegmentedControl, Select, TextField } from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { Link, useNavigate } from "@tanstack/react-router";
import { CalendarClock, CalendarDays, Search, SearchX } from "lucide-react";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import {
  type CatalogPage,
  type CatalogShelf,
  type CatalogSort,
  type IgdbOwned,
  useCatalogFilters,
  useCatalogOwned,
  useCatalogReleases,
  useCatalogSearch,
} from "../api/igdb";
import { ShelfRow } from "../components/ShelfRow";
import { useDefaultFocus, useScreenTabs } from "../input";
import { STORE_GAMES, useOpenStoreGame } from "./game/origin";
import { CatalogCard } from "./store/CatalogCard";
import { listedGames } from "./store/releases";

export const storeTabs = ["browse", "recent", "upcoming"] as const;
export type StoreTab = (typeof storeTabs)[number];

/** The Store's view, as its URL holds it. */
export interface StoreSearch {
  tab: StoreTab;
  sort?: CatalogSort;
  q?: string;
  /** An IGDB platform id. */
  platform?: string;
  /** An IGDB genre id. */
  genre?: string;
  /** First released in this year (Browse). */
  year?: number;
  /** Upcoming: the undated releases. */
  tbd?: boolean;
}

const storeSorts: readonly CatalogSort[] = ["relevance", "mostRated", "popular", "newest"];

const IGDB_ID = /^\d{1,18}$/;

export function validateStoreSearch(search: Record<string, unknown>): StoreSearch {
  const tab = storeTabs.find((t) => t === search.tab) ?? "browse";
  const text = (value: unknown) =>
    typeof value === "string" && value.trim() ? value.trim().slice(0, 100) : undefined;
  const id = (value: unknown) => {
    const raw = typeof value === "number" ? String(value) : text(value);
    return raw && IGDB_ID.test(raw) ? raw : undefined;
  };
  const year = Number(search.year);
  return {
    tab,
    ...(storeSorts.includes(search.sort as CatalogSort) && { sort: search.sort as CatalogSort }),
    ...(text(search.q) && { q: text(search.q) }),
    ...(id(search.platform) && { platform: id(search.platform) }),
    ...(id(search.genre) && { genre: id(search.genre) }),
    ...(Number.isInteger(year) && year >= 1950 && year <= 2100 && { year }),
    ...((search.tbd === true || search.tbd === "true") && { tbd: true }),
  };
}

const tabLabels: Record<StoreTab, MessageDescriptor> = {
  browse: msg({ message: "Browse", comment: "A Store tab: search and filter the whole catalog" }),
  recent: msg({ message: "Recently released", comment: "A Store tab and shelf" }),
  upcoming: msg({ message: "Upcoming", comment: "A Store tab and shelf: releases to come" }),
};

/** Search waits this long after the last keystroke. */
const DEBOUNCE_MS = 350;
const SHELF_SIZE = 12;

export function StoreView({ search }: { search: StoreSearch }) {
  const { t } = useLingui();
  const navigate = useNavigate();
  const go = (next: StoreSearch) => void navigate({ to: "/store", search: next, replace: true });
  const tablist = useRef<HTMLDivElement>(null);
  const tabFocus = useRef<HTMLAnchorElement>(null);
  useScreenTabs(storeTabs, search.tab, (tab) => go({ ...search, tab }), { tabs: tablist });
  useDefaultFocus(tabFocus);
  return (
    <div data-view="store" className="mt-4">
      <h1 className="font-display text-2xl font-semibold">
        <Trans>Store</Trans>
      </h1>
      <p className="mt-1 text-sm text-fg-muted">
        <Trans>
          Every game IGDB knows, with where it is sold. Prices and availability are the stores' own.
        </Trans>
      </p>
      <div ref={tablist} role="tablist" className="mt-5 mb-4 flex flex-wrap gap-2">
        {storeTabs.map((tab) => {
          const selected = tab === search.tab;
          return (
            <Link
              key={tab}
              ref={selected ? tabFocus : undefined}
              to="/store"
              search={{ ...search, tab }}
              replace
              role="tab"
              aria-selected={selected}
              data-selected={selected || undefined}
              data-focus-default={selected || undefined}
              className="rounded-full border border-border bg-surface-raised px-4 py-1.5 text-sm text-fg-muted transition-colors duration-(--cs-motion-duration-fast) hover:text-fg focus-visible:focus-ring data-selected:border-accent-alt/50 data-selected:bg-accent/20 data-selected:text-fg"
            >
              {t(tabLabels[tab])}
            </Link>
          );
        })}
      </div>
      {search.tab === "browse" ? (
        <BrowseTab search={search} go={go} />
      ) : (
        <ReleasesTab search={search} go={go} />
      )}
    </div>
  );
}

type Go = (next: StoreSearch) => void;

function BrowseTab({ search, go }: { search: StoreSearch; go: Go }) {
  const { t } = useLingui();
  const [text, setText] = useState(search.q ?? "");
  const latest = useRef({ search, go });
  latest.current = { search, go };
  // The query this field last put in the URL: a different one came from
  // Back or a link, and the field follows it.
  const pushed = useRef(search.q);
  useEffect(() => {
    if (search.q === pushed.current) return;
    pushed.current = search.q;
    setText(search.q ?? "");
  }, [search.q]);
  const submit = (value: string) => {
    const q = value.trim() || undefined;
    const { search: now, go: navigate } = latest.current;
    if (q === now.q) return;
    pushed.current = q;
    const { q: _old, ...rest } = now;
    navigate(q ? { ...rest, q } : rest);
  };
  // Only the last keystroke's search goes to the server.
  // biome-ignore lint/correctness/useExhaustiveDependencies: `submit` reads the latest props through a ref.
  useEffect(() => {
    const timer = setTimeout(() => submit(text), DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [text]);
  const narrowed = Boolean(search.q || search.platform || search.genre || search.year);
  const results = useCatalogSearch({
    query: search.q,
    sort: search.sort,
    platform: search.platform,
    genre: search.genre,
    year: search.year,
    pastYear:
      !narrowed && (!search.sort || search.sort === "relevance" || search.sort === "mostRated"),
  });
  const query = search.q;
  return (
    <>
      <div className="flex flex-wrap items-end gap-3">
        <TextField
          aria-label={t`Search the catalog`}
          placeholder={t`Search every game…`}
          icon={Search}
          type="search"
          value={text}
          onChange={setText}
          onKeyDown={(event) => {
            if (event.key === "Enter") submit(text);
          }}
          className="min-w-64 flex-[1_1_20rem]"
        />
        <CatalogFilters search={search} go={go} withGenre withYear />
        <Select
          aria-label={t`Sort catalog games`}
          value={
            search.sort === "mostRated" && !search.q ? "relevance" : (search.sort ?? "relevance")
          }
          options={[
            { id: "relevance", label: search.q ? t`Relevance` : t`Most rated` },
            ...(search.q ? [{ id: "mostRated", label: t`Most rated` }] : []),
            { id: "popular", label: t({ message: "Popular", context: "catalog sort" }) },
            { id: "newest", label: t`Newest` },
          ]}
          onChange={(sort) => go({ ...search, sort: sort as CatalogSort })}
        />
      </div>
      {!narrowed && (
        <>
          <ReleaseShelf shelf="recent" search={search} />
          <ReleaseShelf shelf="upcoming" search={search} />
        </>
      )}
      <h2 className="mt-8 mb-4 font-display text-xl font-semibold">
        {query
          ? t`Results for “${query}”`
          : search.sort === "popular"
            ? t`Popular games`
            : search.sort === "newest"
              ? t`Newest games`
              : narrowed
                ? t`Matching games`
                : t({
                    message: "Most rated this past year",
                    comment:
                      "The Store's games released in the last 12 months with the most ratings",
                  })}
      </h2>
      <CatalogGrid
        result={results}
        empty={query ? t`No games match “${query}”.` : t`No games match these filters.`}
      />
    </>
  );
}

function ReleasesTab({ search, go }: { search: StoreSearch; go: Go }) {
  const { t } = useLingui();
  const shelf: CatalogShelf = search.tab === "recent" ? "recent" : search.tbd ? "tbd" : "upcoming";
  const result = useCatalogReleases(shelf, { platform: search.platform, genre: search.genre });
  return (
    <>
      <div className="flex flex-wrap items-end gap-3">
        <CatalogFilters search={search} go={go} withGenre />
        {search.tab === "upcoming" && (
          <SegmentedControl
            aria-label={t`Upcoming releases`}
            value={search.tbd ? "tbd" : "dated"}
            options={[
              {
                id: "dated",
                label: t({ message: "Dated", comment: "Upcoming releases with a release date" }),
              },
              {
                id: "tbd",
                label: t({
                  message: "TBD",
                  comment: "A release date that isn't known yet (to be determined)",
                }),
              },
            ]}
            onChange={(value) => {
              const { tbd: _old, ...rest } = search;
              go(value === "tbd" ? { ...rest, tbd: true } : rest);
            }}
          />
        )}
      </div>
      <p className="mt-3 text-xs text-fg-subtle">
        {shelf === "recent"
          ? t`Releases dated to the day in the last 90 days (UTC), newest first, on each platform.`
          : shelf === "tbd"
            ? t`Announced releases without a date yet.`
            : t`Releases to come, soonest first. Month, quarter and year dates are as precise as IGDB knows them; cancelled ones are marked.`}
      </p>
      <div className="mt-4">
        <CatalogGrid
          result={result}
          empty={shelf === "recent" ? t`No recent releases here.` : t`No upcoming releases here.`}
        />
      </div>
    </>
  );
}

const ANY = "any";

function CatalogFilters({
  search,
  go,
  withGenre = false,
  withYear = false,
}: {
  search: StoreSearch;
  go: Go;
  withGenre?: boolean;
  withYear?: boolean;
}) {
  const { t } = useLingui();
  const filters = useCatalogFilters();
  const platforms = filters.data?.platforms ?? [];
  const genres = filters.data?.genres ?? [];
  const thisYear = new Date().getUTCFullYear();
  const years = Array.from({ length: thisYear + 3 - 1970 }, (_, i) => String(thisYear + 2 - i));
  const pick = (key: "platform" | "genre", value: string) => {
    const { [key]: _old, ...rest } = search;
    go(value === ANY ? rest : { ...rest, [key]: value });
  };
  return (
    <>
      <Select
        aria-label={t({ message: "Platform", comment: "The Store's platform filter" })}
        isDisabled={!filters.data}
        value={search.platform ?? ANY}
        options={[
          { id: ANY, label: t`All platforms` },
          ...platforms.map((p) => ({ id: p.id, label: p.name })),
        ]}
        onChange={(value) => pick("platform", value)}
        className="w-52"
      />
      {withGenre && (
        <Select
          aria-label={t({ message: "Genre", comment: "The Store's genre filter" })}
          isDisabled={!filters.data}
          value={search.genre ?? ANY}
          options={[
            { id: ANY, label: t`All genres` },
            ...genres.map((g) => ({ id: g.id, label: g.name })),
          ]}
          onChange={(value) => pick("genre", value)}
          className="w-48"
        />
      )}
      {withYear && (
        <Select
          aria-label={t`Release year`}
          value={search.year ? String(search.year) : ANY}
          options={[{ id: ANY, label: t`Any year` }, ...years.map((y) => ({ id: y, label: y }))]}
          onChange={(value) => {
            const { year: _old, ...rest } = search;
            go(value === ANY ? rest : { ...rest, year: Number(value) });
          }}
          className="w-36"
        />
      )}
    </>
  );
}

/** The library's games for the catalog games on screen, by IGDB id. */
function useOwnedMap(ids: string[]): Map<string, IgdbOwned> {
  const owned = useCatalogOwned(ids);
  return useMemo(() => {
    const map = new Map<string, IgdbOwned>();
    // An installed copy wins over one that isn't.
    for (const game of owned.data ?? [])
      if (!map.get(game.igdbId)?.installed) map.set(game.igdbId, game);
    return map;
  }, [owned.data]);
}

interface PagedResult {
  data?: { pages: CatalogPage[] };
  isPending: boolean;
  isError: boolean;
  error: Error | null;
  isFetching: boolean;
  isFetchingNextPage: boolean;
  isFetchNextPageError: boolean;
  isPlaceholderData: boolean;
  hasNextPage: boolean;
  fetchNextPage: () => unknown;
  refetch: () => unknown;
}

/** Loading, failure and stale states shared by every catalog list. */
function Notices({ result, children }: { result: PagedResult; children?: ReactNode }) {
  const { t } = useLingui();
  const pages = result.data?.pages ?? [];
  const error = result.error?.message;
  return (
    <>
      {result.isPending && (
        <p role="status" className="text-sm text-fg-muted">
          <Trans>Loading games…</Trans>
        </p>
      )}
      {result.isError && pages.length === 0 && (
        <div role="alert" className="flex flex-wrap items-center gap-3 text-sm text-fg-muted">
          <span>
            {error
              ? t`Could not reach the game catalog: ${error}`
              : t`Could not reach the game catalog.`}
          </span>
          <Button size="sm" onPress={() => void result.refetch()}>
            <Trans>Try again</Trans>
          </Button>
        </div>
      )}
      {pages.some((page) => page.stale) && (
        <p role="status" className="mb-3 text-xs text-warning">
          <Trans>Showing saved results: the metadata server can't be reached right now.</Trans>
        </p>
      )}
      {children}
    </>
  );
}

function CatalogGrid({ result, empty }: { result: PagedResult; empty: string }) {
  const { t } = useLingui();
  const openGame = useOpenStoreGame();
  const listed = listedGames(result.data?.pages ?? []);
  const owned = useOwnedMap(listed.map(({ game }) => game.id));
  return (
    <Notices result={result}>
      {result.data && listed.length === 0 && !result.isPlaceholderData && (
        <div
          data-part="empty"
          className="flex flex-col items-center gap-2 rounded-card border border-dashed border-border py-16 text-center text-fg-muted"
        >
          <SearchX className="size-8 text-fg-subtle" />
          <p className="text-sm">{empty}</p>
        </div>
      )}
      {listed.length > 0 && (
        <div
          data-catalog="results"
          aria-busy={result.isPlaceholderData || undefined}
          className="grid grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-4 aria-busy:opacity-60"
        >
          {listed.map(({ game, release }, i) => (
            <CatalogCard
              key={game.id}
              game={game}
              href={`${STORE_GAMES}/${encodeURIComponent(game.id)}`}
              onOpen={openGame}
              release={release}
              owned={owned.get(game.id)}
              index={i % 24}
            />
          ))}
        </div>
      )}
      {result.isFetchNextPageError && (
        <p role="alert" className="mt-4 text-sm text-fg-muted">
          <Trans>Could not load more games.</Trans>
        </p>
      )}
      {result.hasNextPage && !result.isPlaceholderData && (
        <div className="mt-6 flex justify-center">
          <Button
            onPress={() => void result.fetchNextPage()}
            isDisabled={result.isFetchingNextPage}
          >
            {result.isFetchingNextPage ? t`Loading…` : t`Load more`}
          </Button>
        </div>
      )}
    </Notices>
  );
}

/** Browse's shelves of recent and upcoming releases, with a link to each
 * list. */
function ReleaseShelf({ shelf, search }: { shelf: "recent" | "upcoming"; search: StoreSearch }) {
  const { t } = useLingui();
  const result = useCatalogReleases(shelf, { limit: SHELF_SIZE });
  const openGame = useOpenStoreGame();
  const listed = listedGames(result.data?.pages ?? []).slice(0, SHELF_SIZE);
  const owned = useOwnedMap(listed.map(({ game }) => game.id));
  if (result.isSuccess && listed.length === 0) return null;
  return (
    <ShelfRow
      id={`store-${shelf}`}
      title={t(tabLabels[shelf])}
      icon={shelf === "recent" ? CalendarDays : CalendarClock}
      viewAll={
        <Link
          to="/store"
          search={{ tab: shelf, ...(search.platform && { platform: search.platform }) }}
          data-part="view-all"
          className="text-sm text-accent-alt"
        >
          <Trans>View all →</Trans>
        </Link>
      }
      notice={<Notices result={result} />}
    >
      {listed.map(({ game, release }) => (
        <CatalogCard
          key={game.id}
          game={game}
          href={`${STORE_GAMES}/${encodeURIComponent(game.id)}`}
          onOpen={openGame}
          release={release}
          owned={owned.get(game.id)}
          layout="shelf"
        />
      ))}
    </ShelfRow>
  );
}
