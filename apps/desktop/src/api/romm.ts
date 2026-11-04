/** RomM servers (#1): the view contract for Settings → Emulators → RomM server.
 * The core signs in, syncs, downloads and caches; passwords and tokens go
 * straight to the OS keychain and never come back to the webview. */
import { t } from "@lingui/core/macro";
import { isTauri } from "@tauri-apps/api/core";
import {
  type CacheEntry,
  type CacheStatus_Serialize,
  type ConnectionReport_Serialize,
  type ConnectionState,
  commands,
  type DownloadProgress_Serialize,
  events,
  type RommAuth,
  type RommCacheSettings,
  type RommGameStatus_Serialize,
  type RommOverview_Serialize,
  type RommPlatformMapping_Serialize,
  type RommServerConfig_Serialize,
  type RommSettings_Serialize,
  type RommSyncReport,
  type ServerStatus_Serialize,
} from "./bindings";
import { unwrap } from "./reply";

export type { CacheEntry, ConnectionState, RommAuth, RommCacheSettings, RommSyncReport };
export type RommOverview = RommOverview_Serialize;
export type RommSettings = RommSettings_Serialize;
export type RommServer = RommServerConfig_Serialize;
export type RommMapping = RommPlatformMapping_Serialize;
export type RommServerStatus = ServerStatus_Serialize;
export type RommCache = CacheStatus_Serialize;
export type RommConnection = ConnectionReport_Serialize;
export type RommDownload = DownloadProgress_Serialize;
export type RommGameStatus = RommGameStatus_Serialize;

export interface RommApi {
  overview(): Promise<RommOverview>;
  /** Replaces the whole section; servers left out are removed with their games. */
  saveSettings(settings: RommSettings): Promise<RommOverview>;
  /** `secret` (password or API token) goes to the keychain; `null` keeps the saved one. */
  saveServer(server: RommServer, secret: string | null): Promise<RommOverview>;
  removeServer(id: string): Promise<RommOverview>;
  testServer(server: RommServer, secret: string | null): Promise<RommConnection>;
  /** One server, or every server with `null`. */
  sync(id: string | null): Promise<RommSyncReport[]>;
  clearCache(): Promise<RommCache>;
  setPinned(entryId: string, pinned: boolean): Promise<RommGameStatus>;
  evict(entryId: string): Promise<boolean>;
  cancelDownload(entryId: string): Promise<boolean>;
  subscribeDownloads(listener: (download: RommDownload) => void): Promise<() => void>;
}

export const syncIntervals = [
  { id: "manual", label: "Only when I ask" },
  { id: "1h", label: "Every hour" },
  { id: "6h", label: "Every 6 hours" },
  { id: "24h", label: "Daily" },
] as const;
export const authKinds = [
  { id: "password", label: "Username & password" },
  { id: "token", label: "API token" },
] as const satisfies readonly { id: RommAuth; label: string }[];

/** A server id the core accepts: lowercase letters, digits and dashes. */
export function serverId(name: string, taken: readonly string[]): string {
  const base =
    name
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 24) || "romm";
  let id = base;
  for (let n = 2; taken.includes(id); n++) id = `${base}-${n}`;
  return id;
}

export const emptyServer = (id: string): RommServer => ({
  id,
  name: "",
  url: "",
  auth: "password",
  username: "",
  allowInvalidCerts: false,
  importAll: true,
  platforms: [],
  collections: [],
  syncOnStartup: true,
  syncInterval: "6h",
});

function native<T>(request: () => Promise<T>): Promise<T> {
  if (!isTauri()) return Promise.reject(new Error(t`Open Caskade desktop to use RomM.`));
  return request();
}

/** The generated commands, as the panel uses them. */
export const nativeRommApi: RommApi = {
  overview: () => native(() => unwrap(commands.rommOverview())),
  saveSettings: (settings) => native(() => unwrap(commands.rommSettingsSet(settings))),
  saveServer: (server, secret) => native(() => unwrap(commands.rommServerSave(server, secret))),
  removeServer: (id) => native(() => unwrap(commands.rommServerRemove(id))),
  testServer: (server, secret) => native(() => unwrap(commands.rommServerTest(server, secret))),
  sync: (id) => native(() => unwrap(commands.rommSync(id))),
  clearCache: () => native(() => unwrap(commands.rommCacheClear())),
  setPinned: (entryId, pinned) => native(() => unwrap(commands.rommSetPinned(entryId, pinned))),
  evict: (entryId) => native(() => unwrap(commands.rommEvict(entryId))),
  cancelDownload: (entryId) => native(() => commands.rommCancelDownload(entryId)),
  subscribeDownloads: async (listener) =>
    isTauri() ? events.rommDownloadChanged.listen(({ payload }) => listener(payload)) : () => {},
};
