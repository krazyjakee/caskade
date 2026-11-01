import type { LibraryFilter, LibraryLayout, LibrarySort } from "../../api/types";
import { libraryFilters } from "./LibraryView";

/** The library's filter, sort and layout as the user last left them. A link
 * to `/library` that doesn't set them comes back to these, across restarts. */
export interface LibraryViewPrefs {
  filter: LibraryFilter;
  sort: LibrarySort;
  layout: LibraryLayout;
}

const KEY = "caskade.library.view";
const sorts: readonly LibrarySort[] = [
  "recent",
  "title",
  "playtime",
  "added",
  "released",
  "popular",
];
const layouts: readonly LibraryLayout[] = ["grid", "list"];
const defaultLibraryView: LibraryViewPrefs = {
  filter: "recent",
  sort: "recent",
  layout: "grid",
};

function pick<T extends string>(values: readonly T[], value: unknown): T | undefined {
  return values.find((candidate) => candidate === value);
}

/** Each field of `search` that is valid, the remembered value otherwise. */
export function resolveLibraryView(search: Record<string, unknown>): LibraryViewPrefs {
  const saved = rememberedLibraryView();
  return {
    filter: pick(libraryFilters, search.filter) ?? saved.filter,
    sort: pick(sorts, search.sort) ?? saved.sort,
    layout: pick(layouts, search.layout) ?? saved.layout,
  };
}

export function rememberedLibraryView(): LibraryViewPrefs {
  let saved: Record<string, unknown> = {};
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    if (parsed && typeof parsed === "object") saved = parsed as Record<string, unknown>;
  } catch {
    // Unreadable or unavailable storage: the defaults.
  }
  return {
    filter: pick(libraryFilters, saved.filter) ?? defaultLibraryView.filter,
    sort: pick(sorts, saved.sort) ?? defaultLibraryView.sort,
    layout: pick(layouts, saved.layout) ?? defaultLibraryView.layout,
  };
}

export function rememberLibraryView(prefs: LibraryViewPrefs) {
  try {
    localStorage.setItem(KEY, JSON.stringify(prefs));
  } catch {
    // Storage full or disabled: the view still works, it just isn't kept.
  }
}
