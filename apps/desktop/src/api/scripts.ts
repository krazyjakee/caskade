/** Lifecycle scripts (#179, #180): the global, emulator profile and game
 * scripts, the order a launch runs them in, and deliberate tests through
 * the launch's own runner. */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import type {
  ContextValue,
  LifecycleScript_Serialize,
  ScriptCommand,
  ScriptPhase,
  ScriptRun_Serialize,
  ScriptScope,
  ScriptStatus,
  ScriptStep_Serialize,
  ScriptTest_Serialize,
  Shell,
} from "./bindings";
import { commands } from "./bindings";
import { libraryKeys, useOptionalLibraryApi } from "./provider";
import { unwrap } from "./reply";

export type { ContextValue, ScriptCommand, ScriptPhase, ScriptScope, ScriptStatus, Shell };
export type LifecycleScript = LifecycleScript_Serialize;
export type ScriptRun = ScriptRun_Serialize;
export type ScriptStep = ScriptStep_Serialize;
export type ScriptTest = ScriptTest_Serialize;

export interface ScriptsApi {
  list(scope: ScriptScope): Promise<LifecycleScript[]>;
  /** Adds (an empty `id`) or changes a script of `scope`. */
  save(scope: ScriptScope, script: LifecycleScript): Promise<LifecycleScript>;
  remove(scope: ScriptScope, id: string): Promise<boolean>;
  order(scope: ScriptScope, ids: string[]): Promise<void>;
  /** The steps a launch of the game (through the profile) takes. */
  effective(gameId: string | null, profileId: string | null): Promise<ScriptStep[]>;
  /** Runs a script once, saved or not, as a launch would; a game script
   * with its game's values, others with no game's. */
  test(
    scope: ScriptScope,
    script: LifecycleScript,
    gameId: string | null,
    testId: string,
  ): Promise<ScriptTest>;
  cancelTest(testId: string): Promise<boolean>;
}

function native() {
  if (!isTauri()) throw new Error("Open Caskade desktop to change scripts.");
}

export const nativeScripts: ScriptsApi = {
  async list(scope) {
    native();
    return unwrap(commands.lifecycleScripts(scope));
  },
  async save(scope, script) {
    native();
    return unwrap(commands.lifecycleScriptSave(scope, script));
  },
  async remove(scope, id) {
    native();
    return unwrap(commands.lifecycleScriptDelete(scope, id));
  },
  async order(scope, ids) {
    native();
    await unwrap(commands.lifecycleScriptsOrder(scope, ids));
  },
  async effective(gameId, profileId) {
    native();
    return unwrap(commands.lifecycleScriptsEffective(gameId, profileId));
  },
  async test(scope, script, gameId, testId) {
    native();
    return unwrap(commands.lifecycleScriptTest(scope, script, gameId, testId));
  },
  async cancelTest(testId) {
    native();
    return unwrap(commands.lifecycleScriptTestCancel(testId));
  },
};

const scopeKey = (scope: ScriptScope) =>
  scope.type === "global"
    ? "global"
    : scope.type === "profile"
      ? `profile:${scope.profileId}`
      : `game:${scope.gameId}`;

const scriptKeys = {
  all: [...libraryKeys.all, "scripts"] as const,
  scope: (scope: ScriptScope) => [...scriptKeys.all, scopeKey(scope)] as const,
  effective: (gameId: string | null, profileId: string | null) =>
    [...scriptKeys.all, "effective", gameId, profileId] as const,
};

/** The library's scripts, or `null` where there are none (or no library:
 * a settings panel rendered alone). */
export function useScriptsApi(): ScriptsApi | null {
  return useOptionalLibraryApi()?.scripts ?? null;
}

export function useScripts(scope: ScriptScope) {
  const api = useScriptsApi();
  return useQuery({
    queryKey: scriptKeys.scope(scope),
    queryFn: () => {
      if (!api) throw new Error("Scripts aren't available here");
      return api.list(scope);
    },
    enabled: Boolean(api),
  });
}

export function useEffectiveScripts(gameId: string | null, profileId: string | null) {
  const api = useScriptsApi();
  return useQuery({
    queryKey: scriptKeys.effective(gameId, profileId),
    queryFn: () => {
      if (!api) throw new Error("Scripts aren't available here");
      return api.effective(gameId, profileId);
    },
    enabled: Boolean(api),
  });
}

/** Changes to one scope's scripts; each refreshes every script list. */
export function useScriptChanges(scope: ScriptScope) {
  const api = useScriptsApi();
  const client = useQueryClient();
  const refresh = () => client.invalidateQueries({ queryKey: scriptKeys.all });
  const need = () => {
    if (!api) throw new Error("Scripts aren't available here");
    return api;
  };
  return {
    save: useMutation({
      mutationFn: (script: LifecycleScript) => need().save(scope, script),
      onSuccess: refresh,
    }),
    remove: useMutation({
      mutationFn: (id: string) => need().remove(scope, id),
      onSuccess: refresh,
    }),
    order: useMutation({
      mutationFn: (ids: string[]) => need().order(scope, ids),
      onSuccess: refresh,
    }),
  };
}

/** A fresh script for `scope`: what the editor starts a new one from. */
export function blankScript(scope: ScriptScope, shell: Shell): LifecycleScript {
  return {
    id: "",
    scope,
    name: "",
    enabled: true,
    phase: "before_launch",
    command: { type: "shell", shell, script: "" },
    cwd: null,
    timeoutSecs: 60,
  };
}
