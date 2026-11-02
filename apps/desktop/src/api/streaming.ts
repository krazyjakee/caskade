/** Game streaming (#262): the devices this one can stream from. A game
 * a paired device offers has a release from source `stream:<device id>`,
 * shown as "Stream from <device>". */
import { t } from "@lingui/core/macro";
import { useQuery } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { createContext, createElement, type ReactNode, useContext } from "react";
import { commands, type StreamHost } from "./bindings";

export type { StreamHost };

export interface StreamingApi {
  /** Every device seen since Caskade started, by name. */
  hosts(): Promise<StreamHost[]>;
}

export const nativeStreamingApi: StreamingApi = {
  hosts: async () => (isTauri() ? commands.streamingHosts() : []),
};

const StreamingApiContext = createContext<StreamingApi>(nativeStreamingApi);

/** Tests supply their own API; the app uses the native one. */
export function StreamingApiProvider({
  api,
  children,
}: {
  api: StreamingApi;
  children: ReactNode;
}) {
  return createElement(StreamingApiContext.Provider, { value: api }, children);
}

export function useStreamHosts() {
  const api = useContext(StreamingApiContext);
  return useQuery({
    queryKey: ["streaming", "hosts"],
    queryFn: () => api.hosts(),
    refetchInterval: 15_000,
  });
}

/** A streamed release's name, by the device it streams from: "Stream from
 * Den". `undefined` for any other source. */
export function streamReleaseName(
  source: string,
  hosts: readonly StreamHost[],
): string | undefined {
  if (!source.startsWith("stream:")) return undefined;
  const id = source.slice("stream:".length).toLowerCase();
  const host = hosts.find((candidate) => candidate.id.toLowerCase() === id);
  if (!host) return t`Stream from another device`;
  const device = host.name;
  return t({ message: `Stream from ${device}`, comment: "{device} is another device's name" });
}
