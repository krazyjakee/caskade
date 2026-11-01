import { createContext, useContext } from "react";
import type { DropPlan_Serialize } from "./bindings";

export type DropPlan = DropPlan_Serialize;
export interface InstallRequest {
  requestId: string;
  kind: "url" | "path";
  value: string;
  /** How a dropped file or folder installs. */
  plan?: DropPlan | null;
}
export interface InstallRequestApi {
  pending(): Promise<InstallRequest[]>;
  request(value: string): Promise<InstallRequest>;
  /** A file or folder dropped on the library: the core works out what it
   * installs (a shortcut's target, an executable's folder) first. */
  requestDrop(path: string): Promise<InstallRequest>;
  /** Starts the install; resolves with its job's id. */
  confirm(id: string, rootId?: string): Promise<string>;
  dismiss(id: string): Promise<void>;
  subscribe(listener: (request: InstallRequest) => void): Promise<() => void>;
}

/** Requests made here reach the install confirmation through `subscribe`. */
export const InstallRequestContext = createContext<InstallRequestApi | null>(null);
export function useInstallRequests(): InstallRequestApi | null {
  return useContext(InstallRequestContext);
}
