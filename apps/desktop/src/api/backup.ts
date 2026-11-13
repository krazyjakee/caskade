/** Backups of Caskade's own data (#191–#193): Back up now, automatic
 * backups (`core.backups`), checking a backup and restoring it. A restore
 * is staged, then applied when Caskade restarts. The status follows the
 * core's `backupChanged` events. The browser preview has no core: it uses
 * an in-memory fixture. */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import {
  type BackupComponent,
  type BackupExclusion,
  type BackupInspection_Serialize,
  type BackupJobKind,
  type BackupPhase,
  type BackupPlan_Serialize,
  type BackupStatus_Serialize,
  commands,
  type DeviceBackups,
  events,
  type HeldBackup,
  type RestoreJournal_Serialize,
} from "./bindings";
import { injectable } from "./injectable";
import { unwrap } from "./reply";
import { effectSubscription } from "./subscription";

export type BackupStatus = BackupStatus_Serialize;
export type BackupPlan = BackupPlan_Serialize;
export type BackupInspection = BackupInspection_Serialize;
type RestoreJournal = RestoreJournal_Serialize;

export type {
  BackupComponent,
  BackupExclusion,
  BackupJobKind,
  BackupPhase,
  DeviceBackups,
  HeldBackup,
};

/** The components a backup can hold besides the library, in the order
 * Settings lists them. */
export const optionalComponents: BackupComponent[] = [
  "artwork",
  "downloadedArtwork",
  "plugins",
  "pluginData",
  "themes",
];

export interface BackupApi {
  status(): Promise<BackupStatus>;
  /** What a backup would hold; `destination` null for the settings' folder. */
  plan(destination: string | null, components: BackupComponent[]): Promise<BackupPlan>;
  /** Back up now; the status follows it. */
  start(): Promise<void>;
  cancel(): Promise<void>;
  inspect(path: string): Promise<BackupInspection>;
  /** Stages a restore for the restart. */
  restore(path: string, components: BackupComponent[]): Promise<RestoreJournal>;
  discardRestore(): Promise<void>;
  /** Restarts Caskade to apply the staged restore. */
  restart(): Promise<void>;
  /** Shows a backup in the file manager. */
  reveal(path: string): Promise<void>;
  subscribe(listener: (status: BackupStatus) => void): () => void;
  /** Backups the user's other devices keep (#243), as each last said. */
  onDevices?(): Promise<DeviceBackups[]>;
  /** Fetches one of them here; resolves to its path, to inspect and
   * restore. */
  fetchFromDevice?(hash: string): Promise<string>;
}

const nativeBackupApi: BackupApi = {
  status: () => unwrap(commands.backupStatus()),
  plan: (destination, components) => unwrap(commands.backupPlan(destination, components)),
  start: async () => {
    await unwrap(commands.backupStart());
  },
  cancel: () => commands.backupCancel(),
  inspect: (path) => unwrap(commands.backupInspect(path)),
  restore: (path, components) => unwrap(commands.backupRestore(path, components)),
  discardRestore: async () => {
    await unwrap(commands.backupRestoreDiscard());
  },
  restart: async () => {
    await unwrap(commands.backupRestart());
  },
  reveal: async (path) => {
    await unwrap(commands.backupReveal(path));
  },
  subscribe: (listener) =>
    effectSubscription(events.backupChanged.listen, ({ payload }) => listener(payload)),
  onDevices: () => unwrap(commands.backupsOnDevices()),
  fetchFromDevice: (hash) => unwrap(commands.backupFetchFromDevice(hash)),
};

export const {
  Provider: BackupProvider,
  useApi: useBackupApi,
  preview: previewBackup,
} = injectable(() => nativeBackupApi);

const backupKey = ["backup", "status"] as const;

/** The backup status, kept current by the core's events. */
export function useBackupStatus() {
  const api = useBackupApi();
  const client = useQueryClient();
  useEffect(() => api.subscribe((status) => client.setQueryData(backupKey, status)), [api, client]);
  return useQuery({ queryKey: backupKey, queryFn: () => api.status(), retry: false });
}

/** What a backup with the given folder and components would hold. */
export function useBackupPlan(destination: string | null, components: BackupComponent[]) {
  const api = useBackupApi();
  return useQuery({
    queryKey: ["backup", "plan", destination, [...components].sort()],
    queryFn: () => api.plan(destination, components),
    retry: false,
  });
}

/** Runs a backup action, then refreshes the status. */
export function useBackupAction<A, R>(action: (api: BackupApi, argument: A) => Promise<R>) {
  const api = useBackupApi();
  const client = useQueryClient();
  return useMutation({
    mutationFn: (argument: A) => action(api, argument),
    onSettled: () => client.invalidateQueries({ queryKey: backupKey }),
  });
}
