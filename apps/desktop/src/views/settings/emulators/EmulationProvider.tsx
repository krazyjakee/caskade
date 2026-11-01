import { t } from "@lingui/core/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createContext, type ReactNode, useContext, useEffect } from "react";
import type {
  EmulationApi,
  EmulationDetection,
  EmulationSnapshot,
  ScanJob,
} from "../../../api/emulation";
import type { SetupJob } from "../../../api/emulatorSetup";
import { effectSubscription } from "../../../api/subscription";

const Context = createContext<EmulationApi | null>(null);
export const emulationKey = ["emulation", "settings"] as const;
export const detectionKey = ["emulation", "detection"] as const;
/** Under `emulationKey`, so `emulationChanged` refetches it too. */
const scannersKey = ["emulation", "settings", "scanners"] as const;
export const scanJobsKey = ["emulation", "scans"] as const;
/** Under `emulationKey`, so a configuration change refetches them. */
const missingKey = ["emulation", "settings", "missing"] as const;
const installableKey = ["emulation", "settings", "installable"] as const;
const setupJobsKey = ["emulation", "setup", "jobs"] as const;
/** Keeps the newest revision of every install, oldest first. */
function mergeSetup(jobs: SetupJob[] | undefined, next: SetupJob): SetupJob[] {
  const old = jobs?.find((job) => job.id === next.id);
  if (old && old.revision > next.revision) return jobs ?? [];
  return old
    ? (jobs ?? []).map((job) => (job.id === next.id ? next : job))
    : [...(jobs ?? []), next];
}
/** Keeps the newest revision of every scan, newest scan first. */
export function mergeScan(jobs: ScanJob[] | undefined, next: ScanJob): ScanJob[] {
  const old = jobs?.find((job) => job.id === next.id);
  if (old && old.revision > next.revision) return jobs ?? [];
  return [next, ...(jobs ?? []).filter((job) => job.id !== next.id)];
}
export function EmulationProvider({ api, children }: { api: EmulationApi; children: ReactNode }) {
  const client = useQueryClient();
  useEffect(() => {
    const stops = [
      effectSubscription(api.subscribe, () => {
        void client.invalidateQueries({ queryKey: emulationKey });
      }),
      effectSubscription(api.subscribeDetection, (snapshot) => {
        client.setQueryData<EmulationDetection>(detectionKey, (old) =>
          old && old.id === snapshot.id && old.revision > snapshot.revision ? old : snapshot,
        );
      }),
      effectSubscription(api.subscribeScans, (job) => {
        client.setQueryData<ScanJob[]>(scanJobsKey, (old) => mergeScan(old, job));
        if (job.status === "done") void client.invalidateQueries({ queryKey: ["library"] });
      }),
      ...(api.setup
        ? [
            effectSubscription(api.setup.subscribe, (job) => {
              client.setQueryData<SetupJob[]>(setupJobsKey, (old) => mergeSetup(old, job));
              if (job.status === "done") {
                void client.invalidateQueries({ queryKey: emulationKey });
                void client.invalidateQueries({ queryKey: ["library"] });
              }
            }),
          ]
        : []),
    ];
    return () => {
      for (const stop of stops) stop();
    };
  }, [api, client]);
  return <Context.Provider value={api}>{children}</Context.Provider>;
}
export function useEmulationApi() {
  const api = useContext(Context);
  if (!api) throw new Error("EmulationProvider is required");
  return api;
}
export function useEmulation() {
  const api = useEmulationApi();
  return useQuery({
    queryKey: emulationKey,
    queryFn: () => api.snapshot(),
    refetchOnWindowFocus: true,
  });
}
export function useEmulationWrite() {
  const api = useEmulationApi();
  const client = useQueryClient();
  return useMutation({
    scope: { id: "emulation:configuration" },
    mutationFn: (write: (api: EmulationApi) => Promise<EmulationSnapshot>) => write(api),
    onSuccess: (snapshot) => {
      client.setQueryData(emulationKey, snapshot);
      void client.invalidateQueries({ queryKey: ["library"] });
    },
    onError: () => {
      void client.invalidateQueries({ queryKey: emulationKey });
    },
  });
}
export function useScanners() {
  const api = useEmulationApi();
  return useQuery({ queryKey: scannersKey, queryFn: () => api.scanners() });
}
export function useScanJobs() {
  const api = useEmulationApi();
  return useQuery({
    queryKey: scanJobsKey,
    queryFn: () => api.scanJobs(),
    // Events keep it current; polling covers a missed event while running.
    refetchInterval: (query) =>
      query.state.data?.some((job) => job.status === "running") ? 2000 : false,
  });
}

/** Platforms with games and no emulator; empty without one-click installs. */
export function useMissingEmulators() {
  const { setup } = useEmulationApi();
  return useQuery({
    queryKey: missingKey,
    queryFn: () => setup?.missing() ?? Promise.resolve([]),
  });
}
export function useInstallable() {
  const { setup } = useEmulationApi();
  return useQuery({
    queryKey: installableKey,
    queryFn: () => setup?.installable() ?? Promise.resolve([]),
  });
}
export function useSetupJobs() {
  const { setup } = useEmulationApi();
  return useQuery({
    queryKey: setupJobsKey,
    queryFn: () => setup?.jobs() ?? Promise.resolve([]),
  });
}
/** Starts an install of an emulator, or of a platform's recommended one. */
export function useSetupInstall() {
  const { setup } = useEmulationApi();
  const client = useQueryClient();
  return useMutation({
    mutationFn: (target: { definitionId: string } | { platformId: string }) => {
      if (!setup) return Promise.reject(new Error(t`Caskade can't install emulators here.`));
      return "platformId" in target
        ? setup.installFor(target.platformId)
        : setup.install(target.definitionId);
    },
    onSuccess: (job) =>
      client.setQueryData<SetupJob[]>(setupJobsKey, (old) => mergeSetup(old, job)),
  });
}
/** Update, skip or check a Caskade-installed emulator. */
export function useManagedAction() {
  const { setup } = useEmulationApi();
  const client = useQueryClient();
  return useMutation({
    mutationFn: async ({
      action,
      emulatorId,
    }: {
      action: "update" | "skip" | "check";
      emulatorId: string;
    }) => {
      if (!setup) throw new Error(t`Caskade can't install emulators here.`);
      if (action === "update") {
        const job = await setup.update(emulatorId);
        client.setQueryData<SetupJob[]>(setupJobsKey, (old) => mergeSetup(old, job));
      } else {
        await setup[action](emulatorId);
      }
    },
    onSettled: () => client.invalidateQueries({ queryKey: installableKey }),
  });
}
