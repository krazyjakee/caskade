/** One edit on the chosen games (#173): each field changes only when its
 * box is ticked, so values that differ between the games stay as they
 * are; lists add, remove or replace values. A preview names each game and
 * what changes before anything is saved; the result says exactly which
 * games saved, which already had it and which failed. Cancelling before
 * Apply saves nothing, and applying again changes only what is left. */
import { Button, Checkbox, SegmentedControl, Slider, TextField } from "@caskade/ui";
import { plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import {
  type BulkGameResult,
  type GameEdit,
  type GameField,
  useBulkEdit,
} from "../../api/gameEdit";
import { useInputAction } from "../../input";
import { ListInput, TextAreaField } from "../gameEditor/fields";

type ListKey = "genres" | "tags" | "platforms" | "aliases";
type ListMode = "add" | "remove" | "replace" | "reset";
type TextMode = "replace" | "reset";

interface Form {
  enabled: Partial<Record<GameField, boolean>>;
  lists: Record<ListKey, { mode: ListMode; values: string[] }>;
  description: { mode: TextMode; value: string };
  releaseDate: { mode: TextMode; value: string };
  rating: number | null;
  notes: string;
}

const emptyForm = (): Form => ({
  enabled: {},
  lists: {
    genres: { mode: "add", values: [] },
    tags: { mode: "add", values: [] },
    platforms: { mode: "add", values: [] },
    aliases: { mode: "add", values: [] },
  },
  description: { mode: "replace", value: "" },
  releaseDate: { mode: "replace", value: "" },
  rating: null,
  notes: "",
});

/** The patch the ticked fields make. */
export function bulkPatch(form: Form): GameEdit {
  const edit: GameEdit = {};
  for (const key of ["genres", "tags", "platforms", "aliases"] as const) {
    if (!form.enabled[key]) continue;
    const { mode, values } = form.lists[key];
    edit[key] =
      mode === "reset"
        ? { op: "reset" }
        : mode === "replace"
          ? { op: "set", values }
          : { op: mode, values };
  }
  if (form.enabled.description)
    edit.description =
      form.description.mode === "reset"
        ? { op: "reset" }
        : { op: "set", value: form.description.value };
  if (form.enabled.releaseDate)
    edit.releaseDate =
      form.releaseDate.mode === "reset"
        ? { op: "reset" }
        : { op: "set", value: form.releaseDate.value.trim() || null };
  if (form.enabled.rating) edit.rating = { value: form.rating };
  if (form.enabled.notes) edit.notes = { value: form.notes };
  return edit;
}

type Step =
  | { type: "edit" }
  | { type: "preview"; results: BulkGameResult[] }
  | { type: "done"; results: BulkGameResult[] };

export function BulkEditDialog({ gameIds, close }: { gameIds: string[]; close: () => void }) {
  const { t } = useLingui();
  const dialog = useRef<HTMLElement>(null);
  const [form, setForm] = useState(emptyForm);
  const [step, setStep] = useState<Step>({ type: "edit" });
  const bulk = useBulkEdit();
  // Each step's content replaces the last, so focus moves to its main
  // button rather than falling out of the dialog.
  const primaryButton = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (step.type !== "edit") primaryButton.current?.focus();
  }, [step.type]);
  const edit = bulkPatch(form);
  const ready = Object.keys(edit).length > 0;
  const labels: Record<GameField, string> = {
    title: t`Title`,
    sortTitle: t`Sort title`,
    description: t`Description`,
    aliases: t`Other names`,
    genres: t`Genres`,
    tags: t`Tags`,
    releaseDate: t`Release date`,
    platforms: t`Platforms`,
    rating: t`Your rating`,
    notes: t`Notes`,
  };
  const run = (apply: boolean) =>
    bulk.mutate(
      { gameIds, edit, apply },
      { onSuccess: (results) => setStep({ type: apply ? "done" : "preview", results }) },
    );
  const primary = () => {
    if (bulk.isPending) return;
    if (step.type === "edit" && ready) run(false);
    else if (step.type === "preview") run(true);
    else if (step.type === "done") close();
  };
  useInputAction("menu", primary, {
    label: step.type === "edit" ? t`Preview` : step.type === "preview" ? t`Apply` : t`Done`,
    owner: dialog,
  });
  const toggle = (field: GameField) => (on: boolean) =>
    setForm((f) => ({ ...f, enabled: { ...f.enabled, [field]: on } }));
  const listField = (key: ListKey) => {
    const state = form.lists[key];
    const setList = (next: Partial<Form["lists"][ListKey]>) =>
      setForm((f) => ({ ...f, lists: { ...f.lists, [key]: { ...f.lists[key], ...next } } }));
    return (
      <Field key={key} field={key} label={labels[key]} form={form} toggle={toggle(key)}>
        <SegmentedControl
          aria-label={t`How to change ${labels[key]}`}
          value={state.mode}
          onChange={(mode) => setList({ mode })}
          options={[
            { id: "add", label: t`Add` },
            { id: "remove", label: t`Remove` },
            { id: "replace", label: t`Replace` },
            { id: "reset", label: t`Reset to automatic` },
          ]}
        />
        {state.mode !== "reset" && (
          <ListInput
            label={labels[key]}
            values={state.values}
            split={key !== "aliases"}
            onChange={(values) => setList({ values })}
          />
        )}
      </Field>
    );
  };
  const count = gameIds.length;
  return (
    <ModalOverlay
      isOpen
      isDismissable={!bulk.isPending}
      onOpenChange={(open) => {
        if (!open && !bulk.isPending) close();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm console:p-0"
    >
      <Modal className="flex h-[88vh] w-full max-w-3xl flex-col rounded-panel border border-border-strong bg-surface-raised shadow-lg console:h-full console:max-w-none console:rounded-none">
        <Dialog
          ref={dialog}
          data-cs="bulk-editor"
          data-step={step.type}
          className="flex min-h-0 flex-1 flex-col outline-none"
        >
          <Heading slot="title" className="px-6 pt-5 font-display text-xl font-semibold">
            {plural(count, { one: "Edit # game", other: "Edit # games" })}
          </Heading>
          <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">
            {step.type === "edit" && (
              <div data-part="fields" className="flex flex-col gap-4">
                <p className="text-sm text-fg-muted">
                  <Trans>Tick what to change. Everything else stays as each game has it.</Trans>
                </p>
                {listField("tags")}
                {listField("genres")}
                {listField("platforms")}
                {listField("aliases")}
                <Field
                  field="description"
                  label={labels.description}
                  form={form}
                  toggle={toggle("description")}
                >
                  <SegmentedControl
                    aria-label={t`How to change ${labels.description}`}
                    value={form.description.mode}
                    onChange={(mode) =>
                      setForm((f) => ({ ...f, description: { ...f.description, mode } }))
                    }
                    options={[
                      { id: "replace", label: t`Replace` },
                      { id: "reset", label: t`Reset to automatic` },
                    ]}
                  />
                  {form.description.mode === "replace" && (
                    <TextAreaField
                      label={labels.description}
                      value={form.description.value}
                      onChange={(value) =>
                        setForm((f) => ({ ...f, description: { ...f.description, value } }))
                      }
                    />
                  )}
                </Field>
                <Field
                  field="releaseDate"
                  label={labels.releaseDate}
                  form={form}
                  toggle={toggle("releaseDate")}
                >
                  <SegmentedControl
                    aria-label={t`How to change ${labels.releaseDate}`}
                    value={form.releaseDate.mode}
                    onChange={(mode) =>
                      setForm((f) => ({ ...f, releaseDate: { ...f.releaseDate, mode } }))
                    }
                    options={[
                      { id: "replace", label: t`Set` },
                      { id: "reset", label: t`Reset to automatic` },
                    ]}
                  />
                  {form.releaseDate.mode === "replace" && (
                    <TextField
                      aria-label={labels.releaseDate}
                      placeholder={t`YYYY-MM-DD, or empty for none`}
                      mono
                      value={form.releaseDate.value}
                      onChange={(value) =>
                        setForm((f) => ({ ...f, releaseDate: { ...f.releaseDate, value } }))
                      }
                    />
                  )}
                </Field>
                <Field field="rating" label={labels.rating} form={form} toggle={toggle("rating")}>
                  <div className="flex flex-wrap items-center gap-4">
                    <Checkbox
                      isSelected={form.rating !== null}
                      onChange={(rated) => setForm((f) => ({ ...f, rating: rated ? 50 : null }))}
                    >
                      {t`Rated`}
                    </Checkbox>
                    <Slider
                      aria-label={labels.rating}
                      className="flex-1"
                      minValue={0}
                      maxValue={100}
                      isDisabled={form.rating === null}
                      value={form.rating ?? 0}
                      onChange={(rating) => setForm((f) => ({ ...f, rating }))}
                      format={(value) => t`${value} / 100`}
                    />
                  </div>
                </Field>
                <Field field="notes" label={labels.notes} form={form} toggle={toggle("notes")}>
                  <TextAreaField
                    label={labels.notes}
                    value={form.notes}
                    onChange={(notes) => setForm((f) => ({ ...f, notes }))}
                  />
                </Field>
              </div>
            )}
            {step.type !== "edit" && (
              <Results results={step.results} labels={labels} applied={step.type === "done"} />
            )}
          </div>
          <div
            data-part="footer"
            className="flex flex-wrap items-center gap-3 border-t border-border px-6 py-4"
          >
            <p role="status" data-part="status" className="mr-auto text-sm">
              {bulk.error ? (
                <span className="text-danger">
                  {t`That didn't finish: ${bulk.error.message}. Trying again is safe: what saved stays as it is.`}
                </span>
              ) : bulk.isPending ? (
                <span className="text-fg-muted">
                  {step.type === "edit" ? t`Working out the changes…` : t`Saving…`}
                </span>
              ) : null}
            </p>
            {step.type !== "done" && (
              <Button
                data-part="cancel"
                data-spatial="focusable"
                isDisabled={bulk.isPending}
                onPress={step.type === "preview" ? () => setStep({ type: "edit" }) : close}
              >
                {step.type === "preview" ? <Trans>Back</Trans> : <Trans>Cancel</Trans>}
              </Button>
            )}
            <Button
              ref={primaryButton}
              data-part="apply"
              data-spatial="focusable"
              variant="primary"
              isDisabled={bulk.isPending || (step.type === "edit" && !ready)}
              onPress={primary}
            >
              {step.type === "edit" ? (
                <Trans>Preview</Trans>
              ) : step.type === "preview" ? (
                bulk.error ? (
                  <Trans>Try again</Trans>
                ) : (
                  <Trans>Apply</Trans>
                )
              ) : (
                <Trans>Done</Trans>
              )}
            </Button>
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}

function Field({
  field,
  label,
  form,
  toggle,
  children,
}: {
  field: GameField;
  label: string;
  form: Form;
  toggle: (on: boolean) => void;
  children: ReactNode;
}) {
  const { t } = useLingui();
  const on = Boolean(form.enabled[field]);
  return (
    <div
      data-part="field"
      data-field={field}
      data-enabled={on || undefined}
      className="flex flex-col gap-2"
    >
      <Checkbox data-spatial="focusable" isSelected={on} onChange={toggle}>
        {t`Change ${label}`}
      </Checkbox>
      {on && <div className="flex flex-col gap-2 pl-7">{children}</div>}
    </div>
  );
}

/** How many games each outcome covers, and the games themselves. */
function Results({
  results,
  labels,
  applied,
}: {
  results: BulkGameResult[];
  labels: Record<GameField, string>;
  applied: boolean;
}) {
  const { t } = useLingui();
  const by = (status: BulkGameResult["status"]) =>
    results.filter((result) => result.status === status);
  const changed = by("changed");
  const unchanged = by("unchanged");
  const rejected = by("rejected");
  const missing = by("missing");
  const shown = changed.slice(0, SHOWN);
  return (
    <div data-part="results" className="flex flex-col gap-4">
      <ul data-part="summary" className="flex flex-wrap gap-x-6 gap-y-1 text-sm">
        <li>
          {applied
            ? plural(changed.length, { one: "# game saved", other: "# games saved" })
            : plural(changed.length, { one: "# game changes", other: "# games change" })}
        </li>
        <li className="text-fg-muted">
          {plural(unchanged.length, {
            one: "# already has it",
            other: "# already have it",
          })}
        </li>
        {rejected.length > 0 && (
          <li className="text-danger">
            {plural(rejected.length, { one: "# can't take it", other: "# can't take it" })}
          </li>
        )}
        {missing.length > 0 && (
          <li className="text-fg-muted">
            {plural(missing.length, {
              one: "# is no longer in the library",
              other: "# are no longer in the library",
            })}
          </li>
        )}
      </ul>
      {rejected.length > 0 && (
        <section aria-label={t`Not saved`} className="flex flex-col gap-1">
          <h3 className="text-sm font-semibold text-danger">
            <Trans>Not saved</Trans>
          </h3>
          <ul className="flex flex-col gap-1 text-sm">
            {rejected.map((result) => (
              <li key={result.gameId} data-part="rejected">
                <span className="font-medium">{result.title}</span>
                {": "}
                {result.errors
                  .map((error) => `${labels[error.field]}: ${error.message}`)
                  .join("; ")}
              </li>
            ))}
          </ul>
        </section>
      )}
      {shown.length > 0 && (
        <section aria-label={applied ? t`Saved` : t`What changes`} className="flex flex-col gap-1">
          <h3 className="text-sm font-semibold">
            {applied ? <Trans>Saved</Trans> : <Trans>What changes</Trans>}
          </h3>
          <ul className="flex flex-col divide-y divide-border text-sm">
            {shown.map((result) => (
              <li key={result.gameId} data-part="change" className="py-2">
                <p className="font-medium">{result.title}</p>
                {result.changes.map((change) => (
                  <p key={change.field} className="text-xs text-fg-muted">
                    {labels[change.field]}: <del>{change.before || t`none`}</del>
                    {" → "}
                    <ins className="no-underline text-fg">{change.after || t`none`}</ins>
                  </p>
                ))}
              </li>
            ))}
          </ul>
          {changed.length > shown.length && (
            <p className="text-xs text-fg-subtle">
              {t`And ${changed.length - shown.length} more.`}
            </p>
          )}
        </section>
      )}
    </div>
  );
}

/** Games a preview lists one by one; the count covers the rest. */
const SHOWN = 200;
