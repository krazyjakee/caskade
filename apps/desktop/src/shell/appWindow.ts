import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { createContext, useContext } from "react";
import { commands } from "../api/bindings";

/** What the user can see of the window at the moment. */
export interface WindowPresence {
  visible: boolean;
  minimized: boolean;
  focused: boolean;
}

/** The main window, which has no native title bar: the topbar, the
 * sidebar's brand and the status bar move it (`useWindowDrag`), and the
 * topbar holds its controls. Resizing at the edges and corners stays
 * native (Tauri's undecorated-resize handler on Windows and Linux). */
export interface AppWindow {
  /** `custom`: Caskade draws minimize, maximize and close (Windows, Linux).
   * `overlay`: macOS keeps its traffic lights, over the top of the sidebar. */
  frame: "custom" | "overlay";
  minimize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  close(): Promise<void>;
  isMaximized(): Promise<boolean>;
  /** Hands the pointer to the window manager to move the window; call it
   * while the primary button is down. */
  startDragging(): Promise<void>;
  /** Calls `handler` after every resize; resolves to the unsubscribe. */
  onResized(handler: () => void): Promise<() => void>;
  /** Calls `handler` when the window gains or loses focus; resolves to the
   * unsubscribe. */
  onFocusChanged?(handler: (focused: boolean) => void): Promise<() => void>;
  presence(): Promise<WindowPresence>;
  /** Calls `handler` for each close of the window: its close button, Alt+F4,
   * the tray's Quit. While this listens, Tauri refuses every close and only
   * tells the webview. To let a close through, unsubscribe (and wait for
   * that), then `close()` again: the adapter then decides as usual. Resolves
   * to the unsubscribe. */
  onCloseRequested(handler: () => void): Promise<() => void>;
  /** With `true`, a close that would quit waits: the adapter raises the
   * window and stays in console mode (hiding to the tray goes on). `true`
   * again says the webview heard the closes so far; without that within
   * seconds, the adapter quits anyway. `false` releases and keeps a quit
   * under way; `false` then `true` forgets it. */
  holdClose(held: boolean): Promise<void>;
}

function nativeWindow(): AppWindow | null {
  if (!isTauri()) return null;
  const current = getCurrentWindow();
  return {
    frame: navigator.userAgent.includes("Mac") ? "overlay" : "custom",
    minimize: () => current.minimize(),
    toggleMaximize: () => current.toggleMaximize(),
    close: () => current.close(),
    isMaximized: () => current.isMaximized(),
    startDragging: () => current.startDragging(),
    onResized: (handler) => current.onResized(() => handler()),
    onFocusChanged: (handler) => current.onFocusChanged(({ payload }) => handler(payload)),
    presence: async () => {
      const [visible, minimized, focused] = await Promise.all([
        current.isVisible(),
        current.isMinimized(),
        current.isFocused(),
      ]);
      return { visible, minimized, focused };
    },
    onCloseRequested: (handler) =>
      current.onCloseRequested((event) => {
        // Or Tauri's wrapper destroys the window: past the tray's hide and
        // console mode's restore, and the webview may not destroy it anyway.
        event.preventDefault();
        handler();
      }),
    holdClose: (held) => commands.closeHold(held),
  };
}

/** `null` in the browser preview, which has no window to drive. */
export const AppWindowContext = createContext<AppWindow | null>(nativeWindow());

export function useAppWindow(): AppWindow | null {
  return useContext(AppWindowContext);
}
