/** Game updates: newer releases of games installed from GitHub, which the
 * core finds without an account and checks in the background and before
 * a game is played. Updating is an install job (`kind: "update"`) that
 * keeps the player's files. */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { commands, type GameUpdate_Serialize } from "./bindings";
import { injectable } from "./injectable";
import { libraryKeys } from "./provider";
import { unwrap } from "./reply";

export type GameUpdate = GameUpdate_Serialize;

export interface GameUpdatesApi {
  /** The game's followed installs; empty when none is. */
  game(gameId: string): Promise<GameUpdate[]>;
  /** Asks for the newest release now. */
  check(entryId: string): Promise<GameUpdate>;
  /** The update to offer before playing, after a fresh check; `null` to play. */
  beforePlay(entryId: string): Promise<GameUpdate | null>;
  /** Stops offering `version`; `null` offers every release again. */
  skip(entryId: string, version: string | null): Promise<GameUpdate>;
  setPrereleases(entryId: string, prereleases: boolean): Promise<GameUpdate>;
  /** Starts the update; resolves with its install job's id. */
  install(entryId: string): Promise<string>;
}

function native<T>(request: () => Promise<T>): Promise<T> {
  if (!isTauri()) return Promise.reject(new Error("Open Caskade desktop to check for updates."));
  return request();
}

const nativeGameUpdatesApi: GameUpdatesApi = {
  game: (gameId) => native(() => unwrap(commands.gameUpdates(gameId))),
  check: (entryId) => native(() => unwrap(commands.gameUpdateCheck(entryId))),
  beforePlay: (entryId) => native(() => unwrap(commands.gameUpdateBeforePlay(entryId))),
  skip: (entryId, version) => native(() => unwrap(commands.gameUpdateSkip(entryId, version))),
  setPrereleases: (entryId, prereleases) =>
    native(() => unwrap(commands.gameUpdatePrereleases(entryId, prereleases))),
  install: (entryId) => native(async () => (await unwrap(commands.gameUpdateInstall(entryId))).id),
};

export const { Provider: GameUpdatesProvider, useApi: useGameUpdatesApi } = injectable(
  () => nativeGameUpdatesApi,
);

/** Under `library`, so the core's library events (a check that found a
 * release, an update that finished) refresh it. */
export const gameUpdatesKey = (gameId: string) => [...libraryKeys.all, "updates", gameId];

/** A local read: without the core (the browser preview) there are none. */
export function useGameUpdates(gameId: string) {
  const api = useGameUpdatesApi();
  return useQuery({
    queryKey: gameUpdatesKey(gameId),
    queryFn: () => api.game(gameId),
    retry: false,
  });
}

type GameUpdateChange =
  | { type: "check" }
  | { type: "skip"; version: string | null }
  | { type: "prereleases"; prereleases: boolean };

export function useChangeGameUpdate(gameId: string, entryId: string) {
  const api = useGameUpdatesApi();
  const client = useQueryClient();
  return useMutation({
    mutationFn: (change: GameUpdateChange) =>
      change.type === "check"
        ? api.check(entryId)
        : change.type === "skip"
          ? api.skip(entryId, change.version)
          : api.setPrereleases(entryId, change.prereleases),
    onSuccess: (update) =>
      client.setQueryData<GameUpdate[]>(gameUpdatesKey(gameId), (current) =>
        current?.map((each) => (each.entryId === update.entryId ? update : each)),
      ),
  });
}

/** The release name as people read it: `v1.2` stays, `1.2` gets its `v`. */
export function formatVersion(version: string) {
  return /^\d/.test(version) ? `v${version}` : version;
}
