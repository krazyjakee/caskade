import {
  type GameQuery,
  type GamesQuery,
  HostScope,
  type HostServices,
  type PluginGame,
} from "@caskade/sdk";
import { t } from "@lingui/core/macro";
import { createContext, type ReactNode, useContext, useEffect, useState } from "react";
import {
  LIBRARY_PAGE_SIZE,
  useGame as useLibraryGame,
  useGames as useLibraryGames,
} from "../api/hooks";
import { effectSubscription } from "../api/subscription";
import type { GameDetails, GameSummary } from "../api/types";
import { useInputAction } from "../input";
import { useMode } from "../shell/console/mode";
import { noPlugins } from "./api";
import { PluginLoader } from "./loader";
import { PluginRegistry, PluginRegistryContext } from "./registry";
import { exposeSharedModules } from "./shared";
import type { PluginUiApi } from "./types";

const pluginGame = (game: GameSummary & Partial<GameDetails>): PluginGame => ({
  id: game.id,
  title: game.title,
  platform: game.platform,
  status: game.status,
  favorite: game.favorite,
  playtimeSeconds: game.playtimeSeconds,
  lastPlayedAt: game.lastPlayedAt,
  description: game.description ?? "",
  tags: game.tags ?? [],
});

/** The app's data behind the SDK's hooks (`useGame`, `useGames`), and its
 * console mode behind `useInputAction` and `useMode` (#152). */
export const host: HostServices = {
  useInputAction(action, handler, options) {
    useInputAction(action, handler, options);
  },
  useMode() {
    return useMode().mode;
  },
  useGame(gameId): GameQuery {
    const game = useLibraryGame(gameId);
    return {
      data: game.data && pluginGame(game.data),
      isPending: game.isPending,
      error: game.error,
    };
  },
  useGames(options): GamesQuery {
    const games = useLibraryGames({ filter: "all", sort: options?.sort ?? "title" });
    const data = games.data?.pages
      .flatMap((page) => page.items)
      .slice(0, options?.limit ?? LIBRARY_PAGE_SIZE)
      .map(pluginGame);
    return { data, isPending: games.isPending, error: games.error };
  },
};

const PluginApiContext = createContext<PluginUiApi>(noPlugins);
/** The plugin API the provider loaded with (Settings → Plugins uses it). */
export const usePluginApi = () => useContext(PluginApiContext);

/** Loads enabled plugins' UI halves once and provides their contributions. */
export function PluginProvider({
  api,
  library,
  registry: given,
  importModule,
  children,
}: {
  api: PluginUiApi;
  /** Library reads for `ctx.library` (the app's LibraryApi). */
  library?: { getGame(id: string): Promise<GameDetails | null> };
  registry?: PluginRegistry;
  importModule?: ConstructorParameters<typeof PluginLoader>[2];
  children: ReactNode;
}) {
  const [registry] = useState(() => given ?? new PluginRegistry());
  useEffect(() => {
    exposeSharedModules();
    const loader = new PluginLoader(
      api,
      registry,
      importModule,
      globalThis.document,
      async (id) => {
        const game = await library?.getGame(id);
        return game ? pluginGame(game) : null;
      },
    );
    void loader.loadAll().catch((error) => console.error("Plugins failed to load", error));
    // Core turned a crashing backend off (#125): its UI half goes too.
    const stop = effectSubscription(api.subscribe, (event) => {
      if (event.name === "plugin.disabled") {
        const { plugin, reason } = event.payload as { plugin: string; reason: string };
        const name = loader.name(plugin) ?? plugin;
        void loader.unload(plugin);
        registry.notify(
          { id: plugin, name },
          t`${{ plugin: name }} was turned off: ${{ reason }}`,
          "error",
        );
      } else if (event.name === "locale.changed") {
        // Names, titles and the plugins' own catalogs in the new language (#219).
        void loader.reloadAll().catch((error) => console.error("Plugins failed to reload", error));
      } else if (event.name === "plugins.changed") {
        void loader.refresh().catch((error) => console.error("Plugins failed to refresh", error));
      } else if (event.name === "plugin.reloaded") {
        // A dev plugin changed (#131); a broken rebuild keeps what runs now.
        const { plugin, ui, error } = event.payload as {
          plugin: string;
          ui: boolean;
          error: string | null;
        };
        const name = loader.name(plugin) ?? plugin;
        if (error)
          registry.notify(
            { id: plugin, name },
            t`${{ plugin: name }}: rebuild failed: ${{ error }}`,
            "error",
          );
        else if (ui)
          void loader.refresh(plugin).catch((failure) => console.error("Reload failed", failure));
      }
    });
    return () => {
      stop();
      void loader.unloadAll();
    };
  }, [api, library, registry, importModule]);
  return (
    <PluginApiContext.Provider value={api}>
      <HostScope.Provider value={host}>
        <PluginRegistryContext.Provider value={registry}>{children}</PluginRegistryContext.Provider>
      </HostScope.Provider>
    </PluginApiContext.Provider>
  );
}
