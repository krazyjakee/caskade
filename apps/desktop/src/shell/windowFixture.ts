import type { AppWindow, WindowPresence } from "./appWindow";

/** A window for tests: it records calls, maximizing reports a resize like
 * Tauri's does, `focus` tells listeners the window gained or lost focus, and
 * `place` sets what the user can see of it. Like Tauri, it refuses a close
 * while something listens for one and only tells the listeners: `calls` has
 * "close" for a close that went through, "close refused" otherwise. `holds`
 * lists what the webview told the adapter through `holdClose`. */
export function fakeWindow(): AppWindow & {
  calls: string[];
  listeners: Set<() => void>;
  closeListeners: Set<() => void>;
  holds: boolean[];
  focus: (focused: boolean) => void;
  place: (presence: Partial<WindowPresence>) => void;
} {
  let maximized = false;
  let presence: WindowPresence = { visible: true, minimized: false, focused: true };
  const calls: string[] = [];
  const listeners = new Set<() => void>();
  const closeListeners = new Set<() => void>();
  const holds: boolean[] = [];
  const focusListeners = new Set<(focused: boolean) => void>();
  return {
    frame: "custom",
    calls,
    listeners,
    closeListeners,
    holds,
    place: (next) => {
      presence = { ...presence, ...next };
    },
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
      if (closeListeners.size === 0) {
        calls.push("close");
        return;
      }
      calls.push("close refused");
      for (const listener of closeListeners) listener();
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
    presence: async () => presence,
    onCloseRequested: async (handler) => {
      closeListeners.add(handler);
      return () => {
        closeListeners.delete(handler);
      };
    },
    holdClose: async (held) => {
      holds.push(held);
    },
  };
}
