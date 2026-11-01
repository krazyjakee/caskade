import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { createContext, useContext } from "react";

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
  };
}

/** `null` in the browser preview, which has no window to drive. */
export const AppWindowContext = createContext<AppWindow | null>(nativeWindow());

export function useAppWindow(): AppWindow | null {
  return useContext(AppWindowContext);
}
