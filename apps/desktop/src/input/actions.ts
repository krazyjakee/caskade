/** The console-mode action contract (#146): pads send these from Rust, and
 * the keyboard map below produces the same ones. */
import type { Action, GlyphFamily } from "../api/bindings";

export type { Action, GlyphFamily };
export type InputSource = "pad" | "keyboard";

export const directions = ["nav.up", "nav.down", "nav.left", "nav.right"] as const;
export type Direction = (typeof directions)[number];
export const isDirection = (action: Action): action is Direction =>
  (directions as readonly string[]).includes(action);

/** Fields where keys keep their typing meaning. */
export function isTextEntry(element: Element | null): boolean {
  if (!element) return false;
  if (element instanceof HTMLTextAreaElement) return true;
  if (element instanceof HTMLElement && element.isContentEditable) return true;
  if (!(element instanceof HTMLInputElement)) return false;
  return !["checkbox", "radio", "button", "submit", "reset", "range", "color", "file"].includes(
    element.type,
  );
}

const keys: Record<string, Action> = {
  ArrowUp: "nav.up",
  ArrowDown: "nav.down",
  ArrowLeft: "nav.left",
  ArrowRight: "nav.right",
  Enter: "confirm",
  Escape: "back",
  Backspace: "back",
  "[": "tab.prev",
  "]": "tab.next",
  PageUp: "page.prev",
  PageDown: "page.next",
  "/": "search",
  F3: "search",
  i: "details",
  ContextMenu: "menu",
  m: "menu",
  v: "view",
};

/** The keyboard's action for `event`, or null to leave the key alone:
 * modifiers, and keys that edit or move the caret in a text field. Up and
 * down still leave a field, and Escape still goes back. */
export function keyAction(event: KeyboardEvent): Action | null {
  if (event.ctrlKey || event.metaKey || event.altKey) return null;
  const action = keys[event.key] ?? null;
  if (!action) return null;
  const target = event.target instanceof Element ? event.target : null;
  if (isTextEntry(target)) {
    return action === "nav.up" ||
      action === "nav.down" ||
      (action === "back" && event.key === "Escape")
      ? action
      : null;
  }
  // A slider takes left/right itself.
  if (
    target?.getAttribute("role") === "slider" ||
    (target instanceof HTMLInputElement && target.type === "range")
  )
    if (action === "nav.left" || action === "nav.right") return null;
  return action;
}
