import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createContext, type ReactNode, useContext, useEffect } from "react";
import type { DiscoveryApi, DiscoverySnapshot } from "../../../api/discovery";
import { effectSubscription } from "../../../api/subscription";

const Context = createContext<DiscoveryApi | null>(null);
const discoveryKey = ["discovery", "controller"] as const;
function newest(current: DiscoverySnapshot | undefined, next: DiscoverySnapshot) {
  return current && current.revision >= next.revision ? current : next;
}
export function DiscoveryProvider({ api, children }: { api: DiscoveryApi; children: ReactNode }) {
  const client = useQueryClient();
  useEffect(
    () =>
      effectSubscription(api.subscribe, (snapshot) => {
        client.setQueryData<DiscoverySnapshot>(discoveryKey, (current) =>
          newest(current, snapshot),
        );
      }),
    [api, client],
  );
  return <Context.Provider value={api}>{children}</Context.Provider>;
}
/** The API itself, for work that drives a scan step by step (first-run setup). */
export function useDiscoveryApi() {
  const api = useContext(Context);
  if (!api) throw new Error("DiscoveryProvider is required");
  return api;
}
export function useDiscovery() {
  const api = useDiscoveryApi();
  const client = useQueryClient();
  const query = useQuery({
    queryKey: discoveryKey,
    queryFn: async () => newest(client.getQueryData(discoveryKey), await api.status()),
    refetchInterval: 5000,
  });
  const action = useMutation({
    mutationFn: (
      action:
        | { type: "scan" | "cancel" | "launchers" }
        | { type: "review"; ids: string[]; accept: boolean; revision: number },
    ) =>
      action.type === "review"
        ? api.review(action.ids, action.accept, action.revision)
        : api[action.type](),
    onSuccess: (snapshot) => {
      client.setQueryData<DiscoverySnapshot>(discoveryKey, (current) => newest(current, snapshot));
    },
    onError: () => {
      void client.invalidateQueries({ queryKey: discoveryKey });
    },
  });
  return { ...query, action };
}
