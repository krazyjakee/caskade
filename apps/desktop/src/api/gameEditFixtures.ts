/** Game edits over the fixture library: the core's rules (#171) for the
 * browser preview and tests, close enough to exercise the editor. */
import type {
  ArtworkImage,
  ArtworkSlot,
  BulkGameResult,
  EditableArtworkKind,
  EditableGame,
  FieldError,
  GameEdit,
  GameEditApi,
  GameEditState,
  GameField,
  ListEdit,
  RefreshStatus,
  TextEdit,
} from "./gameEdit";
import type { GameDetails, LibraryEvent, LibraryQuery } from "./types";

const FIELDS: GameField[] = [
  "title",
  "sortTitle",
  "description",
  "aliases",
  "genres",
  "tags",
  "releaseDate",
  "platforms",
  "rating",
  "notes",
];
/** Fields automatic sources fill; rating and notes are the user's alone. */
const AUTOMATIC = new Set<GameField>(FIELDS.filter((f) => f !== "rating" && f !== "notes"));

interface Layers {
  game: EditableGame;
  /** The automatic value of each field the user set. */
  overrides: Partial<Record<GameField, unknown>>;
}

function fromDetails(record: GameDetails): EditableGame {
  return {
    id: record.id,
    title: record.title,
    sortTitle: record.title,
    description: record.description,
    aliases: [],
    genres: record.genre ? [record.genre] : [],
    tags: [...record.tags],
    releaseDate: record.releaseDate,
    platforms: record.platform ? record.platform.split(" · ") : [],
    favorite: record.favorite,
    hidden: false,
    rating: null,
    notes: "",
    defaultEntry: record.defaultEntryId ?? null,
  };
}

const clean = (values: string[]) => [
  ...new Set(values.map((value) => value.trim()).filter(Boolean)),
];

const validDate = (date: string) => {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date);
  if (!match) return false;
  const [year, month, day] = [Number(match[1]), Number(match[2]), Number(match[3])];
  const parsed = new Date(Date.UTC(year, month - 1, day));
  return year > 0 && parsed.getUTCMonth() === month - 1 && parsed.getUTCDate() === day;
};

function text(edit: TextEdit, automatic: string, trim: boolean): [string, boolean] {
  return edit.op === "reset" ? [automatic, true] : [trim ? edit.value.trim() : edit.value, false];
}

function list(edit: ListEdit, current: string[], automatic: string[]): [string[], boolean] {
  switch (edit.op) {
    case "set":
      return [clean(edit.values), false];
    case "add":
      return [[...current, ...clean(edit.values).filter((v) => !current.includes(v))], false];
    case "remove": {
      const gone = clean(edit.values);
      return [current.filter((v) => !gone.includes(v)), false];
    }
    case "reset":
      return [[...automatic], true];
  }
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

const ARTWORK: EditableArtworkKind[] = ["cover", "hero", "icon"];
const IMAGE = /\.(png|jpe?g|webp)$/i;

interface ArtworkLayers {
  automatic: Record<EditableArtworkKind, ArtworkImage[]>;
  /** Kinds the user controls, with their image (null: none). */
  chosen: Partial<Record<EditableArtworkKind, ArtworkImage | null>>;
}

function automaticArtwork(record: GameDetails): ArtworkLayers["automatic"] {
  const one = (kind: EditableArtworkKind, url: string | null) =>
    url ? [{ id: `${record.id}-${kind}`, source: "fixture", url, width: 600, height: 900 }] : [];
  return {
    cover: one("cover", record.coverGrid),
    hero: one("hero", record.hero),
    icon: one("icon", record.icon),
  };
}

/** `records` are the fixture library's games: a saved edit shows in them
 * and `emit` tells the views. */
export function createFixtureGameEdits(
  records: GameDetails[],
  emit: (event: LibraryEvent) => void,
  /** The library's games for a query, in its order. */
  matching: (query: LibraryQuery) => GameDetails[] = () => records,
): GameEditApi & {
  /** An automatic refresh: changes the layer beneath the user's fields. */
  refresh(gameId: string, update: (game: EditableGame) => void): void;
  addAutomaticArtwork(gameId: string, kind: EditableArtworkKind, image: ArtworkImage): void;
  /** Ends the running refresh, as the core would after its last batch. */
  finishRefresh(): void;
} {
  const layers = new Map<string, Layers>();
  const art = new Map<string, ArtworkLayers>();
  let imports = 0;
  const artOf = (gameId: string): ArtworkLayers | null => {
    const known = art.get(gameId);
    if (known) return known;
    const record = records.find((g) => g.id === gameId);
    if (!record) return null;
    const created = { automatic: automaticArtwork(record), chosen: {} };
    art.set(gameId, created);
    return created;
  };
  const slots = (a: ArtworkLayers): ArtworkSlot[] =>
    structuredClone(
      ARTWORK.map((kind) => {
        const chosen = a.chosen[kind];
        return {
          kind,
          choice: chosen === undefined ? "automatic" : chosen ? "chosen" : "removed",
          shown: chosen === undefined ? (a.automatic[kind][0] ?? null) : chosen,
          automatic: a.automatic[kind],
        };
      }),
    );
  const showArt = (gameId: string, a: ArtworkLayers) => {
    const record = records.find((g) => g.id === gameId);
    if (!record) return;
    const shown = (kind: EditableArtworkKind) =>
      slots(a).find((slot) => slot.kind === kind)?.shown?.url ?? null;
    record.coverGrid = shown("cover");
    record.coverList = shown("cover");
    record.hero = shown("hero");
    record.icon = shown("icon");
    emit({ type: "library.changed" });
  };
  const layersOf = (gameId: string): Layers | null => {
    const known = layers.get(gameId);
    if (known) return known;
    const record = records.find((g) => g.id === gameId);
    if (!record) return null;
    const created = { game: fromDetails(record), overrides: {} };
    layers.set(gameId, created);
    return created;
  };
  const automaticOf = ({ game, overrides }: Layers): EditableGame => ({
    ...game,
    ...(overrides as Partial<EditableGame>),
  });
  const stateOf = (l: Layers): GameEditState =>
    structuredClone({
      game: l.game,
      automatic: automaticOf(l),
      manual: FIELDS.filter((field) => field in l.overrides),
    });
  let quiet = false;
  let refresh: RefreshStatus | null = null;
  let pendingRefresh: (() => void) | null = null;
  let cancelled = false;
  const refreshListeners = new Set<(status: RefreshStatus) => void>();
  const publish = () => {
    if (!refresh) return;
    for (const listener of refreshListeners) listener(structuredClone(refresh));
  };
  const show = (gameId: string, game: EditableGame) => {
    const record = records.find((g) => g.id === gameId);
    if (!record) return;
    record.title = game.title;
    record.description = game.description;
    record.tags = [...game.tags];
    record.genre = game.genres.join(", ");
    record.releaseDate = game.releaseDate;
    record.platform = game.platforms.join(" · ");
    if (!quiet) emit({ type: "library.changed" });
  };
  const api: ReturnType<typeof createFixtureGameEdits> = {
    async state(gameId) {
      const l = layersOf(gameId);
      return l ? stateOf(l) : null;
    },
    async save(gameId, edit: GameEdit, base) {
      const l = layersOf(gameId);
      if (!l) throw new Error(`game ${gameId} was not found`);
      const automatic = automaticOf(l);
      const next: EditableGame = structuredClone(l.game);
      const errors: FieldError[] = [];
      const resets = new Set<GameField>();
      const combined = new Set<GameField>();
      const changes: GameField[] = [];
      const invalid = (field: GameField, message: string) =>
        errors.push({ field, problem: "invalid", message });
      if (edit.title) {
        const [value, reset] = text(edit.title, automatic.title, true);
        next.title = value;
        if (reset) resets.add("title");
        changes.push("title");
        if (!value) invalid("title", "invalid game title");
      }
      if (edit.sortTitle) {
        const [value, reset] = text(edit.sortTitle, automatic.sortTitle, true);
        next.sortTitle = value;
        if (reset) resets.add("sortTitle");
        changes.push("sortTitle");
        if (!value) invalid("sortTitle", "invalid sort title");
      }
      if (edit.description) {
        const [value, reset] = text(edit.description, automatic.description, false);
        next.description = value;
        if (reset) resets.add("description");
        changes.push("description");
      }
      for (const field of ["aliases", "genres", "tags", "platforms"] as const) {
        const change = edit[field];
        if (!change) continue;
        const [value, reset] = list(change, l.game[field], automatic[field]);
        next[field] = value;
        if (reset) resets.add(field);
        if (change.op === "add" || change.op === "remove") combined.add(field);
        changes.push(field);
      }
      if (edit.releaseDate) {
        if (edit.releaseDate.op === "reset") {
          next.releaseDate = automatic.releaseDate;
          resets.add("releaseDate");
        } else {
          next.releaseDate = edit.releaseDate.value?.trim() || null;
        }
        changes.push("releaseDate");
        if (next.releaseDate && !validDate(next.releaseDate))
          invalid("releaseDate", "release date must be YYYY-MM-DD");
      }
      if (edit.rating) {
        const value = edit.rating.value ?? null;
        if (value !== null && (!Number.isInteger(value) || value < 0 || value > 100))
          invalid("rating", "rating must be 0 through 100");
        else next.rating = value;
        changes.push("rating");
      }
      if (edit.notes) {
        next.notes = edit.notes.value;
        changes.push("notes");
      }
      for (const field of changes) {
        if (resets.has(field) || combined.has(field) || !base) continue;
        if (errors.some((error) => error.field === field)) continue;
        if (!same(base[field], l.game[field]) && !same(l.game[field], next[field]))
          errors.push({
            field,
            problem: "conflict",
            message: "changed since the editor opened",
          });
      }
      if (errors.length > 0) return { status: "rejected", state: stateOf(l), errors };
      const changed: GameField[] = [];
      for (const field of changes) {
        if (combined.has(field) && same(l.game[field], next[field])) continue;
        const was = field in l.overrides;
        if (resets.has(field)) delete l.overrides[field];
        else if (AUTOMATIC.has(field) && !was) l.overrides[field] = automatic[field];
        if (was !== field in l.overrides || !same(l.game[field], next[field])) changed.push(field);
      }
      l.game = next;
      if (changed.length > 0) show(gameId, next);
      return { status: "saved", state: stateOf(l), changed };
    },
    async artwork(gameId) {
      const a = artOf(gameId);
      return a ? slots(a) : null;
    },
    async setArtwork(gameId, kind, change) {
      const a = artOf(gameId);
      if (!a) throw new Error(`game ${gameId} was not found`);
      switch (change.op) {
        case "choose": {
          const image = [...a.automatic[kind], a.chosen[kind] ?? null].find(
            (candidate) => candidate?.id === change.artworkId,
          );
          if (!image) throw new Error(`artwork ${change.artworkId} was not found`);
          a.chosen[kind] = { ...image, id: `user-${++imports}`, source: "user" };
          break;
        }
        case "import":
          if (!IMAGE.test(change.path)) throw new Error("the file isn't a PNG, JPEG or WebP image");
          a.chosen[kind] = {
            id: `user-${++imports}`,
            source: "user",
            url: null,
            width: 512,
            height: 512,
          };
          break;
        case "remove":
          a.chosen[kind] = null;
          break;
        case "reset":
          delete a.chosen[kind];
          break;
      }
      showArt(gameId, a);
      return slots(a);
    },
    /** An automatic image arriving (a refresh): it shows unless the user
     * chose. */
    addAutomaticArtwork(gameId: string, kind: EditableArtworkKind, image: ArtworkImage) {
      const a = artOf(gameId);
      if (!a) return;
      a.automatic[kind].unshift(image);
      showArt(gameId, a);
    },
    async bulkEdit(gameIds, edit, apply) {
      const results: BulkGameResult[] = [];
      let changedAny = false;
      for (const gameId of gameIds) {
        const l = layersOf(gameId);
        if (!l) {
          results.push({ gameId, title: "", status: "missing", changes: [], errors: [] });
          continue;
        }
        const before = structuredClone(l);
        const record = records.find((g) => g.id === gameId);
        const shownBefore = record && structuredClone(record);
        quiet = true;
        const outcome = await api.save(gameId, edit, null).finally(() => {
          quiet = false;
        });
        if (outcome.status === "rejected") {
          results.push({
            gameId,
            title: before.game.title,
            status: "rejected",
            changes: [],
            errors: outcome.errors,
          });
          continue;
        }
        const text = (game: EditableGame, field: GameField) => {
          const value = game[field];
          return Array.isArray(value) ? value.join(", ") : value === null ? "" : String(value);
        };
        const changes = outcome.changed
          .map((field) => ({
            field,
            before: text(before.game, field),
            after: text(outcome.state.game, field),
          }))
          .filter((change) => change.before !== change.after);
        results.push({
          gameId,
          title: before.game.title,
          status: outcome.changed.length > 0 ? "changed" : "unchanged",
          changes,
          errors: [],
        });
        if (!apply) {
          layers.set(gameId, before);
          if (record && shownBefore) Object.assign(record, shownBefore);
        } else if (outcome.changed.length > 0) changedAny = true;
      }
      if (changedAny) emit({ type: "library.changed" });
      return results;
    },
    async gameIds(query) {
      return matching(query).map((game) => game.id);
    },
    async refreshStart(gameIds, mode) {
      if (refresh?.running) throw new Error("game details are already being refreshed");
      cancelled = false;
      refresh = {
        mode,
        running: true,
        total: gameIds.length,
        done: 0,
        updated: 0,
        unchanged: 0,
        unmatched: 0,
        skipped: 0,
        failed: [],
        cancelled: false,
        remaining: [],
      };
      publish();
      // Games with "fail" in their id fail; the others are updated. The
      // run ends once a test lets it (`finishRefresh`).
      pendingRefresh = () => {
        if (!refresh) return;
        if (cancelled) {
          refresh = { ...refresh, running: false, cancelled: true, remaining: gameIds };
        } else {
          const failed = gameIds.filter((id) => id.includes("fail"));
          refresh = {
            ...refresh,
            running: false,
            done: gameIds.length,
            updated: gameIds.length - failed.length,
            failed,
          };
        }
        pendingRefresh = null;
        publish();
      };
      // The browser preview finishes on its own.
      setTimeout(() => pendingRefresh?.(), 1000);
    },
    async refreshCancel() {
      if (!refresh?.running) return false;
      cancelled = true;
      return true;
    },
    async refreshStatus() {
      return refresh && structuredClone(refresh);
    },
    subscribeRefresh(listener) {
      refreshListeners.add(listener);
      return () => {
        refreshListeners.delete(listener);
      };
    },
    finishRefresh() {
      pendingRefresh?.();
    },
    refresh(gameId, update) {
      const l = layersOf(gameId);
      if (!l) return;
      const automatic = automaticOf(l);
      update(automatic);
      const next = structuredClone(automatic);
      for (const field of FIELDS) {
        if (field in l.overrides) {
          l.overrides[field] = automatic[field];
          (next as Record<string, unknown>)[field] = l.game[field];
        }
      }
      l.game = next;
      show(gameId, next);
    },
  };
  return api;
}
