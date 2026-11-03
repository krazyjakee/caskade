import { createContext, useContext } from "react";
import type { DropPlan_Serialize, PeerSend_Serialize, ShareInfo_Serialize } from "./bindings";

export type DropPlan = DropPlan_Serialize;
/** A game another of the user's devices sends here (#235). */
export type PeerSend = PeerSend_Serialize;
export interface InstallRequest {
  requestId: string;
  kind: "url" | "path" | "peer" | "share";
  value: string;
  /** How a dropped file or folder installs. */
  plan?: DropPlan | null;
  /** The device and game, for a copy another device sends. */
  peer?: PeerSend | null;
  /** What a share link someone sent shares, as its sharer says (#246). */
  share?: ShareInfo_Serialize | null;
}
export interface InstallRequestApi {
  pending(): Promise<InstallRequest[]>;
  request(value: string): Promise<InstallRequest>;
  /** A file or folder dropped on the library: the core works out what it
   * installs (a shortcut's target, an executable's folder) first. */
  requestDrop(path: string): Promise<InstallRequest>;
  /** Starts the install; resolves with its job's id. */
  confirm(id: string, rootId?: string): Promise<string>;
  /** Adds the collection a share request links to (#247): a list, so no
   * job runs. */
  confirmCollection?(id: string): Promise<void>;
  dismiss(id: string): Promise<void>;
  subscribe(listener: (request: InstallRequest) => void): Promise<() => void>;
}

/** Requests made here reach the install confirmation through `subscribe`. */
export const InstallRequestContext = createContext<InstallRequestApi | null>(null);
export function useInstallRequests(): InstallRequestApi | null {
  return useContext(InstallRequestContext);
}
