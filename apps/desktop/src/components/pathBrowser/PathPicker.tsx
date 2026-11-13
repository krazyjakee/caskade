/** The one way to ask for a file or folder: the native dialog in desktop
 * mode, the in-app `FolderBrowser` in console mode, where a pad has to
 * drive it. Mounted inside each `InputProvider` (the app shell, first-run
 * setup), so the browser's pad bindings reach it. */
import { t } from "@lingui/core/macro";
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { type BrowseApi, BrowseApiContext } from "../../api/browse";
import { nativeBrowseApi } from "../../api/browseNative";
import { useMode } from "../../shell/console/mode";
import { FolderBrowser, type PathRequest } from "./FolderBrowser";

export type { PathRequest };

interface PathPicker {
  /** Console mode: `pick` shows the in-app browser. A caller whose native
   * picker returns several paths checks this to take one from the browser. */
  inApp: boolean;
  /** A path, or null when cancelled. `native` is the desktop picker to use
   * (an API's own); the dialog plugin's without one. */
  pick(request: PathRequest, native?: () => Promise<string | null>): Promise<string | null>;
}

/** The dialog plugin's picker. */
async function nativePick(request: PathRequest): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    directory: request.kind === "folder",
    multiple: false,
    title: request.title,
    defaultPath: request.startAt ?? undefined,
    filters:
      request.kind === "file" && request.extensions?.length
        ? [
            {
              name: t({ message: "Files", comment: "A file type filter in the file dialog" }),
              extensions: [...request.extensions],
            },
          ]
        : undefined,
  });
  return typeof selected === "string" ? selected : null;
}

const desktop: PathPicker = {
  inApp: false,
  pick: (request, native) => (native ? native() : nativePick(request)),
};

const Context = createContext<PathPicker | null>(null);

/** The picker for this mode; outside a `PathPickerProvider`, the native one. */
export function usePathPicker(): PathPicker {
  return useContext(Context) ?? desktop;
}

interface Pending {
  id: number;
  request: PathRequest;
  resolve: (path: string | null) => void;
}

export function PathPickerProvider({ children }: { children: ReactNode }) {
  const consoleMode = useMode().mode === "console";
  const api: BrowseApi = useContext(BrowseApiContext) ?? nativeBrowseApi;
  const [pending, setPending] = useState<Pending | null>(null);
  const latest = useRef<Pending | null>(null);
  const ids = useRef(0);
  const settle = useCallback((path: string | null) => {
    const current = latest.current;
    latest.current = null;
    setPending(null);
    current?.resolve(path);
  }, []);
  // A picker still open when this unmounts (setup finished) is cancelled.
  useEffect(() => () => latest.current?.resolve(null), []);
  const picker = useMemo<PathPicker>(
    () =>
      consoleMode
        ? {
            inApp: true,
            pick: (request) =>
              new Promise((resolve) => {
                // A second request replaces the first, which is cancelled.
                latest.current?.resolve(null);
                const next = { id: ++ids.current, request, resolve };
                latest.current = next;
                setPending(next);
              }),
          }
        : desktop,
    [consoleMode],
  );
  return (
    <Context.Provider value={picker}>
      {children}
      {pending && (
        <FolderBrowser key={pending.id} request={pending.request} api={api} onClose={settle} />
      )}
    </Context.Provider>
  );
}
