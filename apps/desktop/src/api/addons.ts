/** The add-on catalogue (#181): curated plugins and themes, with what each
 * means here (installed, update, compatible), read offline from the last
 * fetch. Plugins and themes only. Images come through the artwork cache. */
import { t } from "@lingui/core/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { createContext, createElement, type ReactNode, useContext } from "react";
import { assetUrl, thumbnailPath } from "./artwork";
import {
  type AddonFailure,
  type AddonOutcome_Serialize,
  type AddonProgress,
  type AddonRecords,
  type AppMode,
  type CatalogueItem_Serialize,
  type CatalogueStatus,
  type CatalogueView_Serialize,
  commands,
  type DeviceAddon_Serialize,
  events,
  type UpdatePreference,
} from "./bindings";
import { lazyApi } from "./lazyApi";
import { unwrap } from "./reply";
import { effectSubscription } from "./subscription";

export type {
  AddonFailure,
  AddonProgress,
  AddonRecords,
  AppMode,
  CatalogueStatus,
  UpdatePreference,
};
/** What became of an install or update. */
export type AddonOutcome = AddonOutcome_Serialize;
export type CatalogueItem = CatalogueItem_Serialize;
export type CatalogueView = CatalogueView_Serialize;

export interface AddonsApi {
  /** The catalogue as last fetched; fetches nothing. */
  catalogue(mode: AppMode): Promise<CatalogueView>;
  /** Fetches it again; on failure the last good one stays and the status
   * says why. */
  refresh(mode: AppMode): Promise<CatalogueView>;
  /** One of an entry's screenshots as a 960px image URL; null when it
   * can't load. */
  media(id: string, index: number): Promise<string | null>;
  /** Installs or updates `id` (#182). A first plugin install comes back for
   * review unless `accept`; a rejected release is refused again unless
   * `retry`. */
  install(id: string, mode: AppMode, accept: boolean, retry: boolean): Promise<AddonOutcome>;
  /** Stops an install while it downloads. */
  cancel(id: string): Promise<boolean>;
  /** Removes an add-on; a plugin keeps its data only when `keepData`. */
  uninstall(id: string, keepData: boolean): Promise<void>;
  records(): Promise<AddonRecords>;
  /** Skips `version` of its updates; null offers every update again. */
  skipUpdate(id: string, version: string | null): Promise<AddonRecords>;
  setUpdatePreference(preference: UpdatePreference): Promise<AddonRecords>;
  subscribeProgress(listener: (progress: AddonProgress) => void): () => void;
  /** Add-ons the user's other devices have and this one doesn't (#245),
   * while Plugins and themes syncs; installing one is the user's choice. */
  fromDevices?(): Promise<DeviceAddon[]>;
}

/** An add-on on another of the user's devices. */
export type DeviceAddon = DeviceAddon_Serialize;

export function createNativeAddons(): AddonsApi {
  const native = () => {
    if (!isTauri()) throw new Error(t`Open Caskade desktop to browse add-ons.`);
  };
  return {
    async catalogue(mode) {
      native();
      return unwrap(commands.addonsCatalogue(mode));
    },
    async fromDevices() {
      native();
      return unwrap(commands.addonsFromDevices());
    },
    async refresh(mode) {
      native();
      return unwrap(commands.addonsCatalogueRefresh(mode));
    },
    async media(id, index) {
      native();
      const cached = await unwrap(commands.addonsMedia(id, index));
      const path = thumbnailPath(cached, 960) ?? thumbnailPath(cached, 320);
      return path ? assetUrl(path) : null;
    },
    async install(id, mode, accept, retry) {
      native();
      return unwrap(commands.addonsInstall(id, mode, accept, retry));
    },
    async cancel(id) {
      native();
      return commands.addonsInstallCancel(id);
    },
    async uninstall(id, keepData) {
      native();
      await unwrap(commands.addonsUninstall(id, keepData));
    },
    async records() {
      native();
      return unwrap(commands.addonsRecords());
    },
    async skipUpdate(id, version) {
      native();
      return unwrap(commands.addonsSkipUpdate(id, version));
    },
    async setUpdatePreference(preference) {
      native();
      return unwrap(commands.addonsSetUpdatePreference(preference));
    },
    subscribeProgress(listener) {
      if (!isTauri()) return () => {};
      return effectSubscription(events.addonProgress.listen, ({ payload }) => listener(payload));
    },
  };
}

/** The preview's fixture reports no progress; subscribing stays synchronous. */
function withProgress(api: Omit<AddonsApi, "subscribeProgress">): AddonsApi {
  return { ...api, subscribeProgress: () => () => {} };
}

const Context = createContext<AddonsApi | null>(null);
let nativeApi: AddonsApi | undefined;

export function AddonsProvider({ api, children }: { api: AddonsApi; children: ReactNode }) {
  return createElement(Context.Provider, { value: api }, children);
}

export function useAddonsApi(): AddonsApi {
  const api = useContext(Context);
  if (api) return api;
  // Dev builds started with `VITE_CASKADE_FIXTURES=1` browse a fixture catalogue.
  nativeApi ??=
    import.meta.env.DEV && import.meta.env.VITE_CASKADE_FIXTURES === "1"
      ? withProgress(
          lazyApi(
            () => import("./addonsFixtures").then((module) => module.createAddonsFixture()),
            [
              "catalogue",
              "refresh",
              "media",
              "install",
              "cancel",
              "uninstall",
              "records",
              "skipUpdate",
              "setUpdatePreference",
            ],
          ),
        )
      : createNativeAddons();
  return nativeApi;
}

export const addonsKey = (mode: AppMode) => ["addons", "catalogue", mode] as const;

/** The catalogue as last fetched (offline too); `useRefreshCatalogue` fetches it again. */
export function useCatalogue(mode: AppMode) {
  const api = useAddonsApi();
  return useQuery({ queryKey: addonsKey(mode), queryFn: () => api.catalogue(mode) });
}

export function useRefreshCatalogue(mode: AppMode) {
  const api = useAddonsApi();
  const client = useQueryClient();
  return useMutation({
    mutationFn: () => api.refresh(mode),
    onSuccess: (view) => client.setQueryData(addonsKey(mode), view),
  });
}

export function useAddonMedia(id: string, index: number) {
  const api = useAddonsApi();
  return useQuery({
    queryKey: ["addons", "media", id, index],
    queryFn: () => api.media(id, index),
    staleTime: Number.POSITIVE_INFINITY,
  });
}

export const addonRecordsKey = ["addons", "records"] as const;

export function useAddonRecords() {
  const api = useAddonsApi();
  return useQuery({ queryKey: addonRecordsKey, queryFn: () => api.records() });
}

/** After an install, update or removal: the catalogue's states, the
 * plugins and themes lists, and the records. */
export function useAddonsChanged() {
  const client = useQueryClient();
  return () =>
    Promise.all([
      client.invalidateQueries({ queryKey: ["addons"] }),
      client.invalidateQueries({ queryKey: ["plugins"] }),
      client.invalidateQueries({ queryKey: ["themes"] }),
    ]);
}
