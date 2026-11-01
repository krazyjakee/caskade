/** The on-screen keyboard: how a pad types. It opens on a text field (A on
 * the field, Y for the search, a tap in console mode) and types into it
 * while focus stays there, so a field's own behaviour (the search's
 * suggestions) carries on. The pad's buttons follow the app's rules: the
 * d-pad moves between keys, A types, B closes, X deletes, Y is a space,
 * LB/RB switch between letters and symbols (the keyboard's tabs), LT/RT
 * move the cursor (they page through what's there), Start is Done. Keys
 * also take taps and clicks, which leave focus in the field. */
import { useLingui } from "@lingui/react/macro";
import { Delete, MoveLeft, MoveRight } from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import type { Action } from "./actions";
import { useInputAction } from "./hooks";
import { press } from "./keys";

export type TextTarget = HTMLInputElement | HTMLTextAreaElement;

type Special = "shift" | "layer" | "backspace" | "space" | "left" | "right" | "done";
interface Key {
  /** The character it types, or what it does. */
  type: string | Special;
  /** In key widths; 1 by default. */
  width?: number;
}
type Layer = "letters" | "symbols";
type Shift = "off" | "once" | "lock";

const chars = (row: string): Key[] => Array.from(row, (type) => ({ type }));
const bottom: Key[] = [
  { type: "layer", width: 2 },
  { type: "left" },
  { type: "space", width: 6 },
  { type: "right" },
  { type: "done", width: 2 },
];
/** A language's letter rows (twelve, twelve and ten keys, beside shift)
 * and `more`: its other letters and the punctuation its letters pushed
 * out, as a row of the symbols. */
interface Layout {
  letters: readonly [string, string, string];
  more?: string;
}
const english: Layout = { letters: ["qwertyuiop-_", "asdfghjkl:./", "zxcvbnm,@'"] };
/** A layout per language, as its people's keyboards are, with the letters
 * the language needs within a pad's reach. Others get English's. */
export const keyboardLayouts: Readonly<Record<string, Layout>> = {
  en: english,
  // AZERTY, with é è à ç on the letters and the other accents on the symbols.
  fr: { letters: ["azertyuiopéè", "qsdfghjklmàç", "wxcvbn,.-'"], more: "âêîôûëïüùœ:_" },
  // QWERTZ, with ü ö ä ß.
  de: { letters: ["qwertzuiopü-", "asdfghjklöä.", "yxcvbnm,ß'"], more: ":_" },
  // QWERTY, with å ä ö where a Swedish keyboard has them.
  sv: { letters: ["qwertyuiopå-", "asdfghjklöä.", "zxcvbnm,@'"], more: ":_éü" },
};
/** The layout for `locale` (`fr-CA` is French's), else English's. */
export function layoutFor(locale: string): Layout {
  const language = locale.toLowerCase().split("-")[0] ?? "";
  return keyboardLayouts[language] ?? english;
}
/** Every row is twelve keys wide, so a column lines up from row to row. */
function layersOf({ letters: [top, middle, last], more }: Layout): Record<Layer, Key[][]> {
  return {
    letters: [
      [...chars("1234567890"), { type: "backspace", width: 2 }],
      chars(top),
      chars(middle),
      [{ type: "shift", width: 2 }, ...chars(last)],
      bottom,
    ],
    symbols: [
      [...chars("!@#$%^&*()"), { type: "backspace", width: 2 }],
      chars("~`=+[]{}|\\<>"),
      chars(";\"'?,./€£¥°§"),
      ...(more ? [chars(more)] : []),
      bottom,
    ],
  };
}

const specials: readonly string[] = [
  "shift",
  "layer",
  "backspace",
  "space",
  "left",
  "right",
  "done",
] satisfies Special[];
const special = (type: string): type is Special => specials.includes(type);

/** The key a column position falls on in `row`, by the keys' widths. */
function keyAt(row: Key[], x: number): number {
  let end = 0;
  for (const [index, key] of row.entries()) {
    end += key.width ?? 1;
    if (x < end) return index;
  }
  return row.length - 1;
}
/** Where the middle of key `index` is along its row. */
function middle(row: Key[], index: number): number {
  let start = 0;
  for (const key of row.slice(0, index)) start += key.width ?? 1;
  return start + (row[index]?.width ?? 1) / 2;
}

/** Sets the field's value the way typing would, so React and React Aria
 * hear it, and puts the cursor at `caret`. */
function write(target: TextTarget, value: string, caret: number) {
  const prototype =
    target instanceof HTMLTextAreaElement
      ? HTMLTextAreaElement.prototype
      : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(prototype, "value")?.set?.call(target, value);
  target.dispatchEvent(new Event("input", { bubbles: true }));
  try {
    target.setSelectionRange(caret, caret);
  } catch {
    // Fields without a cursor (email, number) type at the end.
  }
}
function selection(target: TextTarget): [number, number] {
  const end = target.value.length;
  try {
    return [target.selectionStart ?? end, target.selectionEnd ?? end];
  } catch {
    return [end, end];
  }
}
function insert(target: TextTarget, text: string) {
  const [start, end] = selection(target);
  write(target, target.value.slice(0, start) + text + target.value.slice(end), start + text.length);
}
function deleteBack(target: TextTarget) {
  const [start, end] = selection(target);
  const from = start === end ? Math.max(0, start - 1) : start;
  write(target, target.value.slice(0, from) + target.value.slice(end), from);
}
function moveCaret(target: TextTarget, by: number) {
  const [start] = selection(target);
  const at = Math.min(target.value.length, Math.max(0, start + by));
  try {
    target.setSelectionRange(at, at);
  } catch {
    // No cursor to move.
  }
}
/** Enter on the field (the search searches); a form with nothing taking
 * the Enter submits. */
function submit(target: TextTarget) {
  const taken = press(target, "Enter");
  if (!taken && target.isConnected && target.form) target.form.requestSubmit();
}

export function OnScreenKeyboard({ target, close }: { target: TextTarget; close: () => void }) {
  const { t, i18n } = useLingui();
  const locale = i18n.locale;
  const layers = layersOf(layoutFor(locale));
  const deleteLabel = t({ message: "Delete", context: "on-screen keyboard" });
  const spaceLabel = t({ message: "Space", context: "on-screen keyboard" });
  const cursorLeft = t`Cursor left`;
  const cursorRight = t`Cursor right`;
  const doneLabel = t({ message: "Done", context: "on-screen keyboard" });
  const labels: Record<Special, ReactNode> = {
    shift: "⇧",
    layer: null,
    backspace: <Delete className="mx-auto size-5" aria-label={deleteLabel} />,
    space: spaceLabel,
    left: <MoveLeft className="mx-auto size-5" aria-label={cursorLeft} />,
    right: <MoveRight className="mx-auto size-5" aria-label={cursorRight} />,
    done: doneLabel,
  };
  const owner = useRef<Element | null>(target);
  owner.current = target;
  const [layer, setLayer] = useState<Layer>("letters");
  const [shift, setShift] = useState<Shift>("off");
  const [at, setAt] = useState({ row: 1, col: 0 });
  const [text, setText] = useState(target.value);
  // The preview follows the field, whoever types.
  useEffect(() => {
    const update = () => setText(target.value);
    target.addEventListener("input", update);
    return () => target.removeEventListener("input", update);
  }, [target]);
  // The field stays in view above the keyboard.
  useEffect(() => {
    target.scrollIntoView?.({ block: "center", behavior: "auto" });
  }, [target]);

  const rows = layers[layer];
  const switchLayer = () => {
    const next = layer === "letters" ? "symbols" : "letters";
    setLayer(next);
    setAt(({ row, col }) => {
      const from = rows[row];
      const to = Math.min(row, layers[next].length - 1);
      const toRow = layers[next][to] ?? [];
      return { row: to, col: from ? keyAt(toRow, middle(from, col)) : 0 };
    });
  };
  const run = (key: Key) => {
    switch (key.type) {
      case "shift":
        setShift((now) => (now === "off" ? "once" : now === "once" ? "lock" : "off"));
        return;
      case "layer":
        switchLayer();
        return;
      case "backspace":
        deleteBack(target);
        return;
      case "space":
        insert(target, " ");
        return;
      case "left":
        moveCaret(target, -1);
        return;
      case "right":
        moveCaret(target, 1);
        return;
      case "done":
        close();
        submit(target);
        return;
      default: {
        const upper = layer === "letters" && shift !== "off";
        insert(target, upper ? key.type.toLocaleUpperCase(locale) : key.type);
        if (shift === "once") setShift("off");
      }
    }
  };
  const move = (action: Action) => {
    setAt(({ row, col }) => {
      const keys = rows[row] ?? [];
      if (action === "nav.left") return { row, col: (col - 1 + keys.length) % keys.length };
      if (action === "nav.right") return { row, col: (col + 1) % keys.length };
      const next = Math.min(rows.length - 1, Math.max(0, row + (action === "nav.down" ? 1 : -1)));
      return { row: next, col: keyAt(rows[next] ?? [], middle(keys, col)) };
    });
    return true;
  };
  // Owned by the field, so they come first while it has focus, and global
  // so they still do over the field's own popover (the suggestions).
  const on = { owner, global: true };
  useInputAction("nav.up", () => move("nav.up"), on);
  useInputAction("nav.down", () => move("nav.down"), on);
  useInputAction("nav.left", () => move("nav.left"), on);
  useInputAction("nav.right", () => move("nav.right"), on);
  useInputAction(
    "confirm",
    () => {
      const key = rows[at.row]?.[at.col];
      if (key) run(key);
    },
    {
      ...on,
      label: t({
        message: "Type",
        context: "on-screen keyboard",
        comment: "Hint bar: type the focused key",
      }),
    },
  );
  useInputAction("back", close, { ...on, label: t`Close keyboard` });
  useInputAction("details", () => deleteBack(target), { ...on, label: deleteLabel });
  useInputAction("search", () => insert(target, " "), { ...on, label: spaceLabel });
  // Two layers: LB and RB both switch, and say so over the view's tabs.
  const other =
    layer === "letters"
      ? t({ message: "Symbols", context: "on-screen keyboard" })
      : t({ message: "Letters", context: "on-screen keyboard" });
  useInputAction("tab.prev", switchLayer, { ...on, label: other });
  useInputAction("tab.next", switchLayer, { ...on, label: other });
  useInputAction("page.prev", () => moveCaret(target, -1), { ...on, label: cursorLeft });
  useInputAction("page.next", () => moveCaret(target, 1), { ...on, label: cursorRight });
  useInputAction(
    "menu",
    () => {
      close();
      submit(target);
    },
    { ...on, label: doneLabel },
  );

  const secret = target instanceof HTMLInputElement && target.type === "password";
  return (
    <fieldset
      data-cs="osk"
      data-layer={layer}
      data-shift={shift}
      aria-label={t`On-screen keyboard`}
      className="fixed inset-x-0 z-[60] mx-auto w-full max-w-4xl px-4"
      style={{ bottom: "calc(var(--cs-hintbar-height, 0px) + 1rem)" }}
    >
      <div className="rounded-panel border border-border-strong bg-surface-overlay/95 p-4 shadow-xl shadow-black/40 backdrop-blur">
        <output
          data-part="preview"
          className="mb-3 block min-h-11 truncate rounded-control border border-border bg-surface-base px-4 py-2 font-mono text-lg text-fg"
        >
          {secret ? "•".repeat(text.length) : text}
        </output>
        <div className="flex flex-col gap-2">
          {rows.map((row, r) => (
            <div
              // biome-ignore lint/suspicious/noArrayIndexKey: the rows are fixed.
              key={r}
              data-part="row"
              className="flex gap-2"
            >
              {row.map((key, c) => {
                const focused = at.row === r && at.col === c;
                const upper = layer === "letters" && shift !== "off";
                const label = special(key.type)
                  ? key.type === "layer"
                    ? layer === "letters"
                      ? t({
                          message: "?123",
                          context: "on-screen keyboard",
                          comment: "The key that switches to symbols",
                        })
                      : t({
                          message: "abc",
                          context: "on-screen keyboard",
                          comment: "The key that switches back to letters",
                        })
                    : labels[key.type]
                  : upper
                    ? key.type.toLocaleUpperCase(locale)
                    : key.type;
                return (
                  <button
                    // biome-ignore lint/suspicious/noArrayIndexKey: the keys are fixed.
                    key={c}
                    type="button"
                    tabIndex={-1}
                    data-part="key"
                    data-key={key.type}
                    data-focused={focused || undefined}
                    data-selected={(key.type === "shift" && shift !== "off") || undefined}
                    // A tap types without taking focus from the field.
                    onPointerDown={(event) => event.preventDefault()}
                    onClick={() => {
                      setAt({ row: r, col: c });
                      run(key);
                    }}
                    style={{ flex: key.width ?? 1 }}
                    className="h-12 min-w-0 rounded-control border border-border bg-surface-raised font-mono text-lg text-fg data-focused:border-accent-alt data-focused:bg-accent data-focused:text-accent-fg data-selected:text-accent-alt"
                  >
                    {label}
                  </button>
                );
              })}
            </div>
          ))}
        </div>
      </div>
    </fieldset>
  );
}

/** Whether the keyboard should take `element`: a field that types text. */
export function typesText(element: Element | null): element is TextTarget {
  if (element instanceof HTMLTextAreaElement) return !element.readOnly && !element.disabled;
  if (!(element instanceof HTMLInputElement) || element.readOnly || element.disabled) return false;
  return ["text", "search", "url", "email", "password", "tel", "number", ""].includes(element.type);
}
