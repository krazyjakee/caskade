import { t } from "@lingui/core/macro";
import { type Reply, unwrap } from "./reply";

/** Discovery controller projection, independent of the development fixtures. */
interface DiscoveryReview {
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
  activeSources?: string[];
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
  ignoredEntries?: { source: string; key: string; title?: string }[];
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

interface RawDiscoverySnapshot extends Omit<DiscoverySnapshot, "os" | "sources"> {
  os: string;
  sources: {
    source: string;
    count: number;
    duration_ms: number;
    warnings: string[];
    error: unknown;
  }[];
}
/** A source's failure in words: the core's `SourceError` carries a message,
 * unless it timed out or was cancelled. */
function sourceError(error: unknown): string | null {
  if (!error) return null;
  if (typeof error === "string") return error;
  const { type, value } = error as { type?: string; value?: { message?: string } };
  if (type === "timed_out") return t`This source took too long to answer.`;
  if (type === "cancelled") return t`Scanning this source was cancelled.`;
  return value?.message ?? t`Caskade couldn't read this source.`;
}
function mapDiscoverySnapshot(snapshot: RawDiscoverySnapshot): DiscoverySnapshot {
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
      error: sourceError(source.error),
    })),
  };
}
interface DiscoveryTransport {
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
