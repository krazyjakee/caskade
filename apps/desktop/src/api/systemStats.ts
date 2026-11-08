/** Settings → Statistics (#156): live readings of this machine from the
 * core's sampler (`caskade_core::system_stats`).
 * The dashboard watches while it is open and the window is visible, renews
 * its watch, and gets each sample as a `systemStatsSampled` event; the
 * core stops sampling when nobody watches. A metric that couldn't be read
 * is `null` (with a reason where there is one), never 0. */
import { isTauri } from "@tauri-apps/api/core";
import { createContext, useContext, useEffect, useState } from "react";
import { commands, events } from "./bindings";
import { errorMessage, unwrap } from "./reply";
import { effectSubscription } from "./subscription";

export type GpuVendor = "amd" | "intel" | "nvidia" | "other";
export type InterfaceKind = "physical" | "virtual" | "loopback";

export interface CpuSample {
  /** Overall utilization, 0–100; null before the first measurement. */
  usage: number | null;
  cores: number;
}
/** Bytes. */
export interface MemorySample {
  total: number;
  used: number;
  available: number;
}
export interface GpuSample {
  id: string;
  name: string;
  vendor: GpuVendor;
  usage: number | null;
  memoryUsed: number | null;
  memoryTotal: number | null;
  /** Why `usage` is missing. */
  unavailable: string | null;
}
export interface GpuSamples {
  devices: GpuSample[];
  /** Why no GPU is listed. */
  unavailable: string | null;
}
export interface DiskSample {
  id: string;
  name: string;
  mount: string;
  otherMounts: string[];
  device: string;
  fileSystem: string;
  removable: boolean;
  total: number;
  available: number;
}
export interface InterfaceSample {
  name: string;
  kind: InterfaceKind;
  /** Bytes per second. */
  received: number | null;
  transmitted: number | null;
}
export interface NetworkSample {
  interfaces: InterfaceSample[];
  /** Bytes per second over the physical interfaces. */
  received: number | null;
  transmitted: number | null;
}
export interface SystemSample {
  seq: number;
  /** Unix milliseconds. */
  at: number;
  /** Why nothing could be read this time. */
  error: string | null;
  cpu: CpuSample;
  memory: MemorySample | null;
  gpus: GpuSamples;
  disks: DiskSample[];
  network: NetworkSample;
}
export interface SystemStatsSnapshot {
  intervalMs: number;
  history: number;
  samples: SystemSample[];
}

export interface SystemStatsApi {
  /** Starts or renews `watcher`'s watch; the history so far when `history`. */
  watch(watcher: string, history: boolean): Promise<SystemStatsSnapshot>;
  unwatch(watcher: string): Promise<void>;
  /** Every sample; returns the unsubscribe. */
  subscribe(listener: (sample: SystemSample) => void): () => void;
}

const preview = "Open Caskade desktop to see this computer's statistics.";

const ipcSystemStatsApi: SystemStatsApi = {
  watch: (watcher, history) =>
    isTauri()
      ? unwrap(commands.systemStatsWatch(watcher, history))
      : Promise.reject(new Error(preview)),
  unwatch: async (watcher) => {
    if (isTauri()) await unwrap(commands.systemStatsUnwatch(watcher));
  },
  subscribe: (listener) =>
    isTauri()
      ? effectSubscription(events.systemStatsSampled.listen, ({ payload }) => listener(payload))
      : () => {},
};

/** The fixture API, loaded on first use (README.md: dev builds started with
 * `VITE_CASKADE_FIXTURES=1`). */
function lazyFixtureApi(): SystemStatsApi {
  let loading: Promise<SystemStatsApi> | undefined;
  const load = () => {
    loading ??= import("./systemStatsFixtures").then((module) =>
      module.createSystemStatsFixtureApi({ live: true }),
    );
    return loading;
  };
  return {
    watch: (watcher, history) => load().then((api) => api.watch(watcher, history)),
    unwatch: (watcher) => load().then((api) => api.unwatch(watcher)),
    subscribe(listener) {
      let stop: (() => void) | undefined;
      let done = false;
      void load().then((api) => {
        if (!done) stop = api.subscribe(listener);
      });
      return () => {
        done = true;
        stop?.();
      };
    },
  };
}

export const nativeSystemStatsApi: SystemStatsApi =
  import.meta.env.DEV && import.meta.env.VITE_CASKADE_FIXTURES === "1"
    ? lazyFixtureApi()
    : ipcSystemStatsApi;

/** Tests and previews supply their own; the app uses the core's. */
export const SystemStatsApiContext = createContext<SystemStatsApi>(nativeSystemStatsApi);

/** How often a watch is renewed; the core lets one lapse after 30 s. */
export const RENEW_MS = 10_000;
/** Samples older than this many intervals make the view stale. */
export const STALE_INTERVALS = 3;

export type SystemStatsStatus = "loading" | "live" | "stale" | "error";

export interface SystemStatsState {
  /** `loading` until the first sample, `stale` when samples stopped
   * coming, `error` when watching failed and there is nothing to show. */
  status: SystemStatsStatus;
  /** Oldest first, at most `history`. */
  samples: SystemSample[];
  intervalMs: number;
  history: number;
  /** Why the last watch failed. */
  error: string | null;
}

/** Adds `incoming` to `samples` once each, oldest first, keeping `limit`. */
export function mergeSamples(samples: SystemSample[], incoming: SystemSample[], limit: number) {
  if (incoming.length === 0) return samples;
  const bySeq = new Map(samples.map((sample) => [sample.seq, sample]));
  for (const sample of incoming) bySeq.set(sample.seq, sample);
  return [...bySeq.values()].sort((a, b) => a.seq - b.seq).slice(-Math.max(1, limit));
}

/** What the dashboard shows, given when the last sample arrived. */
export function statsStatus(
  state: Omit<SystemStatsState, "status">,
  receivedAt: number | null,
  now: number,
): SystemStatsStatus {
  if (state.samples.length === 0) return state.error ? "error" : "loading";
  if (receivedAt === null || now - receivedAt > state.intervalMs * STALE_INTERVALS) return "stale";
  return state.error ? "stale" : "live";
}

function watcherId() {
  return globalThis.crypto?.randomUUID?.() ?? `stats-${Math.random().toString(36).slice(2)}`;
}

const hidden = () => typeof document !== "undefined" && document.visibilityState === "hidden";

/** Watches the machine while mounted and the window is visible. */
export function useSystemStats(): SystemStatsState {
  const api = useContext(SystemStatsApiContext);
  const [state, setState] = useState<Omit<SystemStatsState, "status">>({
    samples: [],
    intervalMs: 1000,
    history: 60,
    error: null,
  });
  const [receivedAt, setReceivedAt] = useState<number | null>(null);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    const watcher = watcherId();
    let live = true;
    // Ask for the history again after a failed or ended watch.
    let fresh = true;
    const start = () => {
      const history = fresh;
      fresh = false;
      api.watch(watcher, history).then(
        (snapshot) => {
          if (!live) return;
          setState((current) => ({
            intervalMs: snapshot.intervalMs,
            history: snapshot.history,
            error: null,
            samples: mergeSamples(current.samples, snapshot.samples, snapshot.history),
          }));
          const newest = snapshot.samples.at(-1);
          // A history is as fresh as its newest sample.
          if (newest) setReceivedAt((at) => at ?? Math.min(newest.at, Date.now()));
        },
        (error: unknown) => {
          if (!live) return;
          fresh = true;
          setState((current) => ({
            ...current,
            error: errorMessage(error),
          }));
        },
      );
    };
    const stop = () => {
      fresh = true;
      api.unwatch(watcher).catch(() => {});
    };
    const unsubscribe = api.subscribe((sample) => {
      if (!live) return;
      setState((current) => ({
        ...current,
        samples: mergeSamples(current.samples, [sample], current.history),
      }));
      setReceivedAt(Date.now());
    });
    if (!hidden()) start();
    const renew = setInterval(() => {
      if (!hidden()) start();
    }, RENEW_MS);
    // A hidden or minimized window stops the sampler; showing it resumes.
    const visibility = () => (hidden() ? stop() : start());
    document.addEventListener("visibilitychange", visibility);
    return () => {
      live = false;
      clearInterval(renew);
      document.removeEventListener("visibilitychange", visibility);
      unsubscribe();
      stop();
    };
  }, [api]);

  // Re-checks staleness between samples.
  useEffect(() => {
    const tick = setInterval(() => setNow(Date.now()), state.intervalMs);
    return () => clearInterval(tick);
  }, [state.intervalMs]);

  return { ...state, status: statsStatus(state, receivedAt, now) };
}
