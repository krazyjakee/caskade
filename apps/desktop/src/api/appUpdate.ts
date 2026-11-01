/** Caskade's own updates. The core checks the release feed in the
 * background (`core.appUpdates`) and when asked; installing downloads the
 * signed bundle and writes it over this one, and the new version runs
 * after a restart. Copies that can't replace themselves (a deb, a
 * development build) say why in `unavailable`. The status follows the
 * core's `appUpdateChanged` events. */
import { t } from "@lingui/core/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { createContext, createElement, type ReactNode, useContext, useEffect } from "react";
import appConfig from "../../src-tauri/tauri.conf.json";
import { type AppUpdatePhase, type AppUpdateStatus_Serialize, commands, events } from "./bindings";
import { unwrap } from "./reply";
import { effectSubscription } from "./subscription";

export type AppUpdateStatus = AppUpdateStatus_Serialize;
export type { AppUpdatePhase };

export interface AppUpdateApi {
  status(): Promise<AppUpdateStatus>;
  /** Asks the release feed now; resolves with what it found. */
  check(): Promise<AppUpdateStatus>;
  /** Starts downloading and installing the release the last check found. */
  install(): Promise<AppUpdateStatus>;
  /** Restarts into the installed update. */
  restart(): Promise<void>;
  /** Every status change; returns the unsubscribe. */
  subscribe(listener: (status: AppUpdateStatus) => void): () => void;
}

/** The browser preview has no core to update. */
const previewStatus = (): AppUpdateStatus => ({
  currentVersion: appConfig.version,
  unavailable: t`Open Caskade desktop to update it.`,
  phase: "idle",
  release: null,
  checkedAt: null,
  downloaded: 0,
  total: null,
  error: null,
});

function native<T>(request: () => Promise<T>): Promise<T> {
  if (!isTauri()) return Promise.reject(new Error(previewStatus().unavailable ?? ""));
  return request();
}

export const nativeAppUpdateApi: AppUpdateApi = {
  status: () => (isTauri() ? commands.appUpdateStatus() : Promise.resolve(previewStatus())),
  check: () => native(() => unwrap(commands.appUpdateCheck())),
  install: () => native(() => unwrap(commands.appUpdateInstall())),
  restart: () =>
    native(async () => {
      await unwrap(commands.appUpdateRestart());
    }),
  subscribe: (listener) =>
    isTauri()
      ? effectSubscription(events.appUpdateChanged.listen, ({ payload }) => listener(payload))
      : () => {},
};

const AppUpdateContext = createContext<AppUpdateApi>(nativeAppUpdateApi);

/** Tests and previews supply their own API; the app uses the native one. */
export function AppUpdateProvider({ api, children }: { api: AppUpdateApi; children: ReactNode }) {
  return createElement(AppUpdateContext.Provider, { value: api }, children);
}

export const appUpdateKey = ["appUpdate"] as const;

/** The update status, kept current by the core's events. */
export function useAppUpdate() {
  const api = useContext(AppUpdateContext);
  const client = useQueryClient();
  useEffect(
    () => api.subscribe((status) => client.setQueryData(appUpdateKey, status)),
    [api, client],
  );
  return useQuery({ queryKey: appUpdateKey, queryFn: () => api.status(), retry: false });
}

export type AppUpdateAction = "check" | "install" | "restart";

export function useAppUpdateAction() {
  const api = useContext(AppUpdateContext);
  const client = useQueryClient();
  return useMutation({
    mutationFn: async (action: AppUpdateAction) => {
      if (action === "restart") {
        await api.restart();
        return null;
      }
      return action === "check" ? api.check() : api.install();
    },
    onSuccess: (status) => {
      if (status) client.setQueryData(appUpdateKey, status);
    },
  });
}

/** A release to install: found by the last check, or still on offer after
 * a failed download. */
export function canInstall(status: AppUpdateStatus) {
  return status.release !== null && (status.phase === "available" || status.phase === "failed");
}
