/** Files dropped from the desktop go to the drop zone under the pointer:
 * the library installs them, a path field takes the path, Settings → Plugins
 * reviews the folder. A drop outside every zone goes to the fallback, the
 * install confirmation. While files are dragged over a zone it carries
 * `data-drop-active`, which themes may style. */
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
import { effectSubscription } from "../api/subscription";
import { type DropPoint, type DropSource, nativeDropSource } from "./source";

export type DropHandler = (paths: string[]) => void;

interface DropRegistry {
  register(element: HTMLElement, handler: () => DropHandler | null): () => void;
  fallback(handler: DropHandler): () => void;
}

const RegistryContext = createContext<DropRegistry | null>(null);
/** The zone files are dragged over. */
const ActiveContext = createContext<HTMLElement | null>(null);

/** The most paths one drop hands over, like the backend's request queue. */
const MAX_PATHS = 32;

export function DropProvider({
  source = nativeDropSource,
  hitTest = (x, y) => document.elementFromPoint(x, y),
  children,
}: {
  source?: DropSource;
  /** The element at a point; jsdom has no layout. */
  hitTest?: (x: number, y: number) => Element | null;
  children: ReactNode;
}) {
  const zones = useRef(new Map<HTMLElement, () => DropHandler | null>());
  const fallbacks = useRef<DropHandler[]>([]);
  const [active, setActive] = useState<HTMLElement | null>(null);
  // The innermost enabled zone at the point.
  const zoneAt = useCallback(
    ({ x, y }: DropPoint) => {
      let element = hitTest(x, y)?.closest<HTMLElement>("[data-drop-zone]");
      while (element) {
        if (zones.current.get(element)?.()) return element;
        element = element.parentElement?.closest<HTMLElement>("[data-drop-zone]");
      }
      return null;
    },
    [hitTest],
  );
  useEffect(
    () =>
      effectSubscription(source, (event) => {
        if (event.type === "leave") {
          setActive(null);
          return;
        }
        const zone = zoneAt(event.position);
        if (event.type !== "drop") {
          setActive(zone);
          return;
        }
        setActive(null);
        const paths = event.paths.slice(0, MAX_PATHS);
        if (!paths.length) return;
        const handler = zone ? zones.current.get(zone)?.() : fallbacks.current.at(-1);
        handler?.(paths);
      }),
    [source, zoneAt],
  );
  useEffect(() => {
    active?.setAttribute("data-drop-active", "");
    return () => active?.removeAttribute("data-drop-active");
  }, [active]);
  const registry = useMemo<DropRegistry>(
    () => ({
      register(element, handler) {
        zones.current.set(element, handler);
        return () => {
          zones.current.delete(element);
          setActive((current) => (current === element ? null : current));
        };
      },
      fallback(handler) {
        fallbacks.current.push(handler);
        return () => {
          fallbacks.current = fallbacks.current.filter((h) => h !== handler);
        };
      },
    }),
    [],
  );
  return (
    <RegistryContext.Provider value={registry}>
      <ActiveContext.Provider value={active}>{children}</ActiveContext.Provider>
    </RegistryContext.Provider>
  );
}

/** Makes an element a drop zone: put `ref` on it and `onDrop` gets the
 * dropped paths. `active` is true while files are dragged over it. With
 * `null` the zone is off and drops go to the zone around it. */
export function useDropZone<T extends HTMLElement = HTMLElement>(onDrop: DropHandler | null) {
  const registry = useContext(RegistryContext);
  const active = useContext(ActiveContext);
  const handler = useRef(onDrop);
  useEffect(() => {
    handler.current = onDrop;
  });
  const [element, setElement] = useState<T | null>(null);
  useEffect(() => {
    if (!registry || !element) return;
    element.setAttribute("data-drop-zone", "");
    const stop = registry.register(element, () => handler.current);
    return () => {
      stop();
      element.removeAttribute("data-drop-zone");
    };
  }, [registry, element]);
  return { ref: setElement, active: element !== null && active === element };
}

/** Where drops outside every zone go. The latest registration wins. */
export function useDropFallback(onDrop: DropHandler) {
  const registry = useContext(RegistryContext);
  const handler = useRef(onDrop);
  useEffect(() => {
    handler.current = onDrop;
  });
  useEffect(() => registry?.fallback((paths) => handler.current(paths)), [registry]);
}
