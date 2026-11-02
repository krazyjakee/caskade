/** Save locations belong to one release on this device (#238). */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  type BeforePlay_Serialize as BeforePlay,
  commands,
  type LocationSpec,
  type SaveCheck_Serialize as SaveCheck,
  type SaveMatch,
  type SavePreferences,
  type SaveLocationsState as SaveState,
  type SaveVersion_Serialize as SaveVersion,
  type VersionReason,
} from "./bindings";
import { useOptionalLibraryApi } from "./provider";
import { unwrap } from "./reply";

export type {
  BeforePlay,
  LocationSpec,
  SaveCheck,
  SaveMatch,
  SavePreferences,
  SaveState,
  SaveVersion,
  VersionReason,
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
}
export const nativeSaves: SavesApi = {
  state: (entryId) => unwrap(commands.gameSavesState(entryId)),
  edit: (entryId, preferences, revision) =>
    unwrap(commands.gameSavesEdit(entryId, preferences, revision)),
  beforePlay: (entryId) => unwrap(commands.gameSavesBeforePlay(entryId)),
  choose: (entryId, versionId) => unwrap(commands.gameSavesChoose(entryId, versionId)),
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
