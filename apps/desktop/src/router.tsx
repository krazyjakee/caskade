import {
  createRootRoute,
  createRoute,
  createRouter,
  type SearchSchemaInput,
} from "@tanstack/react-router";
import { useEffect } from "react";
import { useLibraryStats } from "./api/hooks";
import type { LibraryLayout, LibrarySort } from "./api/types";
import { PluginViewPage } from "./plugins/PluginViews";
import { AppShell } from "./shell/AppShell";
import { AppCrash, ViewError } from "./shell/ErrorFallback";
import { type ActivityTab, ActivityView, activityTabs } from "./views/activity/ActivityView";
import { type CollectionFilter, CollectionsView, collectionFilters } from "./views/CollectionsView";
import { CollectionView } from "./views/CollectionView";
import { GameView } from "./views/game/GameView";
import { validReturn } from "./views/game/origin";
import { HomeView } from "./views/home/HomeView";
import { type LibraryFilter, LibraryView } from "./views/library/LibraryView";
import {
  rememberedLibraryView,
  rememberLibraryView,
  resolveLibraryView,
} from "./views/library/remembered";
import { NotFoundView } from "./views/PlaceholderView";
import { type SettingsTab, SettingsView } from "./views/SettingsView";
import { StoreView, validateStoreSearch } from "./views/StoreView";
import { rememberSettingsTab, resolveSettingsTab } from "./views/settings/rememberedTab";
import { StoreGameView } from "./views/store/StoreGameView";

/** The page a game was opened from, for its Back link (#195): only an app
 * page is kept (`views/game/origin.ts`). */
function validateFrom(input: { from?: string } & SearchSchemaInput): { from?: string } {
  const from = validReturn((input as Record<string, unknown>).from);
  return from ? { from } : {};
}

const rootRoute = createRootRoute({
  component: AppShell,
  errorComponent: AppCrash,
  notFoundComponent: NotFoundView,
});

const homeRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: HomeView,
});

const libraryRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/library",
  // Library filters live in the URL (TECH-PLAN §1, Routing). A link may
  // leave out the filter, sort and layout to come back to the remembered ones.
  // `opening` marks a filter no link named, which the user therefore didn't
  // choose here; every link that names one drops it.
  validateSearch: (
    input: {
      filter?: LibraryFilter;
      q?: string;
      sort?: LibrarySort;
      layout?: LibraryLayout;
      platform?: string;
      opening?: boolean;
    } & SearchSchemaInput,
  ): {
    filter: LibraryFilter;
    q?: string;
    sort: LibrarySort;
    layout: LibraryLayout;
    platform?: string;
    opening?: true;
  } => {
    const search = input as Record<string, unknown>;
    const q = typeof search.q === "string" && search.q.trim() ? search.q.trim() : undefined;
    const platform =
      typeof search.platform === "string" && search.platform.trim()
        ? search.platform.trim()
        : undefined;
    const opening = search.filter === undefined || search.opening === true;
    return {
      ...resolveLibraryView(search),
      q,
      ...(platform && { platform }),
      ...(opening && { opening: true as const }),
    };
  },
  component: function Library() {
    const { filter, q, sort, layout, platform, opening } = libraryRoute.useSearch();
    const navigate = libraryRoute.useNavigate();
    const stats = useLibraryStats();
    // With nothing played, the Library opens on every game. Pressing Recently
    // Played names the filter, so that stays.
    const nothingRecent = opening && filter === "recent" && stats.data?.recent === 0;
    useEffect(() => {
      if (nothingRecent)
        void navigate({ search: (search) => ({ ...search, filter: "all" }), replace: true });
    }, [nothingRecent, navigate]);
    // A search is a detour, and so is a view the user didn't choose: neither
    // replaces the remembered filter, so a first game brings Recently Played back.
    useEffect(() => {
      const kept = q || opening ? rememberedLibraryView().filter : filter;
      rememberLibraryView({ filter: kept, sort, layout });
    }, [filter, q, opening, sort, layout]);
    return (
      <LibraryView
        filter={nothingRecent ? "all" : filter}
        query={q}
        sort={sort}
        layout={layout}
        platform={platform}
      />
    );
  },
});

const gameRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/games/$gameId",
  validateSearch: validateFrom,
  component: function Game() {
    const { gameId } = gameRoute.useParams();
    const { from } = gameRoute.useSearch();
    return <GameView gameId={gameId} from={from} />;
  },
});

const activityRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/activity",
  // Jobs or the notification feed (#198), its unread filter and the item
  // whose list is open. A link may leave them out: jobs.
  validateSearch: (
    input: { tab?: ActivityTab; unread?: boolean; open?: string } & SearchSchemaInput,
  ): { tab: ActivityTab; unread?: true; open?: string } => {
    const search = input as Record<string, unknown>;
    const open = typeof search.open === "string" && search.open ? search.open : undefined;
    return {
      tab: activityTabs.find((tab) => tab === search.tab) ?? "jobs",
      ...(search.unread === true && { unread: true as const }),
      ...(open && { open }),
    };
  },
  component: function Activity() {
    const { tab, unread, open } = activityRoute.useSearch();
    return <ActivityView tab={tab} unreadOnly={unread ?? false} open={open} />;
  },
});

const storeRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/store",
  // The tab, search text and filters live in the URL (#163), so Back from
  // a game's page returns to the same results.
  validateSearch: validateStoreSearch,
  component: function Store() {
    return <StoreView search={storeRoute.useSearch()} />;
  },
});

const storeGameRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/store/games/$igdbId",
  // A catalog game's page; `from` as on a library game's page.
  validateSearch: validateFrom,
  component: function StoreGame() {
    const { igdbId } = storeGameRoute.useParams();
    const { from } = storeGameRoute.useSearch();
    return <StoreGameView igdbId={igdbId} from={from} />;
  },
});

const collectionsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/collections",
  validateSearch: (search: Record<string, unknown>): { filter: CollectionFilter } => {
    const filter = collectionFilters.find((f) => f === search.filter);
    return { filter: filter ?? "all" };
  },
  component: function Collections() {
    const { filter } = collectionsRoute.useSearch();
    return <CollectionsView filter={filter} />;
  },
});

const collectionRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/collections/$collectionId",
  component: function CollectionDetail() {
    const { collectionId } = collectionRoute.useParams();
    return <CollectionView collectionId={collectionId} />;
  },
});

const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings",
  // A link may leave out the tab to come back to the remembered one.
  // `section` picks a sub-view inside a tab (e.g. emulators → scanners). Each panel validates it.
  validateSearch: (search: Record<string, unknown>): { tab: SettingsTab; section?: string } => {
    const section = typeof search.section === "string" ? search.section : undefined;
    return { tab: resolveSettingsTab(search), ...(section && { section }) };
  },
  component: function Settings() {
    const { tab } = settingsRoute.useSearch();
    useEffect(() => rememberSettingsTab(tab), [tab]);
    return <SettingsView tab={tab} />;
  },
});

const pluginViewRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/plugins/$pluginId/$viewId",
  component: function PluginView() {
    const { pluginId, viewId } = pluginViewRoute.useParams();
    return <PluginViewPage pluginId={pluginId} viewId={viewId} />;
  },
});

export const routeTree = rootRoute.addChildren([
  homeRoute,
  libraryRoute,
  gameRoute,
  activityRoute,
  storeRoute,
  storeGameRoute,
  collectionsRoute,
  collectionRoute,
  settingsRoute,
  pluginViewRoute,
]);

// A view that throws shows its error in place; the shell stays.
export const router = createRouter({ routeTree, defaultErrorComponent: ViewError });

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
