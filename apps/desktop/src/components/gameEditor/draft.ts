/** The editor's unsaved changes to a game's details, field by field, and
 * the patch they make (#172). Only changed fields go in the patch, so a
 * refresh meanwhile keeps whatever it wrote to the others. */
import type { EditableGame, GameEdit, GameEditState, GameField } from "../../api/gameEdit";

export type TextField = "title" | "sortTitle" | "description" | "notes";
export type ListField = "aliases" | "genres" | "tags" | "platforms";

/** A field's value as the editor holds it. */
export interface Values {
  title: string;
  sortTitle: string;
  description: string;
  notes: string;
  aliases: string[];
  genres: string[];
  tags: string[];
  platforms: string[];
  /** `YYYY-MM-DD`, or empty for none. */
  releaseDate: string;
  rating: number | null;
}

export type Change<F extends GameField> = { op: "set"; value: Values[F] } | { op: "reset" };
export type Draft = { [F in GameField]?: Change<F> };

/** Fields automatic sources fill, so they can be reset; rating and notes
 * are only ever the user's. */
export const AUTOMATIC: readonly GameField[] = [
  "title",
  "sortTitle",
  "description",
  "aliases",
  "genres",
  "tags",
  "releaseDate",
  "platforms",
];

export function fieldValue<F extends GameField>(game: EditableGame, field: F): Values[F] {
  const value = field === "releaseDate" ? (game.releaseDate ?? "") : game[field];
  return value as Values[F];
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

/** What the field shows: the change, or the saved value. */
export function shown<F extends GameField>(
  state: GameEditState,
  draft: Draft,
  field: F,
): Values[F] {
  const change = draft[field] as Change<F> | undefined;
  if (change?.op === "set") return change.value;
  return fieldValue(change?.op === "reset" ? state.automatic : state.game, field);
}

/** Whether the field will be the user's once saved. */
export function isManual(state: GameEditState, draft: Draft, field: GameField): boolean {
  const change = draft[field];
  if (change?.op === "reset") return false;
  if (change?.op === "set") return AUTOMATIC.includes(field);
  return state.manual.includes(field);
}

/** `draft` with `field` changed; a change back to the saved value, or a
 * reset of a field that isn't the user's, drops out. */
export function change<F extends GameField>(
  state: GameEditState,
  draft: Draft,
  field: F,
  next: Change<F> | undefined,
): Draft {
  const out: Draft = { ...draft };
  const noop =
    !next ||
    (next.op === "set" && same(next.value, fieldValue(state.game, field))) ||
    (next.op === "reset" && !state.manual.includes(field));
  if (noop) delete out[field];
  else (out as Record<GameField, Change<F>>)[field] = next;
  return out;
}

/** Drops changes the saved game already has: after a save, or when a
 * refresh brought the same values. */
export function prune(state: GameEditState, draft: Draft): Draft {
  let out = draft;
  for (const field of Object.keys(draft) as GameField[]) {
    out = change(state, out, field, draft[field] as Change<typeof field>);
  }
  return out;
}

export const isDirty = (draft: Draft) => Object.keys(draft).length > 0;

/** The patch for `draft`. */
export function toEdit(draft: Draft): GameEdit {
  const edit: GameEdit = {};
  for (const field of ["title", "sortTitle", "description"] as const) {
    const change = draft[field];
    if (change) edit[field] = change.op === "reset" ? { op: "reset" } : change;
  }
  for (const field of ["aliases", "genres", "tags", "platforms"] as const) {
    const change = draft[field];
    if (change)
      edit[field] = change.op === "reset" ? { op: "reset" } : { op: "set", values: change.value };
  }
  const date = draft.releaseDate;
  if (date)
    edit.releaseDate =
      date.op === "reset" ? { op: "reset" } : { op: "set", value: date.value.trim() || null };
  const rating = draft.rating;
  if (rating?.op === "set") edit.rating = { value: rating.value };
  const notes = draft.notes;
  if (notes?.op === "set") edit.notes = { value: notes.value };
  return edit;
}
