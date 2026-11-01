/** Games added by hand over the fixture library (#174). */
import { createFixtureGames } from "./fixtures";
import type { DuplicateGame, ManualGamesApi } from "./manualGames";
import type { GameDetails, LibraryEvent } from "./types";

const words = (title: string) =>
  title
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean)
    .map((word) => word.toLowerCase())
    .join(" ");

/** Paths with "missing" in them aren't there; a path ending in `/` is a
 * folder. */
export function createFixtureManualGames(
  records: GameDetails[],
  emit: (event: LibraryEvent) => void,
): ManualGamesApi {
  const targets = new Map<string, string>();
  let added = 0;
  const key = (target: { launch: unknown }) => JSON.stringify(target.launch);
  return {
    async target(input) {
      const value = input.trim();
      if (/^[a-z][a-z0-9+.-]+:/i.test(value) && !/^[a-z]:[\\/]/i.test(value))
        return { title: "", launch: { type: "uri", uri: value }, folder: null, windows: false };
      if (!value.startsWith("/")) throw new Error("choose the game's program, shortcut or URI");
      if (value.endsWith("/")) throw new Error(`${value} is a folder: choose the game's program`);
      if (value.includes("missing")) throw new Error(`${value} isn't there`);
      const name = value.split("/").at(-1) ?? value;
      const stem = name.replace(/\.[^.]+$/, "");
      const shortcut = /\.(desktop|lnk|url)$/i.test(name);
      const folder = value.slice(0, value.length - name.length - 1) || "/";
      return {
        title: stem,
        launch: {
          type: "executable",
          path: shortcut ? `${folder}/${stem}/${stem}` : value,
          args: shortcut ? ["--windowed"] : [],
          cwd: shortcut ? `${folder}/${stem}` : null,
          env: {},
        },
        folder: shortcut ? `${folder}/${stem}` : folder,
        windows: /\.(exe|bat)$/i.test(name),
      };
    },
    async duplicates(title, target) {
      const wanted = words(title);
      return records.flatMap((game): DuplicateGame[] => {
        if (target && targets.get(key(target)) === game.id)
          return [{ gameId: game.id, title: game.title, reason: "sameTarget" as const }];
        return wanted && words(game.title) === wanted
          ? [{ gameId: game.id, title: game.title, reason: "sameTitle" as const }]
          : [];
      });
    },
    async add(game) {
      const entry = (gameId: string) => ({
        id: `manual-entry-${++added}`,
        source: "manual",
        state: "installed",
        location: game.target?.folder ?? null,
        canLaunch: true,
        emulated: false,
        gameId,
      });
      if (game.linkTo) {
        const existing = records.find((record) => record.id === game.linkTo);
        if (!existing || !game.target)
          throw new Error("only a game with something to start joins another");
        const { gameId: _, ...linked } = entry(existing.id);
        existing.entries.push(linked);
        targets.set(key(game.target), existing.id);
        emit({ type: "library.changed" });
        return existing.id;
      }
      const title = game.title.trim();
      if (!title) throw new Error("invalid game title");
      const [template] = createFixtureGames(1);
      if (!template) throw new Error("No fixture template");
      const id = `fixture-manual-${++added}`;
      const { gameId: _, ...first } = entry(id);
      records.push({
        ...template,
        id,
        title,
        status: game.target ? "installed" : "not-installed",
        favorite: false,
        playtimeSeconds: 0,
        tags: [],
        description: "",
        entries: game.target ? [first] : [],
      });
      if (game.target) targets.set(key(game.target), id);
      emit({ type: "library.changed" });
      return id;
    },
  };
}
