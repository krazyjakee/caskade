/** Save locations belong to one release on this device (#238). */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  commands,
  type LocationSpec,
  type SaveMatch,
  type SavePreferences,
  type SaveLocationsState as SaveState,
} from "./bindings";
import { useOptionalLibraryApi } from "./provider";
import { unwrap } from "./reply";

export type { LocationSpec, SaveMatch, SavePreferences, SaveState };
export interface SavesApi {
  state(entryId: string): Promise<SaveState>;
  edit(entryId: string, preferences: SavePreferences, revision: number): Promise<SaveState>;
}
export const nativeSaves: SavesApi = {
  state: (entryId) => unwrap(commands.gameSavesState(entryId)),
  edit: (entryId, preferences, revision) =>
    unwrap(commands.gameSavesEdit(entryId, preferences, revision)),
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
