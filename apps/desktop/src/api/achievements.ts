/** A game's achievements (#190) as the core keeps them (#185): one set per
 * store account and service game, each with how its last fetch went. Reads
 * answer the cache; Refresh fetches now, and `achievementsChanged` says when
 * a fetch (the page's or a background one) has changed what is cached.
 * Icons come through the artwork cache by achievement, never by address. */
import { t } from "@lingui/core/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { useEffect } from "react";
import { assetUrl, thumbnailPath } from "./artwork";
import { type AchievementSet_Serialize, commands, events } from "./bindings";
import { injectable } from "./injectable";
import { unwrap } from "./reply";
import { effectSubscription } from "./subscription";

export type AchievementSet = AchievementSet_Serialize;
export type Achievement = AchievementSet["achievements"][number];

/** Which icon of an achievement: its own, or the one shown while locked. */
interface IconRequest {
  gameId: string;
  storeAccountId: string;
  productId: string;
  achievementId: string;
  locked: boolean;
}

export interface AchievementsApi {
  /** The signed-in account's sets for the game, as last fetched. */
  sets(gameId: string): Promise<AchievementSet[]>;
  /** Fetches the game's sets now; a set that can't be fetched keeps its
   * last answer and says why. */
  refresh(gameId: string): Promise<AchievementSet[]>;
  /** Stops the game's refresh under way. */
  cancel(gameId: string): Promise<boolean>;
  /** An achievement's icon as an image URL; null when it has none or it
   * can't load. */
  icon(request: IconRequest): Promise<string | null>;
  /** Calls `listener` with a game whose cached achievements changed. */
  subscribe(listener: (gameId: string) => void): () => void;
}

function createNativeAchievements(): AchievementsApi {
  const native = () => {
    if (!isTauri()) throw new Error(t`Open Caskade desktop to see achievements.`);
  };
  return {
    async sets(gameId) {
      // Without the core (the browser preview) there are none to show.
      if (!isTauri()) return [];
      return unwrap(commands.gameAchievements(gameId));
    },
    async refresh(gameId) {
      native();
      return unwrap(commands.gameAchievementsRefresh(gameId));
    },
    async cancel(gameId) {
      native();
      return unwrap(commands.gameAchievementsCancel(gameId));
    },
    async icon({ gameId, storeAccountId, productId, achievementId, locked }) {
      native();
      const cached = await unwrap(
        commands.gameAchievementIcon(gameId, storeAccountId, productId, achievementId, locked),
      );
      const path = thumbnailPath(cached, 96);
      return path ? assetUrl(path) : null;
    },
    subscribe(listener) {
      if (!isTauri()) return () => {};
      return effectSubscription(events.achievementsChanged.listen, ({ payload }) =>
        listener(payload.gameId),
      );
    },
  };
}

export const {
  Provider: AchievementsProvider,
  useApi: useAchievementsApi,
  preview: previewAchievements,
} = injectable(createNativeAchievements);

const achievementsKey = (gameId: string) => ["achievements", gameId] as const;

/** The game's sets, read again whenever a fetch changes them. */
export function useGameAchievements(gameId: string) {
  const api = useAchievementsApi();
  const client = useQueryClient();
  useEffect(
    () =>
      api.subscribe((changed) => {
        if (changed === gameId)
          void client.invalidateQueries({ queryKey: achievementsKey(gameId) });
      }),
    [api, client, gameId],
  );
  return useQuery({
    queryKey: achievementsKey(gameId),
    queryFn: () => api.sets(gameId),
    // A read of the local cache: retrying won't answer differently.
    retry: false,
  });
}

/** Refreshes the game's sets; the answer replaces what is shown. */
export function useRefreshAchievements(gameId: string) {
  const api = useAchievementsApi();
  const client = useQueryClient();
  return useMutation({
    mutationFn: () => api.refresh(gameId),
    onSuccess: (sets) => client.setQueryData(achievementsKey(gameId), sets),
  });
}

export function useAchievementIcon(request: IconRequest, enabled: boolean) {
  const api = useAchievementsApi();
  return useQuery({
    queryKey: ["achievements", "icon", request] as const,
    queryFn: () => api.icon(request),
    enabled,
    staleTime: Number.POSITIVE_INFINITY,
  });
}

/** A set's identity across fetches: an unfetched set has no id yet. */
export function setKey(set: AchievementSet): string {
  return `${set.account.storeAccountId}/${set.productId}`;
}
