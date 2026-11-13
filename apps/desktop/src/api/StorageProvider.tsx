import { useQuery, useQueryClient } from "@tanstack/react-query";
import { createContext, type ReactNode, useContext, useEffect, useMemo, useRef } from "react";
import type { RootSummary, StorageApi } from "./storage";
import { effectSubscription } from "./subscription";

const StorageContext = createContext<StorageApi | null>(null);
const empty: ReadonlySet<string> = new Set();
interface Availability {
  games: ReadonlySet<string>;
  entries: ReadonlySet<string>;
}
const AvailabilityContext = createContext<Availability>({ games: empty, entries: empty });
/** Change events that arrive within this long of each other refetch once. */
export const STORAGE_EVENT_DEBOUNCE_MS = 500;
/** A steady stream of events (a long import) still refetches this often. */
export const STORAGE_EVENT_MAX_WAIT_MS = 5000;
/** Catches a drive that goes away while the window keeps focus and nothing
 * else changes. Listing is cheap: the core caches volume probes and manifests. */
const STORAGE_REFETCH_INTERVAL_MS = 60_000;

function unavailableIn(roots: readonly RootSummary[]) {
  const games = new Set<string>();
  const entries = new Set<string>();
  for (const root of roots)
    for (const game of root.games)
      if (!game.available) {
        games.add(game.gameId);
        entries.add(game.entryId);
      }
  return { games, entries, key: JSON.stringify([[...games].sort(), [...entries].sort()]) };
}
export function StorageProvider({ api, children }: { api: StorageApi; children: ReactNode }) {
  const client = useQueryClient();
  // One shared root inventory serves every visible card/entry. Focus catches
  // mount changes even before a scheduled source scan has updated the DB.
  const roots = useQuery({
    queryKey: ["storage", "roots"],
    queryFn: () => api.list(),
    staleTime: 5000,
    refetchOnWindowFocus: true,
    refetchInterval: STORAGE_REFETCH_INTERVAL_MS,
  });
  // Free space changes on every refetch; consumers (every memoized library
  // card) only care which games are unavailable, so keep the value stable
  // until that set changes.
  const previous = useRef<{ key: string; value: Availability } | null>(null);
  const unavailable = useMemo(() => {
    const { games, entries, key } = unavailableIn(roots.data ?? []);
    if (previous.current?.key !== key) previous.current = { key, value: { games, entries } };
    return previous.current.value;
  }, [roots.data]);
  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let burstStart: number | undefined;
    const refetch = () => {
      timer = undefined;
      burstStart = undefined;
      if (!disposed) void client.invalidateQueries({ queryKey: ["storage"] });
    };
    // Trailing debounce: one refetch per burst of library/settings events.
    const schedule = () => {
      if (disposed) return;
      const now = Date.now();
      burstStart ??= now;
      if (timer !== undefined) clearTimeout(timer);
      const wait = Math.min(
        STORAGE_EVENT_DEBOUNCE_MS,
        Math.max(0, burstStart + STORAGE_EVENT_MAX_WAIT_MS - now),
      );
      timer = setTimeout(refetch, wait);
    };
    const stop = api.subscribe ? effectSubscription(api.subscribe, schedule) : undefined;
    return () => {
      disposed = true;
      if (timer !== undefined) clearTimeout(timer);
      stop?.();
    };
  }, [api, client]);
  return (
    <StorageContext.Provider value={api}>
      <AvailabilityContext.Provider value={unavailable}>{children}</AvailabilityContext.Provider>
    </StorageContext.Provider>
  );
}
export const useStorageApi = () => useContext(StorageContext);
/** A managed copy may be unavailable while another launcher remains playable. */
export const useStorageAvailability = (gameId: string) =>
  useContext(AvailabilityContext).games.has(gameId);
export const useUnavailableStorageEntries = () => useContext(AvailabilityContext).entries;
