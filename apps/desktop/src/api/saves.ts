/** Save locations belong to one release on this device (#238). */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import {
  type BeforePlay_Serialize as BeforePlay,
  commands,
  events,
  type LocationSpec,
  type ManifestStatus_Serialize as ManifestStatus,
  type PlayingElsewhere,
  type SaveCheck_Serialize as SaveCheck,
  type SaveHistory_Serialize as SaveHistory,
  type SaveMatch,
  type SavePreferences,
  type SaveLocationsState as SaveState,
  type SaveVersion_Serialize as SaveVersion,
  type VersionReason,
  type VersionState_Serialize as VersionState,
} from "./bindings";
import { useOptionalLibraryApi } from "./provider";
import { unwrap } from "./reply";
import { effectSubscription } from "./subscription";
import { useProvidedSyncApi } from "./sync";

export type {
  BeforePlay,
  LocationSpec,
  ManifestStatus,
  PlayingElsewhere,
  SaveCheck,
  SaveHistory,
  SaveMatch,
  SavePreferences,
  SaveState,
  SaveVersion,
  VersionReason,
  VersionState,
};
export interface SavesApi {
  state(entryId: string): Promise<SaveState>;
  edit(entryId: string, preferences: SavePreferences, revision: number): Promise<SaveState>;
  /** Before a launch (#239): keeps a save changed here, then puts a newer
   * one from another device in place, or says both devices played. Waits
   * for other devices a few seconds at most. */
  beforePlay(entryId: string): Promise<BeforePlay>;
  /** Settles a conflict: the chosen version's files go in place. */
  choose(entryId: string, versionId: string): Promise<SaveVersion>;
  /** A game's save versions (#241), newest first. */
  history(gameId: string): Promise<SaveHistory>;
  /** Puts an older version in place; the save there now is kept first. */
  restore(entryId: string, versionId: string): Promise<SaveVersion>;
  /** Shows one of the release's save locations in the file manager. */
  reveal(entryId: string, location: number): Promise<void>;
  /** Whether the save manifest is cached, and when it was last checked. */
  manifestStatus(): Promise<ManifestStatus>;
  /** Checks the manifest's source now. */
  manifestUpdate(): Promise<ManifestStatus>;
  /** Calls `listener` with a game whose versions changed here. */
  subscribe(listener: (gameId: string) => void): () => void;
  /** Games running on other devices now (#240), from what they last said;
   * never waits on the network. */
  playingElsewhere(): Promise<PlayingElsewhere[]>;
  /** Calls `listener` whenever what's running elsewhere changes. */
  subscribePlaying(listener: (playing: PlayingElsewhere[]) => void): () => void;
}
export const nativeSaves: SavesApi = {
  state: (entryId) => unwrap(commands.gameSavesState(entryId)),
  edit: (entryId, preferences, revision) =>
    unwrap(commands.gameSavesEdit(entryId, preferences, revision)),
  beforePlay: (entryId) => unwrap(commands.gameSavesBeforePlay(entryId)),
  choose: (entryId, versionId) => unwrap(commands.gameSavesChoose(entryId, versionId)),
  history: (gameId) => unwrap(commands.gameSaveHistory(gameId)),
  restore: (entryId, versionId) => unwrap(commands.gameSavesRestore(entryId, versionId)),
  reveal: (entryId, location) =>
    unwrap(commands.gameSavesReveal(entryId, location)).then(() => undefined),
  manifestStatus: () => unwrap(commands.savesManifestStatus()),
  manifestUpdate: () => unwrap(commands.savesManifestUpdate()),
  subscribe: (listener) =>
    effectSubscription(events.saveVersionsChanged.listen, ({ payload }) => listener(payload)),
  playingElsewhere: () => unwrap(commands.devicesPlaying()),
  subscribePlaying: (listener) =>
    effectSubscription(events.playingElsewhereChanged.listen, ({ payload }) => listener(payload)),
};
const key = (entryId: string) => ["save-locations", entryId] as const;
export function useSaveLocations(entryId: string | undefined) {
  const api = useOptionalLibraryApi()?.saves;
  const client = useQueryClient();
  const query = useQuery({
    queryKey: key(entryId ?? ""),
    queryFn: () => {
      if (!api || !entryId) throw new Error("Save locations are unavailable");
      return api.state(entryId);
    },
    enabled: Boolean(api && entryId),
  });
  const edit = useMutation({
    mutationFn: ({ preferences, revision }: { preferences: SavePreferences; revision: number }) => {
      if (!api || !entryId) throw new Error("Save locations are unavailable");
      return api.edit(entryId, preferences, revision);
    },
    onSuccess: (state) => client.setQueryData(key(state.entryId), state),
    onError: () => {
      if (entryId) void client.invalidateQueries({ queryKey: key(entryId) });
    },
  });
  return { query, edit, available: Boolean(api) };
}

const historyKey = (gameId: string) => ["save-versions", gameId] as const;

/** A game's save versions, refreshed when they change here or a sync with
 * another device brings some. */
export function useSaveHistory(gameId: string) {
  const api = useOptionalLibraryApi()?.saves;
  const client = useQueryClient();
  const sync = useProvidedSyncApi();
  useEffect(() => {
    if (!api) return;
    return api.subscribe((changed) => {
      if (changed === gameId) void client.invalidateQueries({ queryKey: historyKey(gameId) });
    });
  }, [api, client, gameId]);
  // A sync with another device may have brought versions.
  useEffect(() => {
    if (!api || !sync) return;
    return sync.subscribe(() => void client.invalidateQueries({ queryKey: historyKey(gameId) }));
  }, [api, sync, client, gameId]);
  const query = useQuery({
    queryKey: historyKey(gameId),
    queryFn: () => {
      if (!api) throw new Error("Saves are unavailable");
      return api.history(gameId);
    },
    enabled: Boolean(api),
  });
  const settled = () => client.invalidateQueries({ queryKey: historyKey(gameId) });
  const restore = useMutation({
    mutationFn: ({ entryId, versionId }: { entryId: string; versionId: string }) => {
      if (!api) throw new Error("Saves are unavailable");
      return api.restore(entryId, versionId);
    },
    onSettled: settled,
  });
  const choose = useMutation({
    mutationFn: ({ entryId, versionId }: { entryId: string; versionId: string }) => {
      if (!api) throw new Error("Saves are unavailable");
      return api.choose(entryId, versionId);
    },
    onSettled: settled,
  });
  return { query, restore, choose, available: Boolean(api) };
}

const manifestKey = ["save-manifest"] as const;

/** The save manifest's state, and Update now. */
export function useSaveManifest() {
  const api = useOptionalLibraryApi()?.saves;
  const client = useQueryClient();
  const status = useQuery({
    queryKey: manifestKey,
    queryFn: () => {
      if (!api) throw new Error("Saves are unavailable");
      return api.manifestStatus();
    },
    enabled: Boolean(api),
  });
  const update = useMutation({
    mutationFn: () => {
      if (!api) throw new Error("Saves are unavailable");
      return api.manifestUpdate();
    },
    onSuccess: (next) => client.setQueryData(manifestKey, next),
  });
  return { status, update, available: Boolean(api) };
}

const playingKey = ["playing-elsewhere"] as const;

/** One subscription per API however many views and cards watch it. */
const playingWatchers = new WeakMap<SavesApi, { count: number; stop: () => void }>();

function useWatchPlaying(api: SavesApi | undefined) {
  const client = useQueryClient();
  const sync = useProvidedSyncApi();
  useEffect(() => {
    if (!api) return;
    let watcher = playingWatchers.get(api);
    if (!watcher) {
      const stopPlaying = api.subscribePlaying((now) => client.setQueryData(playingKey, now));
      // A device going out of reach stops counting.
      const stopSync = sync?.subscribe(
        () => void client.invalidateQueries({ queryKey: playingKey }),
      );
      watcher = {
        count: 0,
        stop: () => {
          stopPlaying();
          stopSync?.();
        },
      };
      playingWatchers.set(api, watcher);
    }
    watcher.count += 1;
    const held = watcher;
    return () => {
      held.count -= 1;
      if (held.count === 0) {
        held.stop();
        playingWatchers.delete(api);
      }
    };
  }, [api, sync, client]);
}

/** Games running on other devices (#240), kept current by the core's
 * changes and device updates; `gameId` narrows it to one game. */
export function usePlayingElsewhere(gameId?: string) {
  const api = useOptionalLibraryApi()?.saves;
  useWatchPlaying(api);
  const query = useQuery({
    queryKey: playingKey,
    queryFn: () => {
      if (!api) throw new Error("Saves are unavailable");
      return api.playingElsewhere();
    },
    enabled: Boolean(api),
    staleTime: 30_000,
  });
  const all = query.data ?? [];
  return gameId === undefined ? all : all.filter((each) => each.gameId === gameId);
}
