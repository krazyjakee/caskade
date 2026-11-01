import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createContext, useContext, useEffect } from "react";
import {
  nativeRommApi,
  type RommApi,
  type RommDownload,
  type RommOverview,
} from "../../../api/romm";
import { effectSubscription } from "../../../api/subscription";

/** The native API unless a test or preview provides another. */
export const RommApiContext = createContext<RommApi>(nativeRommApi);
export const rommKey = ["romm", "overview"] as const;
export const useRommApi = () => useContext(RommApiContext);

const finished = (download: RommDownload) =>
  download.phase === "done" || download.phase === "cancelled";

/** Newest state per game; finished downloads leave the list, failed ones stay. */
export function mergeDownload(list: RommDownload[], next: RommDownload): RommDownload[] {
  const others = list.filter((download) => download.entryId !== next.entryId);
  return finished(next) ? others : [...others, next];
}

/** Settings, server states, the cache and live download progress. */
export function useRommOverview() {
  const api = useRommApi();
  const client = useQueryClient();
  useEffect(
    () =>
      effectSubscription(api.subscribeDownloads, (download) => {
        client.setQueryData<RommOverview>(rommKey, (old) =>
          old ? { ...old, downloads: mergeDownload(old.downloads, download) } : old,
        );
        if (download.phase === "done" || download.phase === "failed") {
          void client.invalidateQueries({ queryKey: rommKey });
          void client.invalidateQueries({ queryKey: ["library"] });
        }
      }),
    [api, client],
  );
  return useQuery({ queryKey: rommKey, queryFn: () => api.overview() });
}

/** A change that answers with the new overview; library views refresh too. */
export function useRommWrite() {
  const api = useRommApi();
  const client = useQueryClient();
  return useMutation({
    scope: { id: "romm:configuration" },
    mutationFn: (write: (api: RommApi) => Promise<RommOverview>) => write(api),
    onSuccess: (overview) => {
      client.setQueryData(rommKey, overview);
      void client.invalidateQueries({ queryKey: ["library"] });
    },
  });
}
