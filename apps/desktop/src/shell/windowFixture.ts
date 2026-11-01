import type { AppWindow } from "./appWindow";

/** A window for tests: it records calls, maximizing reports a resize like
 * Tauri's does, and `focus` tells listeners the window gained or lost
 * focus. */
export function fakeWindow(): AppWindow & {
  calls: string[];
  listeners: Set<() => void>;
  focus: (focused: boolean) => void;
} {
  let maximized = false;
  const calls: string[] = [];
  const listeners = new Set<() => void>();
  const focusListeners = new Set<(focused: boolean) => void>();
  return {
    frame: "custom",
    calls,
    listeners,
    focus: (focused) => {
      for (const listener of focusListeners) listener(focused);
    },
    minimize: async () => {
      calls.push("minimize");
    },
    toggleMaximize: async () => {
      calls.push("toggleMaximize");
      maximized = !maximized;
      for (const listener of listeners) listener();
    },
    close: async () => {
      calls.push("close");
    },
    isMaximized: async () => maximized,
    startDragging: async () => {
      calls.push("startDragging");
    },
    onResized: async (handler) => {
      listeners.add(handler);
      return () => {
        listeners.delete(handler);
      };
    },
    onFocusChanged: async (handler) => {
      focusListeners.add(handler);
      return () => {
        focusListeners.delete(handler);
      };
    },
  };
}
