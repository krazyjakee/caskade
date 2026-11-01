import { expect, it } from "vitest";
import type { EditableGame, GameEditState } from "../../api/gameEdit";
import { change, isDirty, isManual, prune, shown, toEdit } from "./draft";

const game: EditableGame = {
  id: "g",
  title: "Portal 2",
  sortTitle: "Portal 2",
  description: "",
  aliases: [],
  genres: ["Puzzle"],
  tags: [],
  releaseDate: "2011-04-19",
  platforms: ["Windows"],
  favorite: false,
  hidden: false,
  rating: null,
  notes: "",
  defaultEntry: null,
};
const state: GameEditState = {
  game,
  automatic: { ...game, description: "From IGDB." },
  manual: ["description"],
};

it("only changed fields go in the patch, with reset and clear apart", () => {
  let draft = change(state, {}, "title", { op: "set", value: "Portal Two" });
  draft = change(state, draft, "description", { op: "reset" });
  draft = change(state, draft, "releaseDate", { op: "set", value: "" });
  draft = change(state, draft, "rating", { op: "set", value: 80 });
  expect(toEdit(draft)).toEqual({
    title: { op: "set", value: "Portal Two" },
    description: { op: "reset" },
    releaseDate: { op: "set", value: null },
    rating: { value: 80 },
  });
  expect(shown(state, draft, "description")).toBe("From IGDB.");
  expect(isManual(state, draft, "description")).toBe(false);
  expect(isManual(state, draft, "title")).toBe(true);
  // Personal fields are never "the user's" against an automatic value.
  expect(isManual(state, draft, "rating")).toBe(false);
});

it("a change back to the saved value, or a reset of an automatic field, drops out", () => {
  let draft = change(state, {}, "title", { op: "set", value: "X" });
  draft = change(state, draft, "title", { op: "set", value: "Portal 2" });
  draft = change(state, draft, "genres", { op: "reset" });
  expect(isDirty(draft)).toBe(false);
  const saved = { ...state, game: { ...game, notes: "Done" } };
  expect(prune(saved, { notes: { op: "set", value: "Done" } })).toEqual({});
});
