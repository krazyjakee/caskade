import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createContext, type ReactNode, useContext, useEffect, useRef } from "react";
import { useLocale } from "../locale/LocaleProvider";
import { effectSubscription } from "./subscription";
import type { LibraryApi } from "./types";

const ApiContext = createContext<LibraryApi | null>(null);
export const libraryKeys = {
  all: ["library"] as const,
  games: ["library", "games"] as const,
  game: (id: string) => ["library", "game", id] as const,
  stats: ["library", "stats"] as const,
  sessions: (id?: string) => ["library", "sessions", id ?? "all"] as const,
};
/** Queries and mutations go to the core over IPC, which works without a
 * network. TanStack's default `online` mode would pause every one of them
 * while the OS reports no connection, so Play, Stop and every view would
 * wait for the network to come back. */
export function createLibraryQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { staleTime: 30_000, retry: 1, networkMode: "always" },
      mutations: { networkMode: "always" },
    },
  });
}
export function LibraryProvider({
  api,
  client,
  children,
}: {
  api: LibraryApi;
  client: QueryClient;
  children: ReactNode;
}) {
  useEffect(() => {
    const refetch = () => void client.invalidateQueries({ queryKey: libraryKeys.all });
    return effectSubscription(api.subscribe, refetch, { failed: refetch });
  }, [api, client]);
  // The core translates what it sends (job titles, notifications, errors),
  // so a language switch refetches everything it has already sent (#214).
  const { locale } = useLocale();
  const shown = useRef(locale);
  useEffect(() => {
    if (shown.current === locale) return;
    shown.current = locale;
    void client.invalidateQueries();
  }, [client, locale]);
  return (
    <ApiContext.Provider value={api}>
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    </ApiContext.Provider>
  );
}
export function useLibraryApi() {
  const api = useContext(ApiContext);
  if (!api) throw new Error("LibraryProvider is required");
  return api;
}
/** The library, or null outside a `LibraryProvider` (a view rendered alone). */
export function useOptionalLibraryApi() {
  return useContext(ApiContext);
}
