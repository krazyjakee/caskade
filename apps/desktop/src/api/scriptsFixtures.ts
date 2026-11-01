/** Lifecycle scripts over the fixture library, with the core's rules
 * (#179) and a pretend runner for tests (#180): `pwsh` and programs named
 * `missing…` aren't installed, a script with `sleep` runs until cancelled,
 * `exit N` fails with N, and anything else prints what it is. */
import type {
  ContextValue,
  LifecycleScript,
  ScriptPhase,
  ScriptRun,
  ScriptScope,
  ScriptStep,
  ScriptsApi,
} from "./scripts";

const key = (scope: ScriptScope) =>
  scope.type === "global"
    ? "global"
    : scope.type === "profile"
      ? `profile:${scope.profileId}`
      : `game:${scope.gameId}`;
const same = (a: ScriptScope, b: ScriptScope) => key(a) === key(b);
const PHASES: ScriptPhase[] = ["before_launch", "after_start", "after_exit"];

export function createFixtureScripts(
  gameTitle: (gameId: string) => string | undefined,
): ScriptsApi & { tests: string[] } {
  const scopes = new Map<string, LifecycleScript[]>();
  const running = new Map<string, () => void>();
  const tests: string[] = [];
  let next = 1;
  const list = (scope: ScriptScope) => scopes.get(key(scope)) ?? [];
  const check = (script: LifecycleScript) => {
    if (!script.name.trim()) throw new Error("invalid script name");
    if (script.timeoutSecs < 1 || script.timeoutSecs > 3600)
      throw new Error("the time limit must be 1 to 3600 seconds");
    if (script.command.type === "shell" && !script.command.script.trim())
      throw new Error("the script is empty");
    if (script.command.type === "direct" && !script.command.program.trim())
      throw new Error("choose the program the script runs");
  };
  const values = (phase: ScriptPhase, gameId: string | null): ContextValue[] => [
    { placeholder: "{Phase}", variable: "CASKADE_PHASE", value: phase },
    { placeholder: "{GameId}", variable: "CASKADE_GAME_ID", value: gameId ?? "" },
    {
      placeholder: "{GameTitle}",
      variable: "CASKADE_GAME_TITLE",
      value: (gameId && gameTitle(gameId)) ?? "",
    },
    { placeholder: "{Test}", variable: "CASKADE_TEST", value: "1" },
  ];
  const run = (script: LifecycleScript, testId: string): Promise<ScriptRun> => {
    const base: ScriptRun = {
      scriptId: script.id || "test",
      name: script.name,
      scope: script.scope,
      phase: script.phase,
      status: "succeeded",
      exitCode: 0,
      stdout: "",
      stderr: "",
      truncated: false,
      durationMs: 5,
      error: null,
    };
    const command = script.command;
    const program = command.type === "shell" ? command.shell : command.program;
    if (program === "pwsh" || program.startsWith("missing"))
      return Promise.resolve({
        ...base,
        status: "not_started",
        exitCode: null,
        error: `“${program}” wasn't found; check that it is installed and the path is right`,
      });
    const text = command.type === "shell" ? command.script : command.args.join(" ");
    if (text.includes("sleep"))
      return new Promise((resolve) =>
        running.set(testId, () =>
          resolve({ ...base, status: "cancelled", exitCode: null, durationMs: 1000 }),
        ),
      );
    const exit = /exit (\d+)/.exec(text);
    if (exit && exit[1] !== "0")
      return Promise.resolve({
        ...base,
        status: "failed",
        exitCode: Number(exit[1]),
        stderr: "it failed\n",
      });
    return Promise.resolve({ ...base, stdout: `ran ${script.name}\n` });
  };
  return {
    tests,
    async list(scope) {
      return structuredClone(list(scope));
    },
    async save(scope, script) {
      if (!same(scope, script.scope)) throw new Error("the script belongs to another scope");
      const saved = structuredClone({ ...script, name: script.name.trim() });
      check(saved);
      const scripts = list(scope);
      if (!saved.id) {
        saved.id = `script-${next++}`;
        scripts.push(saved);
      } else {
        const at = scripts.findIndex((each) => each.id === saved.id);
        if (at < 0) scripts.push(saved);
        else scripts[at] = saved;
      }
      scopes.set(key(scope), scripts);
      return structuredClone(saved);
    },
    async remove(scope, id) {
      const scripts = list(scope);
      const at = scripts.findIndex((each) => each.id === id);
      if (at < 0) return false;
      scripts.splice(at, 1);
      return true;
    },
    async order(scope, ids) {
      const scripts = list(scope);
      if (ids.length !== scripts.length || !scripts.every((each) => ids.includes(each.id)))
        throw new Error("the new order must list each of the scope's scripts once");
      scopes.set(
        key(scope),
        ids.map((id) => scripts.find((each) => each.id === id) as LifecycleScript),
      );
    },
    async effective(gameId, profileId) {
      const all = [
        list({ type: "global" }),
        profileId ? list({ type: "profile", profileId }) : [],
        gameId ? list({ type: "game", gameId }) : [],
      ];
      return PHASES.flatMap((phase) =>
        (phase === "after_exit" ? [...all].reverse() : all)
          .flat()
          .filter((script) => script.phase === phase)
          .map((script): ScriptStep => ({ phase, script: structuredClone(script) })),
      );
    },
    async test(scope, script, gameId, testId) {
      check(script);
      tests.push(script.name);
      const game = scope.type === "game" ? scope.gameId : gameId;
      const result = await run(script, testId);
      running.delete(testId);
      return { context: values(script.phase, game), run: result };
    },
    async cancelTest(testId) {
      const stop = running.get(testId);
      stop?.();
      return Boolean(stop);
    },
  };
}
