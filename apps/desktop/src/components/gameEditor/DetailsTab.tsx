/** The editor's Details tab: the game's descriptive fields and the user's
 * own rating and notes, as a draft the editor saves in one patch. */
import { Checkbox, Slider, TextField } from "@caskade/ui";
import { useLingui } from "@lingui/react/macro";
import type { FieldError, GameEditState, GameField } from "../../api/gameEdit";
import {
  type Change,
  change,
  type Draft,
  fieldValue,
  isManual,
  type ListField,
  shown,
  type TextField as TextKey,
} from "./draft";
import { FieldFrame, ListInput, TextAreaField } from "./fields";

interface DetailsTabProps {
  state: GameEditState;
  draft: Draft;
  onDraft: (draft: Draft) => void;
  errors: FieldError[];
  /** Takes the value saved meanwhile into the draft and the base. */
  takeTheirs: (field: GameField) => void;
  /** Keeps the draft's value: the next save overwrites the one saved
   * meanwhile. */
  keepMine: (field: GameField) => void;
}

export function DetailsTab({
  state,
  draft,
  onDraft,
  errors,
  takeTheirs,
  keepMine,
}: DetailsTabProps) {
  const { t } = useLingui();
  const set = <F extends GameField>(field: F, next: Change<F> | undefined) =>
    onDraft(change(state, draft, field, next));
  const frame = (field: GameField, label: string) => {
    const error = errors.find((candidate) => candidate.field === field);
    const theirs = fieldValue(state.game, field);
    return {
      field,
      label,
      manual: isManual(state, draft, field),
      error,
      conflict:
        error?.problem === "conflict"
          ? {
              theirs: Array.isArray(theirs) ? theirs.join(", ") : String(theirs ?? ""),
              takeTheirs: () => takeTheirs(field),
              keepMine: () => keepMine(field),
            }
          : undefined,
    };
  };
  const resettable = (field: GameField, label: string) => ({
    ...frame(field, label),
    onReset: () => set(field, { op: "reset" }),
  });
  const text = (field: TextKey, label: string) => (
    <FieldFrame {...resettable(field, label)}>
      <TextField
        aria-label={label}
        value={shown(state, draft, field)}
        onChange={(value) => {
          if (field === "title") {
            // A sort title that followed the title keeps following it.
            const sort = shown(state, draft, "sortTitle");
            const following = sort === shown(state, draft, "title");
            let next = change(state, draft, "title", { op: "set", value });
            if (following) next = change(state, next, "sortTitle", { op: "set", value });
            onDraft(next);
          } else set(field, { op: "set", value });
        }}
      />
    </FieldFrame>
  );
  const list = (field: ListField, label: string, split: boolean) => (
    <FieldFrame {...resettable(field, label)}>
      <ListInput
        label={label}
        values={shown(state, draft, field)}
        split={split}
        onChange={(values) => set(field, { op: "set", value: values })}
      />
    </FieldFrame>
  );
  const rating = shown(state, draft, "rating");
  return (
    <div data-part="details" className="grid gap-x-6 gap-y-4 md:grid-cols-2">
      {text("title", t`Title`)}
      {text("sortTitle", t`Sort title`)}
      <div className="md:col-span-2">
        <FieldFrame {...resettable("description", t`Description`)}>
          <TextAreaField
            label={t`Description`}
            value={shown(state, draft, "description")}
            onChange={(value) => set("description", { op: "set", value })}
            rows={5}
          />
        </FieldFrame>
      </div>
      {list("genres", t`Genres`, true)}
      {list("tags", t`Tags`, true)}
      {list("platforms", t`Platforms`, false)}
      {list("aliases", t`Other names`, false)}
      <FieldFrame {...resettable("releaseDate", t`Release date`)}>
        <TextField
          aria-label={t`Release date`}
          placeholder={t`YYYY-MM-DD`}
          mono
          value={shown(state, draft, "releaseDate")}
          onChange={(value) => set("releaseDate", { op: "set", value })}
        />
      </FieldFrame>
      <FieldFrame {...frame("rating", t`Your rating`)}>
        <div className="flex flex-wrap items-center gap-4">
          <Checkbox
            data-spatial="focusable"
            isSelected={rating !== null}
            onChange={(rated) => set("rating", { op: "set", value: rated ? (rating ?? 50) : null })}
          >
            {t`Rated`}
          </Checkbox>
          <Slider
            aria-label={t`Your rating`}
            className="flex-1"
            minValue={0}
            maxValue={100}
            step={1}
            isDisabled={rating === null}
            value={rating ?? 0}
            onChange={(value) => set("rating", { op: "set", value })}
            format={(value) => t`${value} / 100`}
          />
        </div>
      </FieldFrame>
      <div className="md:col-span-2">
        <FieldFrame {...frame("notes", t`Notes`)}>
          <TextAreaField
            label={t`Notes`}
            value={shown(state, draft, "notes")}
            onChange={(value) => set("notes", { op: "set", value })}
            rows={6}
          />
        </FieldFrame>
      </div>
    </div>
  );
}
