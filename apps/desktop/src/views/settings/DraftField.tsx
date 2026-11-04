import { TextField } from "@caskade/ui";
import { type ComponentProps, useEffect, useRef, useState } from "react";

type DraftFieldProps = Omit<
  ComponentProps<typeof TextField>,
  "value" | "defaultValue" | "onChange" | "onFocus" | "onBlur" | "onKeyDown"
> & {
  value: string;
  commit: (next: string) => void;
  /** Saves after this many milliseconds without typing; `null` saves only
   * on blur and Enter, for a value that does work as it changes (a folder
   * that is checked on disk). */
  pause?: number | null;
};

/** A text field that keeps what the user types. It saves after a short
 * pause, on Enter or on blur, and takes the stored value only while it isn't
 * focused: saving every keystroke and showing the stored value lost
 * characters typed faster than a save round trip. */
export function DraftField({ value, commit, pause = 600, ...props }: DraftFieldProps) {
  const [draft, setDraft] = useState(value);
  const focused = useRef(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => {
    if (!focused.current) setDraft(value);
  }, [value]);
  useEffect(() => () => clearTimeout(timer.current), []);
  const save = (next: string) => {
    clearTimeout(timer.current);
    if (next !== value) commit(next);
  };
  return (
    <TextField
      {...props}
      value={draft}
      onFocus={() => {
        focused.current = true;
      }}
      onBlur={() => {
        focused.current = false;
        save(draft);
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter") save(draft);
      }}
      onChange={(next) => {
        setDraft(next);
        clearTimeout(timer.current);
        if (pause !== null) timer.current = setTimeout(() => save(next), pause);
      }}
    />
  );
}
