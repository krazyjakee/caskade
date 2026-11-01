/** Whether the OS opens magnet links and .torrent files with Caskade (#159,
 * `src-tauri/src/torrent_links.rs`). Linux makes Caskade their default app;
 * Windows only offers it, and the user chooses it in Default apps. */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { createContext, createElement, type ReactNode, useContext } from "react";
import { commands, type TorrentLinks } from "./bindings";
import { unwrap } from "./reply";

export type { TorrentLinks };

export interface TorrentLinksApi {
  status(): Promise<TorrentLinks>;
  /** Asks the OS to open them with Caskade, or hands them back. */
  set(open: boolean): Promise<TorrentLinks>;
  /** Windows: opens Default apps at Caskade's page. */
  choose(): Promise<void>;
}

/** The browser preview has no OS to ask. */
const preview: TorrentLinks = {
  support: "unsupported",
  registered: false,
  magnet: false,
  torrentFiles: false,
};

export const nativeTorrentLinksApi: TorrentLinksApi = {
  status: () => (isTauri() ? unwrap(commands.torrentLinksStatus()) : Promise.resolve(preview)),
  set: (open) => unwrap(commands.torrentLinksSet(open)),
  choose: async () => {
    await unwrap(commands.torrentLinksChoose());
  },
};

const TorrentLinksContext = createContext<TorrentLinksApi>(nativeTorrentLinksApi);

/** Tests supply their own API; the app uses the native one. */
export function TorrentLinksProvider({
  api,
  children,
}: {
  api: TorrentLinksApi;
  children: ReactNode;
}) {
  return createElement(TorrentLinksContext.Provider, { value: api }, children);
}

const torrentLinksKey = ["torrentLinks"] as const;

export function useTorrentLinks() {
  const api = useContext(TorrentLinksContext);
  return useQuery({
    queryKey: torrentLinksKey,
    queryFn: () => api.status(),
    retry: false,
    // The user chooses in the OS's settings, outside Caskade.
    refetchOnWindowFocus: "always",
  });
}

export function useSetTorrentLinks() {
  const api = useContext(TorrentLinksContext);
  const client = useQueryClient();
  return useMutation({
    mutationFn: (open: boolean) => api.set(open),
    onSuccess: (status) => client.setQueryData(torrentLinksKey, status),
  });
}

export function useChooseTorrentLinks() {
  const api = useContext(TorrentLinksContext);
  return useMutation({ mutationFn: () => api.choose() });
}
