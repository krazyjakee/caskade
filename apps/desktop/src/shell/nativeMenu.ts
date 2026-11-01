/** WebKit's own context menu (Reload, Open Link in New Window, Save Image
 * As…) does nothing useful in the app, or breaks it, so a right click shows
 * only Caskade's menus (`ContextMenu` from `@caskade/ui`). Text fields keep
 * it for Cut, Copy and Paste, and selected text keeps it for Copy. Debug
 * builds show it on Shift+right click, for Inspect Element. */

const textInput =
  'input:not([type="checkbox"], [type="radio"], [type="range"], [type="button"], [type="submit"], [type="reset"], [type="file"], [type="color"], [type="image"], [type="hidden"])';
const editable = `${textInput}, textarea, [contenteditable]:not([contenteditable="false"])`;

/** `tauri dev`, `tauri build --debug` and the browser preview. */
const debugBuild = import.meta.env.DEV || import.meta.env.TAURI_ENV_DEBUG === "true";

/** Turns WebKit's menu off; returns the undo. Caskade's menus still open:
 * they handle the event after this listener. */
export function suppressNativeContextMenu(target: Window = window, debug = debugBuild) {
  const listener = (event: MouseEvent) => {
    if (debug && event.shiftKey) return;
    if (event.target instanceof Element && event.target.closest(editable)) return;
    if (!(target.getSelection()?.isCollapsed ?? true)) return;
    event.preventDefault();
  };
  target.addEventListener("contextmenu", listener, { capture: true });
  return () => target.removeEventListener("contextmenu", listener, { capture: true });
}
