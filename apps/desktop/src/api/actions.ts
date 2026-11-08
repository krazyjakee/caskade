/** Launch actions (#177, #178): a release's own launch (its built-in play
 * action) and the play and tool actions the user adds, each release's
 * default or ask-at-launch choice, and running a tool. The core checks
 * every target; the webview never sends a command line. */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import type {
  ActionKind,
  ActionOption,
  ActionTracking_Serialize,
  EntryActionSettings_Serialize,
  GameAction_Serialize,
  LaunchSpec_Serialize,
  ProtonBuilds_Serialize,
  ReleaseActions_Serialize,
  ToolOutcome_Serialize,
  TrackingMode,
} from "./bindings";
import { commands } from "./bindings";
import { libraryKeys, useLibraryApi } from "./provider";
import { unwrap } from "./reply";

export type { ActionKind, ActionOption, TrackingMode };
export type GameAction = GameAction_Serialize;
export type ActionTarget = LaunchSpec_Serialize;
export type ActionTracking = ActionTracking_Serialize;
export type EntryActionSettings = EntryActionSettings_Serialize;
/** One release with its actions: its built-in action first. */
export type ReleaseActions = ReleaseActions_Serialize;
/** A tool started, or the age limit asks for an admin's PIN first. */
export type ToolOutcome = ToolOutcome_Serialize;
/** The Proton builds on this device an action can choose, best first, and
 * the one an action without its own runs now. */
export type ProtonBuilds = ProtonBuilds_Serialize;

/** The built-in action's id: the release's own launch. */
export const builtInActionId = (entryId: string) => `source:${entryId}`;
export const isBuiltIn = (action: Pick<GameAction, "id" | "entryId">) =>
  action.id === builtInActionId(action.entryId);

export interface ActionsApi {
  list(gameId: string): Promise<ReleaseActions[]>;
  /** Adds (an empty `id`) or changes an action; an empty `entryId` puts it
   * on the game's manual release. Rejects with the reason a target or its
   * tracking can't work. */
  save(gameId: string, action: GameAction): Promise<GameAction>;
  remove(gameId: string, id: string): Promise<boolean>;
  /** Puts a release's added actions in the order of `ids`. */
  order(gameId: string, entryId: string, ids: string[]): Promise<void>;
  saveSettings(
    gameId: string,
    entryId: string,
    settings: EntryActionSettings,
  ): Promise<EntryActionSettings>;
  /** Starts a tool: no play session, playtime or last played. */
  runTool(entryId: string, actionId: string): Promise<ToolOutcome>;
  protonBuilds(): Promise<ProtonBuilds>;
}

function native() {
  if (!isTauri()) throw new Error("Open Caskade desktop to change launch actions.");
}

export const nativeActions: ActionsApi = {
  async list(gameId) {
    native();
    return unwrap(commands.gameActions(gameId));
  },
  async save(gameId, action) {
    native();
    return unwrap(commands.gameActionSave(gameId, action));
  },
  async remove(gameId, id) {
    native();
    return unwrap(commands.gameActionDelete(gameId, id));
  },
  async order(gameId, entryId, ids) {
    native();
    await unwrap(commands.gameActionsOrder(gameId, entryId, ids));
  },
  async saveSettings(gameId, entryId, settings) {
    native();
    return unwrap(commands.gameActionSettingsSave(gameId, entryId, settings));
  },
  async runTool(entryId, actionId) {
    native();
    return unwrap(commands.gameToolRun(entryId, actionId));
  },
  async protonBuilds() {
    native();
    return unwrap(commands.protonBuilds());
  },
};

export const actionKeys = {
  /** Under the library's keys, so library changes refresh them. */
  game: (gameId: string) => [...libraryKeys.all, "actions", gameId] as const,
  proton: ["actions", "proton"] as const,
};

/** The library's launch actions, or `null` where there are none. */
export function useActionsApi(): ActionsApi | null {
  return useLibraryApi().actions ?? null;
}

export function useGameActions(gameId: string) {
  const api = useActionsApi();
  return useQuery({
    queryKey: actionKeys.game(gameId),
    queryFn: () => {
      if (!api) throw new Error("Launch actions aren't available here");
      return api.list(gameId);
    },
    enabled: Boolean(api),
  });
}

/** The Proton builds an action can choose, once a release runs Proton. */
export function useProtonBuilds(enabled: boolean) {
  const api = useActionsApi();
  return useQuery({
    queryKey: actionKeys.proton,
    queryFn: () => {
      if (!api) throw new Error("Launch actions aren't available here");
      return api.protonBuilds();
    },
    enabled: enabled && Boolean(api),
  });
}

/** Whether `action` starts a Windows program, which runs through Proton
 * when its release does: its built-in action, or a program like `.exe`. */
export function runsWindowsProgram(action: GameAction) {
  if (isBuiltIn(action)) return true;
  return (
    action.target.type === "executable" &&
    /\.(exe|bat|cmd|com|lnk|msi)$/i.test(action.target.path.trim())
  );
}

/** Changes to a game's actions; each refreshes its list. */
export function useActionChanges(gameId: string) {
  const api = useActionsApi();
  const client = useQueryClient();
  const refresh = () => client.invalidateQueries({ queryKey: actionKeys.game(gameId) });
  const need = () => {
    if (!api) throw new Error("Launch actions aren't available here");
    return api;
  };
  return {
    save: useMutation({
      mutationFn: (action: GameAction) => need().save(gameId, action),
      onSuccess: refresh,
    }),
    remove: useMutation({
      mutationFn: (id: string) => need().remove(gameId, id),
      onSuccess: refresh,
    }),
    order: useMutation({
      mutationFn: ({ entryId, ids }: { entryId: string; ids: string[] }) =>
        need().order(gameId, entryId, ids),
      onSuccess: refresh,
    }),
    settings: useMutation({
      mutationFn: ({ entryId, settings }: { entryId: string; settings: EntryActionSettings }) =>
        need().saveSettings(gameId, entryId, settings),
      onSuccess: refresh,
    }),
  };
}

/** Runs one of a release's tools. */
export function useRunTool() {
  const api = useActionsApi();
  return useMutation({
    mutationFn: ({ entryId, actionId }: { entryId: string; actionId: string }) => {
      if (!api) throw new Error("Launch actions aren't available here");
      return api.runTool(entryId, actionId);
    },
  });
}

/** A fresh action for a release: what the editor starts a new one from. */
export function blankAction(entryId: string, kind: ActionKind): GameAction {
  return {
    id: "",
    entryId,
    name: "",
    kind,
    target: { type: "executable", path: "", args: [], cwd: null, env: {} },
    tracking: { mode: "automatic", folder: null, names: [] },
    proton: null,
  };
}
