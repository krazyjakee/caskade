/** The editor's fields: a frame with the field's label, whether the value
 * is the user's, Reset, and what went wrong; a multi-line text field; and
 * a list of values as removable chips. */
import { Button, TextField } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { Plus, RotateCcw, UserPen, X } from "lucide-react";
import { type ReactNode, useId, useState } from "react";
import { TextField as AriaTextField, TextArea } from "react-aria-components";
import type { FieldError } from "../../api/gameEdit";

export function FieldFrame({
  field,
  label,
  labelId,
  manual,
  onReset,
  error,
  conflict,
  children,
}: {
  field: string;
  label: string;
  /** For a field whose control labels itself with this id. */
  labelId?: string;
  /** The value is the user's (set by hand, kept through refreshes). */
  manual: boolean;
  /** Back to the automatic value; absent for fields without one. */
  onReset?: () => void;
  error?: FieldError;
  /** For a conflict: take the value saved meanwhile, or keep the user's. */
  conflict?: { takeTheirs: () => void; keepMine: () => void; theirs: string };
  children: ReactNode;
}) {
  const { t } = useLingui();
  return (
    <div
      data-part="field"
      data-field={field}
      data-manual={manual || undefined}
      data-invalid={error ? true : undefined}
      className="flex flex-col gap-1.5"
    >
      <div className="flex min-h-8 items-center gap-2">
        <span id={labelId} data-part="label" className="text-sm font-medium">
          {label}
        </span>
        {manual && onReset && (
          <span
            data-part="manual"
            title={t`Set by you: refreshes keep it until you reset it.`}
            className="inline-flex items-center gap-1 rounded-control border border-accent-alt/40 px-1.5 py-0.5 text-[11px] text-accent-alt"
          >
            <UserPen className="size-3" />
            <Trans>Yours</Trans>
          </span>
        )}
        {manual && onReset && (
          <Button
            data-part="reset"
            data-spatial="focusable"
            variant="ghost"
            size="sm"
            icon={RotateCcw}
            className="ml-auto"
            aria-label={t`Reset ${label} to automatic`}
            onPress={onReset}
          >
            <Trans>Reset</Trans>
          </Button>
        )}
      </div>
      {children}
      {error && (
        <div role="alert" data-part="error" className="flex flex-wrap items-center gap-2">
          <span className="text-xs text-danger">
            {error.problem === "conflict" && conflict ? (
              <Trans>
                Changed elsewhere since you opened the editor; it is now “{conflict.theirs}”.
              </Trans>
            ) : (
              error.message
            )}
          </span>
          {error.problem === "conflict" && conflict && (
            <>
              <Button
                data-part="use-theirs"
                data-spatial="focusable"
                size="sm"
                variant="ghost"
                onPress={conflict.takeTheirs}
              >
                <Trans>Use theirs</Trans>
              </Button>
              <Button
                data-part="keep-mine"
                data-spatial="focusable"
                size="sm"
                variant="ghost"
                onPress={conflict.keepMine}
              >
                <Trans>Keep mine</Trans>
              </Button>
            </>
          )}
        </div>
      )}
    </div>
  );
}

/** A multi-line text field (a description, notes). */
export function TextAreaField({
  label,
  value,
  onChange,
  rows = 4,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  rows?: number;
}) {
  return (
    <AriaTextField
      aria-label={label}
      value={value}
      onChange={onChange}
      data-cs="text-field"
      className="group flex flex-col"
    >
      <div
        data-part="control"
        className="flex rounded-control border border-border-strong bg-surface-base/60 transition-[border-color,box-shadow] duration-(--cs-motion-duration-fast) focus-within:border-accent-alt/60 focus-within:shadow-[0_0_16px_-6px] focus-within:shadow-accent-alt console:focus-within:focus-ring"
      >
        <TextArea
          data-part="input"
          data-spatial="focusable"
          rows={rows}
          className="block w-full resize-y bg-transparent px-3 py-2 text-sm text-fg outline-none placeholder:text-fg-subtle"
        />
      </div>
    </AriaTextField>
  );
}

/** A list of values as chips, each with its own remove button, and a
 * field that adds more: Enter or Add, and with `split` a comma between
 * several. */
export function ListInput({
  label,
  values,
  onChange,
  split = false,
}: {
  label: string;
  values: string[];
  onChange: (values: string[]) => void;
  split?: boolean;
}) {
  const { t } = useLingui();
  const [text, setText] = useState("");
  const listId = useId();
  const add = () => {
    const added = (split ? text.split(",") : [text])
      .map((value) => value.trim())
      .filter((value) => value && !values.includes(value));
    if (added.length > 0) onChange([...values, ...new Set(added)]);
    setText("");
  };
  return (
    <div data-part="list" className="flex flex-col gap-2">
      {values.length > 0 ? (
        <ul id={listId} aria-label={label} className="flex flex-wrap gap-1.5">
          {values.map((value) => (
            <li
              key={value}
              data-part="chip"
              className="inline-flex items-center gap-1 rounded-control border border-border-strong bg-surface-overlay/70 py-0.5 pr-0.5 pl-2 text-xs"
            >
              <span>{value}</span>
              <Button
                data-part="remove"
                data-spatial="focusable"
                variant="ghost"
                size="sm"
                icon={X}
                className="h-6 px-1.5"
                aria-label={t`Remove ${value}`}
                onPress={() => onChange(values.filter((other) => other !== value))}
              />
            </li>
          ))}
        </ul>
      ) : (
        <p className="text-xs text-fg-subtle">
          <Trans>None</Trans>
        </p>
      )}
      <TextField
        aria-label={t`Add to ${label}`}
        placeholder={split ? t`Add, separated by commas` : t`Add`}
        value={text}
        onChange={setText}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            add();
          }
        }}
        action={
          <Button
            data-spatial="focusable"
            size="md"
            icon={Plus}
            isDisabled={!text.trim()}
            aria-label={t`Add to ${label}`}
            onPress={add}
          />
        }
      />
    </div>
  );
}
