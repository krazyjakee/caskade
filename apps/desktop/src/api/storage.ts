/** Root registry and managed-move contract. Native implementation uses generated bindings. */

import { t } from "@lingui/core/macro";

export type RootStatus = "online" | "offline" | "read_only" | "identity_mismatch";
export interface StorageRoot {
  id: string;
  path: string;
  isDefault: boolean;
  label: string | null;
}
export interface RootCapabilities {
  writable: boolean;
  executable: boolean;
  symlinks: boolean;
  caseSensitive: boolean;
  freeBytes: number | null;
  totalBytes: number | null;
  filesystem: string | null;
  volume?: string | null;
  warnings: string[];
}
export interface StorageGame {
  entryId: string;
  gameId: string;
  title: string;
  relativeDir: string;
  sizeBytes: number;
  available: boolean;
  moveBlocked: string | null;
}
export interface RootSummary {
  root: StorageRoot;
  status: RootStatus;
  capabilities: RootCapabilities;
  folderTemplate: string;
  games: StorageGame[];
  gamesBytes: number;
  otherBytes: number | null;
}
export interface StorageApi {
  subscribe?(listener: () => void): Promise<() => void>;
  list(): Promise<RootSummary[]>;
  preferences(): Promise<{ alwaysUseDefault: boolean }>;
  add(path: string, label?: string): Promise<RootSummary>;
  edit(id: string, label: string, template: string): Promise<RootSummary>;
  setDefault(id: string): Promise<RootSummary>;
  remove(id: string): Promise<void>;
  chooseDirectory(): Promise<string | null>;
  move(entryId: string, targetRootId: string): Promise<void>;
  uninstall(entryId: string): Promise<void>;
}
export interface RootRequirements {
  estimatedBytes?: number;
  executable?: boolean;
  symlinks?: boolean;
}
/** The same constraints drive the picker and tests; the backend rechecks them. */
export function rootBlockReason(root: RootSummary, needs: RootRequirements): string | null {
  if (root.status !== "online")
    return root.status === "read_only" ? t`Read-only location` : t`Location unavailable`;
  if (!root.capabilities.writable) return t`Read-only location`;
  if (needs.executable && !root.capabilities.executable) return t`Cannot run executables here`;
  if (needs.symlinks && !root.capabilities.symlinks) return t`This game needs symbolic links`;
  if (
    needs.estimatedBytes !== undefined &&
    root.capabilities.freeBytes !== null &&
    root.capabilities.freeBytes < needs.estimatedBytes
  )
    return t`Not enough free space`;
  return null;
}
export function initialInstallRoot(roots: RootSummary[], needs: RootRequirements): string | null {
  const usable = roots.filter((root) => !rootBlockReason(root, needs));
  return usable.find((root) => root.root.isDefault)?.root.id ?? usable[0]?.root.id ?? null;
}
export function skipRootPicker(
  roots: RootSummary[],
  alwaysUseDefault: boolean,
  needs: RootRequirements,
): string | null {
  const selected =
    roots.length === 1
      ? roots[0]
      : alwaysUseDefault
        ? roots.find((root) => root.root.isDefault)
        : undefined;
  return selected && !rootBlockReason(selected, needs) ? selected.root.id : null;
}
/** Binary sizes (`140 GiB`) in the app's language; see `presentation.ts`. */
export { formatBytes } from "./presentation";
