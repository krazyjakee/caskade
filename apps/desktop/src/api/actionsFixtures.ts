/** Launch actions over the fixture library, with the core's rules (#177):
 * each launchable release's own launch is its built-in play action,
 * editable only on a manual release; names, programs and tracking are
 * checked as the core checks them. Tools record that they ran. */
import {
  type ActionsApi,
  builtInActionId,
  type EntryActionSettings,
  type GameAction,
  type ReleaseActions,
} from "./actions";
import type { GameDetails, GameEntry, LaunchPrompt } from "./types";

const MANUAL = "manual";

export function createFixtureActions(records: GameDetails[]): ActionsApi & {
  /** Tools that ran, by action id. */
  ran: string[];
  /** The prompt a launch of `entryId` asks, or null: Play's ask-at-launch. */
  prompt(entryId: string, action?: string | null): LaunchPrompt | null;
  /** Fails the next save with `message`, as the core would. */
  failNextSave(message: string): void;
} {
  const added = new Map<string, GameAction[]>();
  const settings = new Map<string, EntryActionSettings>();
  // A manual release's own target, once the user changed it.
  const targets = new Map<string, GameAction["target"]>();
  const ran: string[] = [];
  let failure: string | null = null;
  let next = 1;
  const entryOf = (gameId: string, entryId: string): GameEntry => {
    const entry = records
      .find((game) => game.id === gameId)
      ?.entries.find((candidate) => candidate.id === entryId);
    if (!entry) throw new Error(`entry ${entryId} was not found`);
    return entry;
  };
  const settingsOf = (entryId: string): EntryActionSettings =>
    settings.get(entryId) ?? {
      defaultAction: null,
      ask: false,
      builtIn: { name: null, tracking: null },
    };
  const builtIn = (entry: GameEntry): GameAction | null => {
    if (!entry.canLaunch && !targets.has(entry.id)) return null;
    const own = settingsOf(entry.id).builtIn;
    return {
      id: builtInActionId(entry.id),
      entryId: entry.id,
      name: own.name ?? "Play",
      kind: "play",
      target: targets.get(entry.id) ?? {
        type: "executable",
        path: `${entry.location ?? "/games/fixture"}/game`,
        args: [],
        cwd: null,
        env: {},
      },
      tracking: own.tracking ?? { mode: "automatic", folder: null, names: [] },
    };
  };
  const actionsOf = (entry: GameEntry): GameAction[] => [
    ...(builtIn(entry) ? [builtIn(entry) as GameAction] : []),
    ...(added.get(entry.id) ?? []),
  ];
  const check = (action: GameAction) => {
    if (failure) {
      const message = failure;
      failure = null;
      throw new Error(message);
    }
    if (!action.name.trim()) throw new Error("invalid action name");
    const target = action.target;
    if (target.type === "executable" && !target.path.trim())
      throw new Error("choose the program the action starts");
    if (target.type === "uri" && !/^[a-z][a-z0-9+.-]*:/i.test(target.uri))
      throw new Error("launch URI has no scheme");
    if (
      action.kind === "play" &&
      action.tracking.mode === "process_name" &&
      action.tracking.names.length === 0
    )
      throw new Error("add the game's process names (like game.exe) to track it by name");
  };
  const find = (id: string) => {
    for (const [entryId, list] of added) {
      const index = list.findIndex((action) => action.id === id);
      if (index >= 0) return { entryId, list, index };
    }
    return null;
  };
  return {
    ran,
    failNextSave(message) {
      failure = message;
    },
    prompt(entryId, action) {
      if (action) return null;
      const entry = records.flatMap((game) => game.entries).find((each) => each.id === entryId);
      if (!entry || !settingsOf(entryId).ask) return null;
      const play = actionsOf(entry).filter((each) => each.kind === "play");
      if (play.length < 2) return null;
      const chosen = settingsOf(entryId).defaultAction ?? play[0]?.id;
      return {
        entryId,
        platformId: "",
        profiles: [],
        discs: [],
        actions: play.map((each) => ({
          id: each.id,
          name: each.name,
          default: each.id === chosen,
        })),
      };
    },
    async list(gameId) {
      const game = records.find((each) => each.id === gameId);
      return structuredClone(
        (game?.entries ?? []).map(
          (entry): ReleaseActions => ({
            entryId: entry.id,
            editableBuiltIn: entry.source === MANUAL,
            hasFolder: entry.location !== null,
            actions: actionsOf(entry),
            settings: settingsOf(entry.id),
          }),
        ),
      );
    },
    async save(gameId, action) {
      const game = records.find((each) => each.id === gameId);
      if (!game) throw new Error(`game ${gameId} was not found`);
      let entryId = action.entryId;
      if (!entryId) {
        const manual = game.entries.find((entry) => entry.source === MANUAL) ?? game.entries[0];
        if (manual) entryId = manual.id;
        else {
          entryId = `manual-${next++}`;
          game.entries.push({
            id: entryId,
            source: MANUAL,
            state: "installed",
            location: null,
            canLaunch: false,
            emulated: false,
          });
        }
      }
      const entry = entryOf(gameId, entryId);
      const saved = structuredClone({ ...action, entryId, name: action.name.trim() });
      check(saved);
      if (
        saved.id === builtInActionId(entryId) ||
        (!actionsOf(entry).length && saved.kind === "play" && !saved.id)
      ) {
        const current = builtIn(entry);
        if (
          current &&
          JSON.stringify(current.target) !== JSON.stringify(saved.target) &&
          entry.source !== MANUAL
        )
          throw new Error(
            "this release's own launch comes from where it was found, which keeps it current: duplicate it to change it",
          );
        targets.set(entryId, saved.target);
        entry.canLaunch = true;
        settings.set(entryId, {
          ...settingsOf(entryId),
          builtIn: {
            name: saved.name === "Play" ? null : saved.name,
            tracking: saved.tracking.mode === "automatic" ? null : saved.tracking,
          },
        });
        return builtIn(entry) as GameAction;
      }
      const list = added.get(entryId) ?? [];
      if (!saved.id) {
        saved.id = `action-${next++}`;
        list.push(saved);
      } else {
        const at = list.findIndex((each) => each.id === saved.id);
        if (at < 0) throw new Error("that action no longer exists; choose another");
        list[at] = saved;
      }
      added.set(entryId, list);
      const own = settingsOf(entryId);
      if (saved.kind === "utility" && own.defaultAction === saved.id)
        settings.set(entryId, { ...own, defaultAction: null });
      return structuredClone(saved);
    },
    async remove(_gameId, id) {
      if (id.startsWith("source:")) throw new Error("a release's own launch can't be deleted");
      const found = find(id);
      if (!found) return false;
      found.list.splice(found.index, 1);
      const own = settingsOf(found.entryId);
      if (own.defaultAction === id) settings.set(found.entryId, { ...own, defaultAction: null });
      return true;
    },
    async order(_gameId, entryId, ids) {
      const list = added.get(entryId) ?? [];
      if (ids.length !== list.length || !list.every((each) => ids.includes(each.id)))
        throw new Error("the new order must list each of the release's actions once");
      added.set(
        entryId,
        ids.map((id) => list.find((each) => each.id === id) as GameAction),
      );
    },
    async saveSettings(gameId, entryId, wanted) {
      const entry = entryOf(gameId, entryId);
      const play = actionsOf(entry).filter((each) => each.kind === "play");
      if (wanted.defaultAction && !play.some((each) => each.id === wanted.defaultAction))
        throw new Error("the default must be one of the release's play actions");
      const saved = {
        ...wanted,
        defaultAction:
          wanted.defaultAction === builtInActionId(entryId) ? null : (wanted.defaultAction ?? null),
      };
      settings.set(entryId, saved);
      return structuredClone(saved);
    },
    async runTool(entryId, actionId) {
      const tool = (added.get(entryId) ?? []).find((each) => each.id === actionId);
      if (tool?.kind !== "utility") throw new Error("that action no longer exists; choose another");
      ran.push(actionId);
      return { type: "started" };
    },
  };
}
