/** An emulated game's emulator settings and readiness (#122). */
import { useMutation, useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { createContext, createElement, type ReactNode, useContext } from "react";
import {
  type ArgsOverride_Deserialize,
  commands,
  type EmulatorTarget,
  type GameEmulation_Serialize,
} from "./bindings";
import { libraryKeys } from "./provider";
import { unwrap } from "./reply";

export type GameEmulation = GameEmulation_Serialize;
export type GameArgs = ArgsOverride_Deserialize;
export type { EmulatorTarget };

export interface GameEmulationApi {
  gameEmulation(entryId: string): Promise<GameEmulation>;
  setGameEmulation(entryId: string, target: EmulatorTarget, args: GameArgs): Promise<GameEmulation>;
  /** Shell-word splitting happens in the backend, never in the UI. */
  splitArgs(text: string): Promise<string[]>;
}

export function createNativeGameEmulationApi(): GameEmulationApi {
  const native = () => {
    if (!isTauri()) throw new Error("Open Caskade desktop to see emulator settings.");
  };
  return {
    async gameEmulation(entryId) {
      native();
      return unwrap(commands.emulationGame(entryId));
    },
    async setGameEmulation(entryId, target, args) {
      native();
      return unwrap(commands.emulationGameSet(entryId, target, args));
    },
    async splitArgs(text) {
      native();
      return unwrap(commands.argsSplit(text));
    },
  };
}

const Context = createContext<GameEmulationApi | null>(null);
let nativeApi: GameEmulationApi | undefined;

/** Tests and previews supply their own API; the app uses the native one. */
export function GameEmulationProvider({
  api,
  children,
}: {
  api: GameEmulationApi;
  children: ReactNode;
}) {
  return createElement(Context.Provider, { value: api }, children);
}

function useApi() {
  const api = useContext(Context);
  if (api) return api;
  nativeApi ??= createNativeGameEmulationApi();
  return nativeApi;
}

/** Under `library` so library changes (and launches) refresh it. */
export const gameEmulationKey = (entryId: string) => [...libraryKeys.all, "emulation", entryId];

export function useGameEmulation(entryId: string, enabled = true) {
  const api = useApi();
  return useQuery({
    queryKey: gameEmulationKey(entryId),
    queryFn: () => api.gameEmulation(entryId),
    enabled,
  });
}

/** Several entries' settings at once, as `useGameEmulation` each. */
export function useGameEmulations(entryIds: readonly string[]) {
  const api = useApi();
  return useQueries({
    queries: entryIds.map((entryId) => ({
      queryKey: gameEmulationKey(entryId),
      queryFn: () => api.gameEmulation(entryId),
    })),
  });
}

export function useSetGameEmulation(entryId: string) {
  const api = useApi();
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ target, args }: { target: EmulatorTarget; args: GameArgs }) =>
      api.setGameEmulation(entryId, target, args),
    onSuccess: (game) => client.setQueryData(gameEmulationKey(entryId), game),
  });
}

export function useSplitArgs() {
  const api = useApi();
  return useMutation({ mutationFn: (text: string) => api.splitArgs(text) });
}
