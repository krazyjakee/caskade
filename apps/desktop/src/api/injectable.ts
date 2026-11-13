import { createContext, createElement, type ReactNode, useContext } from "react";

/** A backend tests and previews swap. `Provider` supplies one; `useApi`
 * reads it, else the app's own, which `fallback` makes on first use and
 * `preview` replaces with the dev preview's fixture (`previewFixtures.ts`). */
export function injectable<T>(fallback: () => T) {
  const Context = createContext<T | null>(null);
  let own: T | undefined;
  return {
    Provider({ api, children }: { api: T; children: ReactNode }) {
      return createElement(Context.Provider, { value: api }, children);
    },
    useApi(): T {
      const api = useContext(Context);
      if (api) return api;
      own ??= fallback();
      return own;
    },
    /** The supplied API only. */
    useProvided: (): T | null => useContext(Context),
    preview(api: T) {
      own = api;
    },
  };
}
