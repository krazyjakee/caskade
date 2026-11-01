import { useEffect } from "react";
import type { AppWindow } from "./appWindow";

/** Pointer travel before a press on the title bar becomes a window move. */
const DRAG_THRESHOLD = 4;

const CLICKABLE = "a, button, input, select, textarea, label, summary, [contenteditable]";
const INTERACTIVE_ROLES = new Set([
  "button",
  "link",
  "menuitem",
  "tab",
  "checkbox",
  "radio",
  "switch",
  "option",
]);

function isClickable(element: HTMLElement) {
  if (element.matches(CLICKABLE)) return element.getAttribute("contenteditable") !== "false";
  const tabindex = element.getAttribute("tabindex");
  if (tabindex !== null && tabindex !== "-1") return true;
  return INTERACTIVE_ROLES.has(element.getAttribute("role") ?? "");
}

/** Whether a press on `path` belongs to the title bar: inside an element
 * with `data-window-drag`, and not on a control within it. `"false"` opts
 * a subtree out. */
export function inDragRegion(path: EventTarget[]): boolean {
  for (const element of path) {
    if (!(element instanceof HTMLElement)) continue;
    const region = element.dataset.windowDrag;
    if (region === "false") return false;
    if (region !== undefined) return true;
    if (isClickable(element)) return false;
  }
  return false;
}

/** Moves and maximizes the window from its title bar regions
 * (`data-window-drag`). Tauri's own `data-tauri-drag-region` starts the
 * move on mousedown, so the window manager takes the pointer and a double
 * click never reaches the page on Linux. Here the move starts only once
 * the pointer travels, and a double click toggles maximize. */
export function useWindowDrag(appWindow: AppWindow | null) {
  useEffect(() => {
    if (!appWindow) return;
    let pressed: { x: number; y: number } | null = null;

    const onMouseDown = (event: MouseEvent) => {
      pressed = null;
      if (event.button !== 0 || !inDragRegion(event.composedPath())) return;
      // No text selection or focus change from the bar.
      event.preventDefault();
      if (event.detail === 2) void appWindow.toggleMaximize();
      else if (event.detail === 1) pressed = { x: event.screenX, y: event.screenY };
    };
    const onMouseMove = (event: MouseEvent) => {
      if (!pressed) return;
      if (!(event.buttons & 1)) {
        pressed = null;
        return;
      }
      const travel = Math.hypot(event.screenX - pressed.x, event.screenY - pressed.y);
      if (travel < DRAG_THRESHOLD) return;
      pressed = null;
      void appWindow.startDragging();
    };
    const onMouseUp = () => {
      pressed = null;
    };

    document.addEventListener("mousedown", onMouseDown);
    document.addEventListener("mousemove", onMouseMove);
    document.addEventListener("mouseup", onMouseUp);
    return () => {
      document.removeEventListener("mousedown", onMouseDown);
      document.removeEventListener("mousemove", onMouseMove);
      document.removeEventListener("mouseup", onMouseUp);
    };
  }, [appWindow]);
}
