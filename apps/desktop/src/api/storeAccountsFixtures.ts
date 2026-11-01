/** Store accounts in memory, for the browser preview and tests: one
 * provider per way of signing in, and accounts in every connection state. */
import type { ErrorReport_Serialize } from "./bindings";
import type {
  OwnedRelease,
  StoreAccountSummary,
  StoreAccountsApi,
  StoreAuthFlow,
  StoreConnectionState,
  StoreProviderCapabilities,
} from "./storeAccounts";

const provider = (
  id: string,
  name: string,
  auth: StoreProviderCapabilities["auth"],
): StoreProviderCapabilities => ({
  id,
  name,
  auth,
  library: true,
  playtime: auth !== "apiKey",
  install: auth === "launcher",
});

export const fixtureProviders: StoreProviderCapabilities[] = [
  provider("steam", "Steam", "apiKey"),
  provider("gog", "GOG", "browser"),
  provider("epic", "Epic Games", "device"),
  provider("itch", "itch.io", "apiKey"),
  provider("lutris", "Lutris", "launcher"),
  // Achievements only: no library.
  {
    ...provider("retroachievements", "RetroAchievements", "apiKey"),
    library: false,
    playtime: false,
  },
];

const HOUR = 3_600_000;

function account(
  id: string,
  providerId: string,
  name: string,
  state: StoreConnectionState,
  extra: Partial<StoreAccountSummary> = {},
): StoreAccountSummary {
  const now = Date.UTC(2026, 9, 30, 9);
  const error: ErrorReport_Serialize | null =
    state === "offline"
      ? {
          code: "offline",
          summary: "the store can't be reached: no route to host",
          detail: null,
          hints: ["checkConnection"],
          transient: true,
          reference: "f1x7",
        }
      : state === "expired"
        ? {
            code: "auth",
            summary: "the sign-in has expired; connect the account again (token revoked)",
            detail: null,
            hints: ["signIn"],
            transient: false,
            reference: "e9x2",
          }
        : null;
  return {
    account: {
      id,
      provider: providerId,
      externalId: `${id}-external`,
      displayName: name,
      ownerId: null,
      enabled: state !== "notConnected",
      sync: {
        lastOkMs: now - 2 * HOUR,
        lastAttemptMs: now - HOUR,
        error,
        failures: error ? 1 : 0,
        retryAtMs: null,
      },
      createdAtMs: now - 200 * HOUR,
    },
    capabilities: fixtureProviders.find((p) => p.id === providerId) ?? null,
    owned: 412,
    lost: state === "connected" ? 2 : 0,
    installed: 37,
    state,
    sessionOnly: false,
    ...extra,
  };
}

/** Owned releases of fixture games `g1` (installable), `g2` (no launcher)
 * and `g3` (another system); entry ids start with their game's id. */
export function fixtureReleases(): OwnedRelease[] {
  const release = (entryId: string, state: OwnedRelease["state"]): OwnedRelease => ({
    entryId,
    storeAccountId: "a1",
    provider: "steam",
    providerName: "Steam",
    accountName: "Ada",
    productId: "620",
    title: "Portal 2",
    state,
    refreshedAtMs: Date.UTC(2026, 9, 30, 7),
  });
  return [
    release("g1-owned", { type: "available", launcher: "Steam" }),
    release("g2-owned", { type: "launcherMissing", launcher: "GOG Galaxy" }),
    release("g3-owned", { type: "unsupportedSystem", systems: ["windows"] }),
  ];
}

/** One account in each connection state. */
export function fixtureAccounts(): StoreAccountSummary[] {
  return [
    account("a1", "steam", "Ada", "connected", { sessionOnly: true }),
    account("a2", "steam", "Kid", "syncing"),
    account("a3", "epic", "Ada Epic", "offline"),
    account("a4", "itch", "ada-itch", "expired"),
    account("a5", "lutris", "Lutris here", "notConnected"),
    account("a6", "origin", "Old Origin", "unsupported", { capabilities: null }),
  ];
}

export function createStoreAccountsFixtureApi(
  accounts: StoreAccountSummary[] = fixtureAccounts(),
  owned: OwnedRelease[] = fixtureReleases(),
): StoreAccountsApi & { calls: string[] } {
  let list = accounts;
  let releases: OwnedRelease[] = owned;
  let flows: StoreAuthFlow[] = [];
  const listeners = new Set<() => void>();
  const calls: string[] = [];
  const changed = () => {
    for (const listener of listeners) listener();
  };
  const set = (id: string, change: (summary: StoreAccountSummary) => StoreAccountSummary) => {
    list = list.map((summary) => (summary.account.id === id ? change(summary) : summary));
    changed();
  };
  return {
    calls,
    providers: async () => fixtureProviders,
    list: async () => list,
    flows: async () => flows,
    begin: async (providerId, reconnect) => {
      calls.push(`begin ${providerId}`);
      const kind = fixtureProviders.find((p) => p.id === providerId)?.auth ?? "browser";
      const flow: StoreAuthFlow = {
        flowId: `flow-${calls.length}`,
        provider: providerId,
        kind,
        challenge: {
          url:
            kind === "browser"
              ? providerId === "gog"
                ? "https://auth.gog.com/auth"
                : "https://store.example/login"
              : kind === "device"
                ? "https://store.example/activate"
                : providerId === "steam"
                  ? "https://steamcommunity.com/dev/apikey"
                  : null,
          userCode: kind === "device" ? "WDJB-MJHT" : null,
          fields:
            providerId === "steam"
              ? [
                  {
                    id: "profile",
                    label: "Steam profile",
                    description: "Your SteamID64, profile link or custom URL name.",
                    secret: false,
                  },
                  { id: "key", label: "Steam Web API key", description: null, secret: true },
                ]
              : [],
          instructions:
            providerId === "steam"
              ? "With a key from another account, your profile's game details must be public."
              : providerId === "gog"
                ? "Sign in on GOG's page, then paste the address it ends on."
                : null,
        },
        expiresAtMs: Date.now() + 600_000,
        reconnect,
      };
      flows = [...flows, flow];
      changed();
      return flow;
    },
    complete: async (flowId, input) => {
      calls.push(`complete ${flowId} ${input.type}`);
      const flow = flows.find((f) => f.flowId === flowId);
      if (!flow) throw new Error("This sign-in has expired or was cancelled. Start again.");
      if (input.type === "code" && input.code === "wrong") {
        throw Object.assign(new Error("the store refused the sign-in: unknown key"), {
          code: "auth",
        });
      }
      flows = flows.filter((f) => f.flowId !== flowId);
      const name =
        input.type === "code"
          ? input.code
          : input.type === "fields"
            ? (input.fields.profile ?? "New account")
            : "New account";
      const added = account(`new-${calls.length}`, flow.provider, name, "connected");
      if (flow.reconnect) set(flow.reconnect, (s) => ({ ...s, state: "connected" }));
      else list = [...list, added];
      changed();
      return added.account;
    },
    cancel: async (flowId) => {
      calls.push(`cancel ${flowId}`);
      flows = flows.filter((f) => f.flowId !== flowId);
      changed();
    },
    refresh: async (id) => {
      calls.push(`refresh ${id}`);
      set(id, (s) => ({ ...s, state: "connected" }));
      return { added: 3, updated: 1, lost: 0, regained: 0, partial: false, merged: 1 };
    },
    cancelRefresh: async (id) => {
      calls.push(`cancelRefresh ${id}`);
    },
    disconnect: async (id) => {
      calls.push(`disconnect ${id}`);
      set(id, (s) => ({
        ...s,
        state: "notConnected",
        account: { ...s.account, enabled: false },
      }));
    },
    removalPreview: async () => ({
      removed: [{ gameId: "g1", title: "Portal" }],
      kept: [{ gameId: "g2", title: "Portal 2" }],
    }),
    remove: async (id) => {
      calls.push(`remove ${id}`);
      list = list.filter((s) => s.account.id !== id);
      changed();
      return { removed: [{ gameId: "g1", title: "Portal" }], kept: [] };
    },
    ownedReleases: async (gameId) => releases.filter((r) => r.entryId.startsWith(gameId)),
    install: async (entryId) => {
      calls.push(`install ${entryId}`);
      releases = releases.map((r) =>
        r.entryId === entryId && r.state.type === "available"
          ? { ...r, state: { type: "installing", launcher: r.state.launcher, sinceMs: Date.now() } }
          : r,
      );
      changed();
      const release = releases.find((r) => r.entryId === entryId);
      if (!release) throw new Error("that release isn't owned by an account");
      return release;
    },
    cancelInstall: async (entryId) => {
      calls.push(`cancelInstall ${entryId}`);
      releases = releases.map((r) =>
        r.entryId === entryId && r.state.type === "installing"
          ? { ...r, state: { type: "available", launcher: r.state.launcher } }
          : r,
      );
      changed();
    },
    playtime: async (gameId) => {
      const reported = gameId.endsWith("1") ? 10 * 3_600_000 : 0;
      return {
        totalMs: reported + 2 * 3_600_000,
        trackedMs: 2 * 3_600_000,
        reportedMs: reported,
        lastPlayedMs: null,
        sources: reported
          ? [
              {
                provider: "steam",
                accountName: "Ada",
                totalMs: 12 * 3_600_000,
                countedMs: reported,
                lastPlayedMs: null,
                observedAtMs: Date.UTC(2026, 9, 30),
              },
            ]
          : [],
      };
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}
