/** This device and the devices paired with it (#226, #231): how each is
 * reached, pairing by code or link, forgetting, and library sync's state
 * per device (#230). Never use relays and what syncs are settings
 * (`core.sync`, through the settings API). The status follows the core's
 * `devicesChanged` events; a `caskade://pair` link opened from outside
 * arrives as `pairingLinkOpened` and waits for an admin in the app.
 *
 * The browser preview and tests use the in-memory fixture. */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { createContext, createElement, type ReactNode, useContext, useEffect } from "react";
import {
  commands,
  type DeviceKind,
  type DeviceLink,
  type DeviceOs,
  type DevicesStatus_Serialize,
  events,
  type IdentityStatus,
  type NearbyDevice,
  type PairedDevice_Serialize,
  type Pairing,
  type PairingLinkInfo,
  type PairingLinkTarget,
  type PairingState,
  type PeerCopy_Serialize,
  type ThisDevice_Serialize,
} from "./bindings";
import { unwrap } from "./reply";
import { effectSubscription } from "./subscription";
import { createSyncFixtureApi } from "./syncFixtures";

export type DevicesStatus = DevicesStatus_Serialize;
export type ThisDevice = ThisDevice_Serialize;
export type PairedDevice = PairedDevice_Serialize;
/** A game's copy on another of the user's devices (#232). */
export type PeerCopy = PeerCopy_Serialize;
/** How a device is reached right now. */
export type PeerLink = DeviceLink;
export type {
  DeviceKind,
  DeviceOs,
  IdentityStatus,
  NearbyDevice,
  Pairing,
  PairingLinkInfo,
  PairingLinkTarget,
  PairingState,
};

/** A copy of a game between two of the user's devices (#233). */
export interface Transfer {
  id: string;
  title: string;
  deviceName: string;
  direction: "incoming" | "outgoing";
  doneBytes: number;
  totalBytes: number;
  bytesPerSecond: number;
  link: PeerLink;
  /** `waitingDirect`: only a relay reaches the device, and game files wait
   * for a direct connection. */
  state: "running" | "waitingDirect" | "done" | "failed";
}

/** A game shared once with someone else (#246). */
export interface Share {
  id: string;
  title: string;
  createdAtMs: number;
  expiresAtMs: number | null;
  sizeBytes: number;
  recipients: number;
}

export interface SyncApi {
  status(): Promise<DevicesStatus>;
  /** Renames this device; empty goes back to the host name. */
  rename(name: string): Promise<void>;
  setKind(kind: DeviceKind): Promise<void>;
  /** Starts pairing with a nearby device: both show the code. */
  pair(deviceId: string): Promise<Pairing>;
  confirmPairing(deviceId: string): Promise<void>;
  /** Cancels a pairing, or dismisses a finished one. */
  cancelPairing(deviceId: string): Promise<void>;
  /** A link that pairs another device with this one from anywhere, once. */
  pairingLink(): Promise<PairingLinkInfo>;
  acceptPairingLink(link: string): Promise<PairedDevice>;
  /** The `caskade://pair` link opened from outside, if one waits. */
  openedLink(): Promise<PairingLinkTarget | null>;
  acceptOpenedLink(): Promise<PairedDevice>;
  dismissOpenedLink(): Promise<void>;
  forget(deviceId: string): Promise<void>;
  syncNow(deviceId: string): Promise<void>;
  /** A game's copies on the other devices, best reached first. */
  copies(gameId: string): Promise<PeerCopy[]>;
  /** Copies a game from another device to this computer: an install job
   * in Activity (#233). */
  copyHere(gameId: string, deviceId: string): Promise<void>;
  /** Asks a device to copy a game from this computer; it confirms there
   * unless it accepts copies without asking (#235). */
  sendTo(gameId: string, deviceId: string): Promise<void>;
  /** After the keychain lost this device's key: a new identity. */
  createIdentity(): Promise<void>;
  retryIdentity(): Promise<void>;
  /** Copies between devices (#233) and shares (#246): none until those
   * exist, so the app shows nothing for them. */
  transfers(): Promise<Transfer[]>;
  shares(): Promise<Share[]>;
  /** Called on every change, with a pairing link opened from outside. */
  subscribe(listener: (opened?: PairingLinkTarget) => void): () => void;
}

const nothing = async () => {};

export const nativeSyncApi: SyncApi = {
  status: () => unwrap(commands.devicesStatus()),
  rename: (name) => unwrap(commands.devicesRename(name)).then(nothing),
  setKind: (kind) => unwrap(commands.devicesSetKind(kind)).then(nothing),
  pair: (id) => unwrap(commands.devicesPairNearby(id)),
  confirmPairing: (id) => unwrap(commands.devicesConfirmPairing(id)).then(nothing),
  cancelPairing: (id) => unwrap(commands.devicesCancelPairing(id)).then(nothing),
  pairingLink: () => unwrap(commands.devicesPairingLink()),
  acceptPairingLink: (link) => unwrap(commands.devicesAcceptPairingLink(link)),
  openedLink: () => unwrap(commands.devicesOpenedLink()),
  acceptOpenedLink: () => unwrap(commands.devicesAcceptOpenedLink()),
  dismissOpenedLink: () => unwrap(commands.devicesDismissOpenedLink()).then(nothing),
  forget: (id) => unwrap(commands.devicesForget(id)).then(nothing),
  syncNow: (id) => unwrap(commands.devicesSyncNow(id)).then(nothing),
  copies: (gameId) => unwrap(commands.gameOtherDevices(gameId)),
  copyHere: (gameId, deviceId) =>
    unwrap(commands.gameCopyHere(gameId, deviceId, null)).then(nothing),
  sendTo: (gameId, deviceId) => unwrap(commands.gameSendTo(gameId, deviceId)).then(nothing),
  createIdentity: () => unwrap(commands.devicesCreateIdentity()).then(nothing),
  retryIdentity: () => unwrap(commands.devicesRetryIdentity()).then(nothing),
  transfers: () => unwrap(commands.devicesTransfers()),
  shares: async () => [],
  subscribe: (listener) => {
    const changes = effectSubscription(events.devicesChanged.listen, () => listener());
    const opened = effectSubscription(events.pairingLinkOpened.listen, ({ payload }) =>
      listener(payload),
    );
    return () => {
      changes();
      opened();
    };
  },
};

let preview: SyncApi | undefined;
function defaultApi(): SyncApi {
  if (isTauri()) return nativeSyncApi;
  preview ??= createSyncFixtureApi();
  return preview;
}

const SyncContext = createContext<SyncApi | null>(null);

/** Tests and previews supply their own API; the app uses the native one. */
export function SyncProvider({ api, children }: { api: SyncApi; children: ReactNode }) {
  return createElement(SyncContext.Provider, { value: api }, children);
}

export function useSyncApi(): SyncApi {
  return useContext(SyncContext) ?? defaultApi();
}

/** The API when there is one for real: given by a provider, or the core.
 * The status bar's entry shows only then, not in a bare browser preview. */
export function useProvidedSyncApi(): SyncApi | null {
  return useContext(SyncContext) ?? (isTauri() ? nativeSyncApi : null);
}

export const syncKey = ["sync"] as const;

/** Refetches the sync queries whenever the core says something changed. */
function useSyncRefresh(api: SyncApi) {
  const client = useQueryClient();
  useEffect(
    () => api.subscribe(() => void client.invalidateQueries({ queryKey: syncKey })),
    [api, client],
  );
}

function useSyncQuery<T>(key: readonly unknown[], fetch: (api: SyncApi) => Promise<T>) {
  const api = useSyncApi();
  useSyncRefresh(api);
  return useQuery({ queryKey: [...syncKey, ...key], queryFn: () => fetch(api), retry: false });
}

export const useSyncStatus = () => useSyncQuery(["status"], (api) => api.status());
export const useOpenedLink = () => useSyncQuery(["opened"], (api) => api.openedLink());
export function useTransfers() {
  const api = useSyncApi();
  useSyncRefresh(api);
  // Progress moves on its own: refreshed while copies go on.
  return useQuery({
    queryKey: [...syncKey, "transfers"],
    queryFn: () => api.transfers(),
    retry: false,
    refetchInterval: (query) => ((query.state.data?.length ?? 0) > 0 ? 2000 : false),
  });
}
export const useShares = () => useSyncQuery(["shares"], (api) => api.shares());
export const usePeerCopies = (gameId: string) =>
  useSyncQuery(["copies", gameId], (api) => api.copies(gameId));

/** Runs a sync action, then refreshes what it changed. */
export function useSyncAction<A, R>(action: (api: SyncApi, argument: A) => Promise<R>) {
  const api = useSyncApi();
  const client = useQueryClient();
  return useMutation({
    mutationFn: (argument: A) => action(api, argument),
    onSettled: () => client.invalidateQueries({ queryKey: syncKey }),
  });
}

/** The paired devices reachable now, best link first. */
export function reachable(devices: PairedDevice[]): PairedDevice[] {
  const order: PeerLink[] = ["lan", "direct", "relay"];
  return devices
    .filter((device) => device.link !== "offline")
    .sort((a, b) => order.indexOf(a.link) - order.indexOf(b.link));
}
