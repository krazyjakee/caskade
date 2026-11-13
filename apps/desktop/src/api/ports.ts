/** Fan ports of library games: decompilations, recompilations and source
 * ports from Decomp Buddy's install plans (`caskade_core::ports`). A game
 * page lists its own; installing one is an install job that adds it to the
 * game as another release. */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  commands,
  type GamePorts_Serialize,
  type Port_Serialize,
  type PortFileHow,
  type PortHere,
  type PortNeeds_Serialize,
} from "./bindings";
import { injectable } from "./injectable";
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

const nativePortsApi: PortsApi = {
  game: (gameId) => unwrap(commands.gamePorts(gameId)),
  install: async (gameId, project) =>
    (await unwrap(commands.gamePortInstall(gameId, project, null))).id,
};

export const {
  Provider: PortsProvider,
  useApi: usePortsApi,
  preview: previewPorts,
} = injectable(() => nativePortsApi);

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
