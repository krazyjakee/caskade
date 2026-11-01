import { Select } from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { Link, useNavigate } from "@tanstack/react-router";
import { ArrowLeft, CheckSquare, Grid2X2, List, ListPlus, SearchX, X } from "lucide-react";
import { useState } from "react";
import { Button } from "react-aria-components";
import { useCollectionsApi } from "../../api/collections";
import { useGameEditApi } from "../../api/gameEdit";
import { useGameWindow, useLibraryPlatforms, useTitleLetters } from "../../api/hooks";
import { nonPcPlatforms } from "../../api/presentation";
import type { LibraryFilter, LibraryLayout, LibrarySort } from "../../api/types";
import { CollectionDialog } from "../../components/collections/CollectionDialog";
import { FailureNotice } from "../../components/FailureNotice";
import { SelectionBar } from "../../components/gameSelection/SelectionBar";
import {
  SelectionScope,
  useGameSelectionStore,
  useSelectionVersion,
} from "../../components/gameSelection/selection";
import { useInputAction, useScreenTabs } from "../../input";
import { AddGame } from "../activity/AddGame";
import { LibraryDropHint, useLibraryDrop } from "./LibraryDrop";
import { LibraryTableHeader } from "./LibraryRow";
import { presentPlatform, sortPlatforms } from "./platforms/catalog";
import { PlatformGrid } from "./platforms/PlatformGrid";
import { VirtualLibrary } from "./VirtualLibrary";
import { AddWebGame } from "./web/AddWebGame";
import { useWebGames, WebMosaic } from "./web/WebMosaic";

/** Lists sorted by title get an alphabet bar from this many games. */
const ALPHABET_BAR_MIN_GAMES = 50;

/** In the sidebar's order. Recently played is the default; All Games comes last. */
export const libraryFilters = [
  "recent",
  "installed",
  "owned",
  "emulated",
  "web",
  "favorites",
  "all",
] as const;
export type { LibraryFilter } from "../../api/types";

export const filterLabels: Record<LibraryFilter, MessageDescriptor> = {
  all: msg({ message: "All Games", context: "library filter" }),
  installed: msg({ message: "Installed", context: "library filter" }),
  owned: msg({
    message: "Ready to Install",
    context: "library filter",
    comment: "Games a connected store account owns that aren't installed",
  }),
  recent: msg({ message: "Recently Played", context: "library filter" }),
  emulated: msg({ message: "Emulated", context: "library filter" }),
  web: msg({
    message: "Web",
    context: "library filter",
    comment: "The library's tab of games played in the browser",
  }),
  favorites: msg({ message: "Favorites", context: "library filter" }),
};
export function LibraryView({
  filter,
  query,
  sort = "recent",
  layout = "grid",
  platform,
}: {
  filter: LibraryFilter;
  query?: string;
  sort?: LibrarySort;
  layout?: LibraryLayout;
  /** Only this platform's games. */
  platform?: string;
}) {
  const { t } = useLingui();
  const navigate = useNavigate();
  const breakdown = usePlatformBreakdown({ filter, query, sort, platform });
  const result = useGameWindow({ filter, query, sort, platform });
  // The Web tab shows its games as a mosaic, every one at once.
  const web = filter === "web";
  const mosaic = useWebGames({ filter, query, sort, platform }, { enabled: web });
  const total = result.total ?? 0;
  const alphabet = filter !== "web" && sort === "title" && total >= ALPHABET_BAR_MIN_GAMES;
  const letters = useTitleLetters({ filter, query, sort, platform }, { enabled: alphabet });
  const drop = useLibraryDrop();
  const collections = useCollectionsApi();
  const [saving, setSaving] = useState(false);
  // Choosing several games for one edit (#173).
  const selection = useGameSelectionStore(result.game);
  useSelectionVersion(selection);
  const edits = useGameEditApi();
  const selectable = Boolean(edits) && !web && total > 0;
  useInputAction("back", () => selection.stop(), {
    label: t`Done selecting`,
    enabled: selection.active,
  });
  // LB/RB step through the filters (the sidebar's links), in place.
  useScreenTabs(
    libraryFilters,
    filter,
    (next) =>
      void navigate({
        to: "/library",
        search: { filter: next, q: query, sort, layout, platform },
        replace: true,
      }),
  );
  return (
    <div data-view="library" ref={drop.ref} className="relative flex flex-1 flex-col">
      {drop.active && <LibraryDropHint />}
      {saving && (
        <CollectionDialog
          preset={{ type: "smart", query: { filter, sort, query, platform } }}
          close={() => setSaving(false)}
          onSaved={(id) =>
            void navigate({ to: "/collections/$collectionId", params: { collectionId: id } })
          }
        />
      )}
      <h1 className="sr-only">{query ? t`Results for “${query}”` : t`Library`}</h1>
      {drop.error && (
        <p
          role="alert"
          data-part="drop-error"
          className="mt-4 flex items-start justify-between gap-3 rounded-control border border-danger p-3 text-sm text-danger"
        >
          {drop.error}
          <Button aria-label={t`Dismiss`} className="shrink-0" onPress={drop.clearError}>
            <X className="size-4" />
          </Button>
        </p>
      )}
      <div className="mt-4 mb-4 flex flex-wrap items-center gap-2" data-part="toolbar">
        {selectable && (
          <Button
            data-part="select"
            aria-pressed={selection.active}
            onPress={() => (selection.active ? selection.stop() : selection.start())}
            className="flex items-center gap-1.5 rounded-control border border-border px-3 py-2 text-sm text-fg-muted outline-none data-focus-visible:focus-ring data-hovered:text-fg aria-pressed:border-accent-alt aria-pressed:text-fg"
          >
            <CheckSquare className="size-4" />
            <Trans comment="Starts choosing several games in the library">Select</Trans>
          </Button>
        )}
        {web ? <AddWebGame /> : <AddGame />}
        {platform && (
          <div className="flex items-center gap-2">
            <Link
              to="/library"
              search={{ filter, q: query, sort, layout }}
              data-part="platform-back"
              className="flex items-center gap-1.5 rounded-control border border-border px-3 py-2 text-sm text-fg-muted outline-none hover:text-fg focus-visible:focus-ring"
            >
              <ArrowLeft className="size-4" />
              <Trans comment="Goes back from one platform's games to the list of platforms">
                All platforms
              </Trans>
            </Link>
            <span
              data-part="platform"
              title={platform}
              className="flex items-center gap-1.5 px-1 text-sm font-semibold text-fg"
            >
              <PlatformChipLabel name={platform} />
            </span>
          </div>
        )}
        {query && (
          <Link
            to="/library"
            search={{ filter, sort, layout, platform }}
            data-part="clear-search"
            className="flex items-center gap-1.5 rounded-full border border-accent-alt py-1 pr-2 pl-3 text-sm text-fg outline-none focus-visible:focus-ring"
          >
            {t`Results for “${query}”`}
            <X className="size-4" aria-label={t`Clear search`} />
          </Link>
        )}
        <div className="ml-auto flex flex-wrap items-center gap-2">
          {collections && (
            <Button
              data-part="save-collection"
              onPress={() => setSaving(true)}
              className="flex items-center gap-1.5 rounded-control border border-border px-3 py-2 text-sm text-fg-muted outline-none data-focus-visible:focus-ring data-hovered:text-fg"
            >
              <ListPlus className="size-4" />
              <Trans>Save as collection</Trans>
            </Button>
          )}
          <Select
            aria-label={t`Sort games`}
            value={sort}
            options={[
              { id: "recent", label: t({ message: "Recently played", context: "library sort" }) },
              { id: "title", label: t({ message: "Title", context: "library sort" }) },
              { id: "popular", label: t({ message: "Popular", context: "library sort" }) },
              { id: "playtime", label: t({ message: "Playtime", context: "library sort" }) },
              { id: "added", label: t({ message: "Recently added", context: "library sort" }) },
              { id: "released", label: t({ message: "Release date", context: "library sort" }) },
            ]}
            onChange={(next) => {
              void navigate({
                to: "/library",
                search: { filter, q: query, sort: next, layout, platform },
              });
            }}
          />
          {!web &&
            (["grid", "list"] as const).map((mode) => (
              <Link
                key={mode}
                to="/library"
                search={{ filter, q: query, sort, layout: mode, platform }}
                aria-label={mode === "grid" ? t`grid view` : t`list view`}
                aria-current={mode === layout ? "page" : undefined}
                className="rounded-control border border-border p-2 text-fg-muted data-selected:text-accent-alt"
                data-selected={mode === layout || undefined}
              >
                {mode === "grid" ? <Grid2X2 className="size-4" /> : <List className="size-4" />}
              </Link>
            ))}
        </div>
      </div>
      {result.error ? (
        <FailureNotice
          error={result.error}
          title={t`Couldn't load your library`}
          onRetry={() => {
            void result.refetch();
          }}
        />
      ) : web && mosaic.error ? (
        // A failed load isn't "no web games yet" (#160).
        <FailureNotice
          error={mosaic.error}
          title={t`Couldn't load your web games`}
          onRetry={() => {
            void mosaic.refetch();
          }}
        />
      ) : web && mosaic.data && mosaic.data.items.length > 0 ? (
        <WebMosaic games={mosaic.data.items} />
      ) : breakdown ? (
        <PlatformGrid platforms={breakdown} filter={filter} sort={sort} layout={layout} />
      ) : !web && total > 0 ? (
        <SelectionScope selection={selection}>
          <VirtualLibrary
            game={result.game}
            total={total}
            layout={layout}
            header={
              <LibraryTableHeader sort={sort} search={{ filter, q: query, layout, platform }} />
            }
            loading={result.isFetching}
            onRange={result.setRange}
            letters={alphabet ? letters.data : undefined}
          />
          <SelectionBar
            selection={selection}
            total={total}
            loadedIds={() => loadedIds(result.game, total)}
            allIds={edits ? () => edits.gameIds({ filter, sort, query, platform }) : null}
          />
        </SelectionScope>
      ) : (
        !result.isPending &&
        !(web && mosaic.isPending) && (
          <div
            data-part="empty"
            className="flex flex-col items-center gap-2 rounded-card border border-dashed border-border py-16 text-fg-muted"
          >
            <SearchX className="size-8" />
            <p>
              {query
                ? t`No games match your search in this filter.`
                : filter === "recent"
                  ? t`You haven't played anything yet.`
                  : web
                    ? t`No web games yet. Add one with Add web game.`
                    : t`No games in this filter yet.`}
            </p>
            {filter !== "all" && (
              <Link
                to="/library"
                search={{ filter: "all", q: query, sort, layout, platform }}
                className="text-sm text-accent-alt"
              >
                <Trans>Show all games</Trans>
              </Link>
            )}
          </div>
        )
      )}
    </div>
  );
}

function PlatformChipLabel({ name }: { name: string }) {
  const look = presentPlatform(name);
  return (
    <>
      {look.known && look.art && (
        <img src={look.art} alt="" draggable={false} className="h-5 w-7 object-contain" />
      )}
      {look.title}
    </>
  );
}

/** Emulated games on more than one platform show their platforms first,
 * in the library's sort, until one is chosen or a search narrows them. */
function usePlatformBreakdown(view: {
  filter: LibraryFilter;
  query?: string;
  sort: LibrarySort;
  platform?: string;
}) {
  const wanted = view.filter === "emulated" && !view.platform && !view.query;
  const platforms = useLibraryPlatforms(view.filter, { enabled: wanted });
  if (!wanted || !platforms.data) return null;
  // A game also out on PC is found under its console.
  const consoles = platforms.data.filter((platform) => nonPcPlatforms(platform.name));
  return consoles.length > 1 ? sortPlatforms(consoles, view.sort) : null;
}

/** The ids of the view's games loaded so far, in order. */
export function loadedIds(gameAt: (index: number) => { id: string } | undefined, total: number) {
  const ids: string[] = [];
  for (let index = 0; index < total; index++) {
    const game = gameAt(index);
    if (game) ids.push(game.id);
  }
  return ids;
}
