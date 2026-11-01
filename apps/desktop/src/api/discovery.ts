import { type Reply, unwrap } from "./reply";

/** Discovery controller projection, independent of the development fixtures. */
export interface DiscoveryReview {
  id: string;
  title: string;
  sources: string[];
  location: string | null;
  evidence: string[];
}
export interface DiscoverySnapshot {
  revision: number;
  os: "windows" | "linux" | "macos";
  checks: { id: string; label: string; sources: string[] }[];
  phase: string;
  launchers: {
    source: string;
    found: boolean;
    path: string | null;
    version: string | null;
    kind: unknown;
  }[];
  sources: {
    source: string;
    count: number;
    durationMs: number;
    warnings: string[];
    error: string | null;
  }[];
  found: number;
  imported: number;
  review: DiscoveryReview[];
  warnings: string[];
  error: string | null;
  watching: boolean;
}
export interface DiscoveryPreferences {
  enabled: boolean;
  scanOnStartup: boolean;
  backgroundRefresh: boolean;
  refreshInterval: string;
  watchFolders: boolean;
  deepScans: {
    customFolders: boolean;
    externalDrives: boolean;
    portableExecutables: boolean;
    winePrefixes: boolean;
    fullDisk: boolean;
  };
  osChecks: Record<string, Record<string, boolean>>;
  scanPaths?: string[];
  ignoredEntries?: { source: string; key: string }[];
}
export interface DiscoveryApi {
  status(): Promise<DiscoverySnapshot>;
  launchers(): Promise<DiscoverySnapshot>;
  scan(): Promise<DiscoverySnapshot>;
  cancel(): Promise<DiscoverySnapshot>;
  review(ids: string[], accept: boolean, revision: number): Promise<DiscoverySnapshot>;
  subscribe(listener: (snapshot: DiscoverySnapshot) => void): Promise<() => void>;
}
export const scanning = (snapshot?: DiscoverySnapshot) =>
  snapshot && ["scanning", "importing", "cancelling"].includes(snapshot.phase);

export interface RawDiscoverySnapshot extends Omit<DiscoverySnapshot, "os" | "sources"> {
  os: string;
  sources: {
    source: string;
    count: number;
    duration_ms: number;
    warnings: string[];
    error: unknown;
  }[];
}
export function mapDiscoverySnapshot(snapshot: RawDiscoverySnapshot): DiscoverySnapshot {
  if (snapshot.os !== "linux" && snapshot.os !== "windows" && snapshot.os !== "macos")
    throw new Error("Unknown backend discovery profile.");
  return {
    ...snapshot,
    os: snapshot.os,
    sources: snapshot.sources.map((source) => ({
      source: source.source,
      count: source.count,
      durationMs: source.duration_ms,
      warnings: source.warnings,
      error: source.error
        ? typeof source.error === "string"
          ? source.error
          : JSON.stringify(source.error)
        : null,
    })),
  };
}
export interface DiscoveryTransport {
  discoveryStatus(): Promise<Reply<RawDiscoverySnapshot>>;
  discoveryLaunchers(): Promise<Reply<RawDiscoverySnapshot>>;
  discoveryScan(): Promise<Reply<RawDiscoverySnapshot>>;
  discoveryCancel(): Promise<Reply<RawDiscoverySnapshot>>;
  discoveryReview(
    ids: string[],
    accept: boolean,
    revision: number,
  ): Promise<Reply<RawDiscoverySnapshot>>;
}
export function createDiscoveryApi(
  transport: DiscoveryTransport,
  subscribe: (listener: (snapshot: RawDiscoverySnapshot) => void) => Promise<() => void>,
): DiscoveryApi {
  const project = async (request: Promise<Reply<RawDiscoverySnapshot>>) =>
    mapDiscoverySnapshot(await unwrap(request));
  return {
    status: () => project(transport.discoveryStatus()),
    launchers: () => project(transport.discoveryLaunchers()),
    scan: () => project(transport.discoveryScan()),
    cancel: () => project(transport.discoveryCancel()),
    review: (ids, accept, revision) => project(transport.discoveryReview(ids, accept, revision)),
    subscribe: (listener) => subscribe((snapshot) => listener(mapDiscoverySnapshot(snapshot))),
  };
}
