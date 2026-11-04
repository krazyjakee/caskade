/** Console mode (#148): the same app and data with a different shell. The
 * mode lives on `<html data-mode>`, so the `mode-console` token set and
 * themes follow it; the adapter makes the window fullscreen and remembers
 * the choice. */
import { isTauri } from "@tauri-apps/api/core";
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import { commands, events } from "../../api/bindings";
import { unwrap } from "../../api/reply";
import { effectSubscription } from "../../api/subscription";
import { afterUnsavedChanges } from "../../views/settings/unsavedChanges";

export type Mode = "desktop" | "console";

export interface ConsoleApi {
  /** The mode to start in: the `--console` flag, the setting, gamescope. */
  initial(): Promise<Mode>;
  /** Switches the window (fullscreen, or back to its desktop size) and
   * remembers the choice. */
  set(mode: Mode): Promise<void>;
  /** Mode changes from elsewhere, e.g. a second `caskade --console`. */
  subscribe(listener: (mode: Mode) => void): Promise<() => void>;
}

const toMode = (console: boolean): Mode => (console ? "console" : "desktop");

export const nativeConsoleApi: ConsoleApi = {
  async initial() {
    if (!isTauri()) return previewMode();
    return toMode(await commands.consoleInitialMode());
  },
  async set(mode) {
    if (!isTauri()) {
      sessionStorage.setItem(PREVIEW_KEY, mode);
      return;
    }
    await unwrap(commands.setConsoleMode(mode === "console"));
  },
  async subscribe(listener) {
    if (!isTauri()) return () => {};
    return events.consoleModeEvent.listen(({ payload }) => listener(toMode(payload.active)));
  },
};

/** The browser preview has no window: `?console` or the last choice in
 * this tab decides. */
const PREVIEW_KEY = "caskade.mode";
function previewMode(): Mode {
  if (new URLSearchParams(window.location.search).has("console")) return "console";
  return sessionStorage.getItem(PREVIEW_KEY) === "console" ? "console" : "desktop";
}

export interface ModeState {
  mode: Mode;
  setMode(mode: Mode): void;
  toggle(): void;
}

const ModeContext = createContext<ModeState>({
  mode: "desktop",
  setMode: () => {},
  toggle: () => {},
});

/** The current mode, for views and plugins (`useMode` in the SDK). */
export function useMode(): ModeState {
  return useContext(ModeContext);
}

export function applyMode(mode: Mode, root: HTMLElement = document.documentElement) {
  root.dataset.mode = mode;
}

export function ModeProvider({
  api = nativeConsoleApi,
  initial = "desktop",
  root = document.documentElement,
  children,
}: {
  api?: ConsoleApi;
  /** Read before the first render (`api.initial()`), so nothing flashes. */
  initial?: Mode;
  root?: HTMLElement;
  children: ReactNode;
}) {
  const [mode, setState] = useState<Mode>(initial);

  useEffect(() => applyMode(mode, root), [mode, root]);

  useEffect(() => effectSubscription(api.subscribe, setState), [api]);

  const setMode = useCallback(
    (next: Mode) => {
      // The other shell remounts the view, and Settings' drafts with it.
      afterUnsavedChanges("mode", () => {
        let previous: Mode = next;
        setState((current) => {
          previous = current;
          return next;
        });
        api.set(next).catch((error: unknown) => {
          // The window didn't change, so neither does the shell: a console
          // shell over a desktop window (or the reverse) would strand a pad
          // or a mouse (#160).
          console.warn("console mode not switched", error);
          setState((current) => (current === next ? previous : current));
        });
      });
    },
    [api],
  );
  const state = useMemo(
    () => ({
      mode,
      setMode,
      toggle: () => setMode(mode === "console" ? "desktop" : "console"),
    }),
    [mode, setMode],
  );
  return <ModeContext.Provider value={state}>{children}</ModeContext.Provider>;
}
