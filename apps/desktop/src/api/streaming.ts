/** Game streaming (#262–#265): the devices this one can stream from, and
 * hosting, which lets other devices stream from this one. A game a paired
 * device offers has a release from source `stream:<device id>`, shown as
 * "Stream from <device>". */
import { t } from "@lingui/core/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { createContext, createElement, type ReactNode, useContext } from "react";
import {
  commands,
  events,
  type HostOutcome,
  type HostReach,
  type HostState,
  type HostStatus,
  type PairedClient,
  type PairingOutcome,
  type PairingStarted,
  type PairRequest,
  type StreamHost,
  type StreamingPart,
  type StreamingProgram,
} from "./bindings";
import { unwrap } from "./reply";
import { objectReply, type SettingsSnapshot } from "./settings";
import { createStreamingFixtureApi } from "./streamingFixtures";

export type {
  HostOutcome,
  HostReach,
  HostState,
  HostStatus,
  PairedClient,
  PairingOutcome,
  PairingStarted,
  PairRequest,
  StreamHost,
  StreamingPart,
  StreamingProgram,
};

/** How a pairing this device asked for ended. */
export interface PairingFinished {
  hostId: string;
  outcome: PairingOutcome;
  /** Why it failed, in Caskade's words. */
  message?: string | null;
}

/** Each part `null` is chosen automatically. */
export interface StreamQuality {
  resolution: string | null;
  fps: number | null;
  bitrateKbps: number | null;
}

export interface StreamingApi {
  /** Every device seen since Caskade started, by name. */
  hosts(): Promise<StreamHost[]>;
  /** Adds a device by its address and looks for it at once. */
  addHost(address: string): Promise<StreamHost[]>;
  /** Starts pairing with a device: the code to enter on it. */
  pairWith(hostId: string): Promise<PairingStarted>;
  /** Hears how pairings end. */
  onPairingFinished(listener: (finished: PairingFinished) => void): () => void;
  forgetHost(hostId: string): Promise<void>;
  /** Hosting on this device. */
  hostStatus(): Promise<HostStatus>;
  setHosting(on: boolean): Promise<HostOutcome>;
  /** Sets how games streamed to this device look: any account may (#66). */
  setQuality(quality: StreamQuality): Promise<SettingsSnapshot>;
  /** Hosts through the streaming already set up here, with its sign-in. */
  hostThroughExisting(user: string, password: string): Promise<HostOutcome>;
  /** Devices waiting to pair with this one. */
  pairRequests(): Promise<PairRequest[]>;
  acceptPairing(id: string, code: string, name: string): Promise<void>;
  declinePairing(id: string): Promise<void>;
  /** Devices paired with this one, which may stream from it. */
  pairedDevices(): Promise<PairedClient[]>;
  unpair(uuid: string): Promise<void>;
  /** Forgets every paired device: each needs a code again. */
  unpairAll(): Promise<void>;
  /** Re-runs hosting's setup: one administrator prompt at most. */
  repair(): Promise<HostOutcome>;
  /** The streaming programs installed here, with updates waiting. */
  programs(): Promise<StreamingProgram[]>;
  /** Installs the newest version; never during a stream. */
  update(part: StreamingPart): Promise<HostOutcome>;
  /** Stops offering the newest version. */
  skip(part: StreamingPart): Promise<void>;
}

const nothing = async () => {};

export const nativeStreamingApi: StreamingApi = {
  hosts: () => commands.streamingHosts(),
  addHost: (address) => unwrap(commands.streamingAddHost(address)),
  pairWith: (hostId) => unwrap(commands.streamingPairWith(hostId)),
  onPairingFinished: (listener) => {
    let stop: (() => void) | undefined;
    let stopped = false;
    void events.streamingPairingFinished
      .listen(({ payload }) => listener(payload))
      .then((unlisten) => {
        if (stopped) unlisten();
        else stop = unlisten;
      });
    return () => {
      stopped = true;
      stop?.();
    };
  },
  forgetHost: (hostId) => unwrap(commands.streamingForgetHost(hostId)).then(nothing),
  hostStatus: async () => commands.streamingHostStatus(),
  setHosting: (on) => unwrap(commands.streamingSetHosting(on)),
  setQuality: (quality) => unwrap(commands.streamingSetQuality(quality).then(objectReply)),
  hostThroughExisting: (user, password) => unwrap(commands.streamingUseExisting(user, password)),
  pairRequests: () => unwrap(commands.streamingPairRequests()),
  acceptPairing: (id, code, name) =>
    unwrap(commands.streamingAcceptPairing(id, code, name)).then(nothing),
  declinePairing: (id) => unwrap(commands.streamingDeclinePairing(id)).then(nothing),
  pairedDevices: () => unwrap(commands.streamingPairedDevices()),
  unpair: (uuid) => unwrap(commands.streamingUnpair(uuid)).then(nothing),
  unpairAll: () => unwrap(commands.streamingUnpairAll()).then(nothing),
  repair: () => unwrap(commands.streamingRepair()),
  programs: () => unwrap(commands.streamingPrograms()),
  update: (part) => unwrap(commands.streamingUpdate(part)),
  skip: (part) => unwrap(commands.streamingSkip(part)).then(nothing),
};

let preview: StreamingApi | undefined;
/** The core in the app; a fixture device in the browser preview. */
function defaultApi(): StreamingApi {
  if (isTauri()) return nativeStreamingApi;
  preview ??= createStreamingFixtureApi();
  return preview;
}

const StreamingApiContext = createContext<StreamingApi | null>(null);

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

export function useStreamingApi(): StreamingApi {
  return useContext(StreamingApiContext) ?? defaultApi();
}

export const streamingKey = ["streaming"] as const;

export function useStreamHosts() {
  const api = useStreamingApi();
  return useQuery({
    queryKey: [...streamingKey, "hosts"],
    queryFn: () => api.hosts(),
    refetchInterval: 15_000,
  });
}

/** Hosting's state, often while it changes. */
export function useHostStatus({ enabled = true } = {}) {
  const api = useStreamingApi();
  return useQuery({
    queryKey: [...streamingKey, "host"],
    queryFn: () => api.hostStatus(),
    enabled,
    refetchInterval: (query) =>
      query.state.data?.state === "settingUp" || query.state.data?.state === "starting"
        ? 1_000
        : 5_000,
  });
}

/** Devices waiting to pair, while this device hosts. */
export function usePairRequests({ enabled = true } = {}) {
  const api = useStreamingApi();
  return useQuery({
    queryKey: [...streamingKey, "requests"],
    queryFn: () => api.pairRequests(),
    enabled,
    refetchInterval: 3_000,
  });
}

export function usePairedDevices({ enabled = true } = {}) {
  const api = useStreamingApi();
  return useQuery({
    queryKey: [...streamingKey, "paired"],
    queryFn: () => api.pairedDevices(),
    enabled,
  });
}

/** The streaming programs installed here (#266). */
export function useStreamingPrograms() {
  const api = useStreamingApi();
  return useQuery({
    queryKey: [...streamingKey, "programs"],
    queryFn: () => api.programs(),
  });
}

/** Runs a streaming action, then refreshes what it changed. */
export function useStreamingAction<A, R>(action: (api: StreamingApi, argument: A) => Promise<R>) {
  const api = useStreamingApi();
  const client = useQueryClient();
  return useMutation({
    mutationFn: (argument: A) => action(api, argument),
    onSettled: () => client.invalidateQueries({ queryKey: streamingKey }),
  });
}

/** A device's state, for people: "Ready", "Asleep", … */
export function reachLabel(reach: HostReach): string {
  switch (reach) {
    case "ready":
      return t({ message: "Ready", context: "streaming device" });
    case "busy":
      return t({ message: "Playing another game", context: "streaming device" });
    case "asleep":
      return t({ message: "Asleep: it wakes when you stream", context: "streaming device" });
    case "unreachable":
      return t({ message: "Can't be reached", context: "streaming device" });
  }
}

/** A streamed release's name, by the device it streams from: "Stream from
 * Den". `undefined` for any other source. */
export function streamReleaseName(
  source: string,
  hosts: readonly StreamHost[],
): string | undefined {
  if (!source.startsWith("stream:")) return undefined;
  const host = streamHost(source, hosts);
  if (!host) return t`Stream from another device`;
  const device = host.name;
  return t({ message: `Stream from ${device}`, comment: "{device} is another device's name" });
}

/** The device a `stream:` release streams from, when known. */
export function streamHost(source: string, hosts: readonly StreamHost[]): StreamHost | undefined {
  if (!source.startsWith("stream:")) return undefined;
  const id = source.slice("stream:".length).toLowerCase();
  return hosts.find((candidate) => candidate.id.toLowerCase() === id);
}

/** A streamed release's device state when it isn't simply ready: shown
 * beside "Stream from …". */
export function streamReleaseState(
  source: string,
  hosts: readonly StreamHost[],
): string | undefined {
  const host = streamHost(source, hosts);
  return host && host.reach !== "ready" ? reachLabel(host.reach) : undefined;
}
