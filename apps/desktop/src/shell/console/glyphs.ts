/** What each action is called on the controller in hand (#148), for the
 * button-hint bar. Labels follow `caskade-input`'s mapping: Nintendo pads
 * confirm with A (right) and go back with B (bottom). */
import { i18n, type MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";
import type { Action, GlyphFamily } from "../../input";

/** A pad family, or the keyboard when no pad is in use. */
export type HintFamily = GlyphFamily | "keyboard";

const face = {
  xbox: { confirm: "A", back: "B", details: "X", search: "Y" },
  playstation: { confirm: "✕", back: "○", details: "□", search: "△" },
  nintendo: { confirm: "A", back: "B", details: "Y", search: "X" },
  generic: { confirm: "A", back: "B", details: "X", search: "Y" },
} as const;

const shoulders = {
  xbox: ["LB", "RB", "LT", "RT", "Menu", "View", "Guide"],
  playstation: ["L1", "R1", "L2", "R2", "Options", "Create", "PS"],
  nintendo: ["L", "R", "ZL", "ZR", "+", "−", "Home"],
  generic: ["LB", "RB", "LT", "RT", "Start", "Select", "Guide"],
} as const;

/** Pad buttons keep the names printed on them; keys with a name translate. */
const keyboard: Partial<Record<Action, string | MessageDescriptor>> = {
  confirm: msg({ message: "Enter", context: "keyboard key" }),
  back: msg({ message: "Esc", context: "keyboard key" }),
  details: "I",
  search: "/",
  "tab.prev": "[",
  "tab.next": "]",
  "page.prev": msg({ message: "PgUp", context: "keyboard key" }),
  "page.next": msg({ message: "PgDn", context: "keyboard key" }),
  menu: "M",
  view: "V",
};

/** The label of `action`'s button, or null when the family has none (the
 * keyboard has no guide key). */
export function glyph(action: Action, family: HintFamily): string | null {
  if (family === "keyboard") {
    const key = keyboard[action];
    return key === undefined ? null : typeof key === "string" ? key : i18n._(key);
  }
  const [lb, rb, lt, rt, menu, view, guide] = shoulders[family];
  switch (action) {
    case "confirm":
    case "back":
    case "details":
    case "search":
      return face[family][action];
    case "tab.prev":
      return lb;
    case "tab.next":
      return rb;
    case "page.prev":
      return lt;
    case "page.next":
      return rt;
    case "menu":
      return menu;
    case "view":
      return view;
    case "guide":
      return guide;
    default:
      return null;
  }
}

/** Hints read left to right in this order. */
export const hintOrder: Action[] = [
  "confirm",
  "back",
  "details",
  "search",
  "tab.prev",
  "tab.next",
  "page.prev",
  "page.next",
  "menu",
  "view",
  "guide",
];

/** Face buttons are told apart by colour on most pads. */
export const faceTone: Partial<Record<Action, string>> = {
  confirm: "installed",
  back: "danger",
  details: "accent",
  search: "warning",
};
