/** Folder listings for the in-app path browser (console mode's file and
 * folder picker). The core lists; the caller still checks the path picked. */
import { createContext } from "react";
import type { DirEntry, DirListing_Serialize, Place } from "./bindings";

export type { DirEntry, Place };
export type DirListing = DirListing_Serialize;

export interface BrowseApi {
  /** `path`'s entries, folders first; home without a path. Rejects with the
   * core's message when the folder is missing or can't be read. */
  list(path: string | null, showHidden?: boolean): Promise<DirListing>;
}

/** The browse API for the path browser; the core's unless a test or the
 * browser preview gives another. */
export const BrowseApiContext = createContext<BrowseApi | null>(null);
