/** Store accounts (#164, #165): connecting stores, their sync health and
 * what they own. Sign-ins stay in the core and the OS keychain: nothing
 * here ever holds a token. The browser preview uses an in-memory fixture. */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { createContext, createElement, type ReactNode, useContext, useEffect } from "react";
import {
  commands,
  events,
  type OwnedInstallState,
  type OwnedRelease_Serialize,
  type PlaytimeBreakdown_Serialize,
  type StoreAccount_Serialize,
  type StoreAccountSummary_Serialize,
  type StoreAuthFlow_Serialize,
  type StoreAuthInput,
  type StoreAuthKind,
  type StoreConnectionState,
  type StoreProviderCapabilities,
  type StoreRefreshOutcome,
  type StoreRemovalPreview,
} from "./bindings";
import { unwrap } from "./reply";
import { createStoreAccountsFixtureApi } from "./storeAccountsFixtures";
import { effectSubscription } from "./subscription";

export type StoreAccountSummary = StoreAccountSummary_Serialize;
export type StoreAccount = StoreAccount_Serialize;
export type StoreAuthFlow = StoreAuthFlow_Serialize;
export type PlaytimeBreakdown = PlaytimeBreakdown_Serialize;
export type OwnedRelease = OwnedRelease_Serialize;
export type {
  OwnedInstallState,
  StoreAuthInput,
  StoreAuthKind,
  StoreConnectionState,
  StoreProviderCapabilities,
  StoreRefreshOutcome,
  StoreRemovalPreview,
};

export interface StoreAccountsApi {
  providers(): Promise<StoreProviderCapabilities[]>;
  list(): Promise<StoreAccountSummary[]>;
  flows(): Promise<StoreAuthFlow[]>;
  /** Starts a sign-in, or (`reconnect`) signing an account in again. */
  begin(provider: string, reconnect: string | null): Promise<StoreAuthFlow>;
  complete(flowId: string, input: StoreAuthInput): Promise<StoreAccount>;
  cancel(flowId: string): Promise<void>;
  refresh(id: string): Promise<StoreRefreshOutcome>;
  cancelRefresh(id: string): Promise<void>;
  disconnect(id: string): Promise<void>;
  removalPreview(id: string): Promise<StoreRemovalPreview>;
  remove(id: string): Promise<StoreRemovalPreview>;
  /** Where a game's play time comes from (#169). */
  playtime(gameId: string): Promise<PlaytimeBreakdown>;
  /** A game's owned releases and whether each installs here (#170). */
  ownedReleases(gameId: string): Promise<OwnedRelease[]>;
  /** Hands an owned release to its launcher. */
  install(entryId: string): Promise<OwnedRelease>;
  cancelInstall(entryId: string): Promise<void>;
  /** Every change; returns the unsubscribe. */
  subscribe(listener: () => void): () => void;
}

export const nativeStoreAccountsApi: StoreAccountsApi = {
  providers: () => commands.storeProviders(),
  list: () => unwrap(commands.storeAccountsList()),
  flows: () => commands.storeAuthFlows(),
  begin: (provider, reconnect) => unwrap(commands.storeConnectBegin(provider, reconnect)),
  complete: (flowId, input) => unwrap(commands.storeConnectComplete(flowId, input)),
  cancel: (flowId) => commands.storeConnectCancel(flowId),
  refresh: (id) => unwrap(commands.storeAccountRefresh(id)),
  cancelRefresh: (id) => commands.storeAccountCancelRefresh(id),
  disconnect: async (id) => {
    await unwrap(commands.storeAccountDisconnect(id));
  },
  removalPreview: (id) => unwrap(commands.storeAccountRemovalPreview(id)),
  remove: (id) => unwrap(commands.storeAccountRemove(id)),
  playtime: (gameId) => unwrap(commands.gamePlaytime(gameId)),
  ownedReleases: (gameId) => unwrap(commands.gameOwnedReleases(gameId)),
  install: (entryId) => unwrap(commands.ownedInstall(entryId)),
  cancelInstall: (entryId) => commands.ownedInstallCancel(entryId),
  subscribe: (listener) => effectSubscription(events.storeAccountsChanged.listen, () => listener()),
};

let preview: StoreAccountsApi | undefined;
function defaultApi(): StoreAccountsApi {
  if (isTauri()) return nativeStoreAccountsApi;
  preview ??= createStoreAccountsFixtureApi();
  return preview;
}

const Context = createContext<StoreAccountsApi | null>(null);

/** Tests and previews supply their own API; the app uses the native one. */
export function StoreAccountsProvider({
  api,
  children,
}: {
  api: StoreAccountsApi;
  children: ReactNode;
}) {
  return createElement(Context.Provider, { value: api }, children);
}

export function useStoreAccountsApi(): StoreAccountsApi {
  return useContext(Context) ?? defaultApi();
}

export const storeAccountsKey = ["storeAccounts"] as const;

/** Providers, accounts and sign-ins under way, kept current by events. */
export function useStoreAccounts() {
  const api = useStoreAccountsApi();
  const client = useQueryClient();
  useEffect(
    () => api.subscribe(() => void client.invalidateQueries({ queryKey: storeAccountsKey })),
    [api, client],
  );
  return useQuery({
    queryKey: storeAccountsKey,
    queryFn: async () => {
      const [providers, accounts, flows] = await Promise.all([
        api.providers(),
        api.list(),
        api.flows(),
      ]);
      return { providers, accounts, flows };
    },
    retry: false,
  });
}

/** Where a game's play time comes from; asked again when its total moves. */
export function useGamePlaytime(gameId: string, totalSeconds: number) {
  const api = useStoreAccountsApi();
  return useQuery({
    queryKey: [...storeAccountsKey, "playtime", gameId, totalSeconds],
    queryFn: () => api.playtime(gameId),
    retry: false,
  });
}

/** A game's owned releases; asked again when its install state moves. */
export function useOwnedReleases(gameId: string, status: string) {
  const api = useStoreAccountsApi();
  const client = useQueryClient();
  useEffect(
    () => api.subscribe(() => void client.invalidateQueries({ queryKey: storeAccountsKey })),
    [api, client],
  );
  return useQuery({
    queryKey: [...storeAccountsKey, "owned", gameId, status],
    queryFn: () => api.ownedReleases(gameId),
    retry: false,
  });
}

/** Runs an action, then refreshes the accounts. */
export function useStoreAccountAction<A, R>(
  action: (api: StoreAccountsApi, argument: A) => Promise<R>,
) {
  const api = useStoreAccountsApi();
  const client = useQueryClient();
  return useMutation({
    mutationFn: (argument: A) => action(api, argument),
    onSettled: () => client.invalidateQueries({ queryKey: storeAccountsKey }),
  });
}
