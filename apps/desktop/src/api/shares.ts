/** One-off shares (#246, #249, #247): a game shared once with someone else
 * through a link that can be stopped and may expire, the people who
 * received it, and receiving a share from its link. Game files come
 * straight from the sharer (and the share's other recipients), never
 * through a relay.
 *
 * The browser preview and tests use the in-memory fixture. */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { createContext, createElement, type ReactNode, useContext, useEffect } from "react";
import {
  type Collection_Serialize,
  commands,
  events,
  type GameHere,
  type JobSnapshot_Serialize,
  type ReceivedShare,
  type ShareContents_Serialize,
  type ShareCreated_Serialize,
  type SharedGame_Serialize,
  type ShareExpiry,
  type ShareInfo_Serialize,
  type ShareItem,
  type ShareReach,
  type ShareRecipient,
  type ShareRecord_Serialize,
  type ShareState,
} from "./bindings";
import { unwrap } from "./reply";
import { createSharesFixtureApi } from "./sharesFixtures";
import { effectSubscription } from "./subscription";

export type ShareRecord = ShareRecord_Serialize;
export type ShareInfo = ShareInfo_Serialize;
export type ShareCreated = ShareCreated_Serialize;
export type ShareContents = ShareContents_Serialize;
export type SharedGame = SharedGame_Serialize;
export type {
  GameHere,
  ReceivedShare,
  ShareExpiry,
  ShareItem,
  ShareReach,
  ShareRecipient,
  ShareState,
};

/** What Share with someone… shares, and what it's called: a game's
 * release, or something smaller (#248: a save version, a web game's offline
 * copy, a ROM, a collection). */
export type ShareTarget =
  | { kind: "game"; entryId: string; title: string }
  | { kind: "item"; item: ShareItem; title: string };

export interface SharesApi {
  /** Shares `target` until `expiry`: the share and its link. */
  share(target: ShareTarget, expiry: ShareExpiry): Promise<ShareCreated>;
  /** Every share this device made, the newest first. */
  list(): Promise<ShareRecord[]>;
  /** A running share's link again. */
  link(id: string): Promise<string>;
  /** A running share's link as a QR code, an SVG document. */
  qr(id: string): Promise<string>;
  /** Stops a share: its link fails at once, also part-way through. */
  stop(id: string): Promise<void>;
  /** Forgets a share that stopped or expired. */
  forget(id: string): Promise<void>;
  /** What a link shares, asked of its sharer; nothing is fetched. */
  inspect(link: string): Promise<ShareInfo>;
  /** Receives the share a link links to: an install job in Activity. */
  receive(link: string, rootId?: string): Promise<JobSnapshot_Serialize>;
  /** Adds the collection a link shares to the signed-in account's
   * collections; nothing downloads. */
  receiveCollection(link: string): Promise<Collection_Serialize>;
  /** Shares received here that arrive or help others receive them. */
  received(): Promise<ReceivedShare[]>;
  /** Whether a received share goes on helping its other recipients once
   * it's here. */
  keepHelping(job: string, on: boolean): Promise<void>;
  /** Called on every change. */
  subscribe(listener: () => void): () => void;
}

const nothing = async () => {};

export const nativeSharesApi: SharesApi = {
  share: (target, expiry) =>
    unwrap(
      target.kind === "game"
        ? commands.shareGame(target.entryId, expiry)
        : commands.shareItem(target.item, expiry),
    ),
  list: () => unwrap(commands.sharesList()),
  link: (id) => unwrap(commands.shareLink(id)),
  qr: (id) => unwrap(commands.shareQr(id)),
  stop: (id) => unwrap(commands.shareStop(id)).then(nothing),
  forget: (id) => unwrap(commands.shareForget(id)).then(nothing),
  inspect: (link) => unwrap(commands.shareInspect(link)),
  receive: (link, rootId) => unwrap(commands.shareReceive(link, rootId ?? null)),
  receiveCollection: (link) => unwrap(commands.shareReceiveCollection(link)),
  received: () => commands.sharesReceived(),
  keepHelping: (job, on) => unwrap(commands.shareKeepHelping(job, on)).then(nothing),
  subscribe: (listener) => effectSubscription(events.sharesChanged.listen, () => listener()),
};

let preview: SharesApi | undefined;
function defaultApi(): SharesApi {
  if (isTauri()) return nativeSharesApi;
  preview ??= createSharesFixtureApi();
  return preview;
}

const SharesContext = createContext<SharesApi | null>(null);

/** Tests and previews supply their own API; the app uses the native one. */
export function SharesProvider({ api, children }: { api: SharesApi; children: ReactNode }) {
  return createElement(SharesContext.Provider, { value: api }, children);
}

export function useSharesApi(): SharesApi {
  return useContext(SharesContext) ?? defaultApi();
}

export const sharesKey = ["shares"] as const;

/** Refetches the share queries whenever the core says something changed. */
function useSharesRefresh(api: SharesApi) {
  const client = useQueryClient();
  useEffect(
    () => api.subscribe(() => void client.invalidateQueries({ queryKey: sharesKey })),
    [api, client],
  );
}

/** Whether a share is under way: progress moves without an event for
 * every byte, so these refresh every two seconds meanwhile. */
const busy = (share: ShareRecord) =>
  share.state.state === "preparing" ||
  (share.state.state === "ready" && share.recipients.some((r) => r.connected && !r.finished));

export function useShareList() {
  const api = useSharesApi();
  useSharesRefresh(api);
  return useQuery({
    queryKey: [...sharesKey, "list"],
    queryFn: () => api.list(),
    retry: false,
    refetchInterval: (query) => ((query.state.data ?? []).some(busy) ? 2000 : false),
  });
}

export function useReceivedShares() {
  const api = useSharesApi();
  useSharesRefresh(api);
  return useQuery({
    queryKey: [...sharesKey, "received"],
    queryFn: () => api.received(),
    retry: false,
  });
}

/** A running share's QR code, as an image's address. */
export function useShareQr(id: string | undefined) {
  const api = useSharesApi();
  return useQuery({
    queryKey: [...sharesKey, "qr", id],
    queryFn: async () => `data:image/svg+xml;utf8,${encodeURIComponent(await api.qr(id ?? ""))}`,
    enabled: id !== undefined,
    retry: false,
    staleTime: Number.POSITIVE_INFINITY,
  });
}

/** Runs a share action, then refreshes what it changed. */
export function useShareAction<A, R>(action: (api: SharesApi, argument: A) => Promise<R>) {
  const api = useSharesApi();
  const client = useQueryClient();
  return useMutation({
    mutationFn: (argument: A) => action(api, argument),
    onSettled: () => client.invalidateQueries({ queryKey: sharesKey }),
  });
}

/** Whether a share's link works now. */
export const shareRunning = (share: ShareRecord) =>
  share.state.state === "preparing" || share.state.state === "ready";
