/** Fan ports of library games: decompilations, recompilations and source
 * ports from Decomp Buddy's install plans (`caskade_core::ports`). A game
 * page lists its own; installing one is an install job that adds it to the
 * game as another release. */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { createContext, createElement, type ReactNode, useContext } from "react";
import {
  commands,
  type GamePorts_Serialize,
  type Port_Serialize,
  type PortFileHow,
  type PortHere,
  type PortNeeds_Serialize,
} from "./bindings";
import { createPortsFixtureApi } from "./portsFixtures";
import { libraryKeys } from "./provider";
import { unwrap } from "./reply";

export type GamePorts = GamePorts_Serialize;
export type Port = Port_Serialize;
export type PortNeeds = PortNeeds_Serialize;
export type { PortFileHow, PortHere };

export interface PortsApi {
  /** The game's ports, surest first, with the catalogue's state. */
  game(gameId: string): Promise<GamePorts>;
  /** Installs `project` into the game; resolves with its install job's id. */
  install(gameId: string, project: string): Promise<string>;
}

export const nativePortsApi: PortsApi = {
  game: (gameId) => unwrap(commands.gamePorts(gameId)),
  install: async (gameId, project) =>
    (await unwrap(commands.gamePortInstall(gameId, project, null))).id,
};

let preview: PortsApi | undefined;
/** Without the core: the fixture ports in a dev build started with
 * `VITE_CASKADE_FIXTURES=1`, none otherwise. */
function defaultApi(): PortsApi {
  if (isTauri()) return nativePortsApi;
  preview ??= createPortsFixtureApi(
    import.meta.env.DEV && import.meta.env.VITE_CASKADE_FIXTURES === "1" ? undefined : [],
  );
  return preview;
}

const Context = createContext<PortsApi | null>(null);

/** Tests and previews supply their own API; the app uses the native one. */
export function PortsProvider({ api, children }: { api: PortsApi; children: ReactNode }) {
  return createElement(Context.Provider, { value: api }, children);
}

export function usePortsApi(): PortsApi {
  return useContext(Context) ?? defaultApi();
}

/** Under `library`, so an install finishing (a library event) refreshes it. */
export const gamePortsKey = (gameId: string) => [...libraryKeys.all, "ports", gameId];

export function useGamePorts(gameId: string) {
  const api = usePortsApi();
  return useQuery({
    queryKey: gamePortsKey(gameId),
    queryFn: () => api.game(gameId),
    retry: false,
    staleTime: 60_000,
  });
}

export function useInstallPort(gameId: string) {
  const api = usePortsApi();
  const client = useQueryClient();
  return useMutation({
    mutationFn: (project: string) => api.install(gameId, project),
    onSuccess: () => void client.invalidateQueries({ queryKey: gamePortsKey(gameId) }),
  });
}

/** The catalogue's names of systems as people read them. */
export function systemName(system: string): string {
  switch (system) {
    case "windows":
      return "Windows";
    case "linux":
      return "Linux";
    case "macos-arm64":
      return "macOS";
    default:
      return system;
  }
}
