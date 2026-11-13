/** Games added by hand (#174): a title-only record, or a program, shortcut
 * or URI Caskade starts where it is. Nothing is copied or installed. */
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import {
  commands,
  type DuplicateGame,
  type ManualGame_Deserialize,
  type ManualTarget_Serialize,
} from "./bindings";
import { libraryKeys, useOptionalLibraryApi } from "./provider";
import { unwrap } from "./reply";

export type { DuplicateGame };
/** What a program, shortcut or URI starts, and the title it suggests. */
export type ManualTarget = ManualTarget_Serialize;
type ManualGame = ManualGame_Deserialize;

export interface ManualGamesApi {
  /** Refused, with the reason, when it can't start a game. */
  target(input: string): Promise<ManualTarget>;
  /** Games in the library that may be the one being added. */
  duplicates(title: string, target: ManualTarget | null): Promise<DuplicateGame[]>;
  /** Resolves to the game's id (the linked game's, when linked). */
  add(game: ManualGame): Promise<string>;
}

const desktop = () => {
  if (!isTauri()) throw new Error("Open Caskade desktop to add games.");
};

export const nativeManualGames: ManualGamesApi = {
  async target(input) {
    desktop();
    return unwrap(commands.manualGameTarget(input));
  },
  async duplicates(title, target) {
    desktop();
    return unwrap(commands.manualGameDuplicates(title, target));
  },
  async add(game) {
    desktop();
    return unwrap(commands.manualGameAdd(game));
  },
};

/** Adding games by hand, or `null` where the library can't. */
export function useManualGamesApi(): ManualGamesApi | null {
  return useOptionalLibraryApi()?.manual ?? null;
}

export function useAddManualGame() {
  const api = useManualGamesApi();
  const client = useQueryClient();
  return useMutation({
    mutationFn: (game: ManualGame) => {
      if (!api) throw new Error("Adding games by hand isn't available here");
      return api.add(game);
    },
    onSuccess: () => client.invalidateQueries({ queryKey: libraryKeys.all }),
  });
}
