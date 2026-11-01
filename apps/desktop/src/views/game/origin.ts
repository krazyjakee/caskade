/** Where a game's page goes back to (#195): the page it was opened from,
 * carried in the game route's `from` search param.
 *
 * The Back chain: a game opened from a browsing page (Home, the Library
 * with its filter, sort and search, a collection, the Store, Activity)
 * goes back to that page. A game opened from another game's page goes back
 * to that game's page, which keeps its own `from`, so Back walks the games
 * in the order they were opened and then returns to where browsing
 * started. The chain keeps the last `MAX_GAMES` games; past that the
 * oldest is forgotten, never the browsing page. A game page with no usable
 * `from` (opened directly, after a restart, or from a link that names
 * something other than an app page) goes back to the Library.
 *
 * The Store's catalog game pages (`/store/games/$igdbId`, #163) are game
 * pages in the chain too: a similar game opened from one goes back to it,
 * and one opened directly goes back to the Store. */
import type { MessageDescriptor } from "@lingui/core";
import { msg, t } from "@lingui/core/macro";
import { type HistoryState, useRouter } from "@tanstack/react-router";
import { useCallback } from "react";

export const MAX_GAMES = 4;

const BASE = "http://caskade.invalid";
const GAME = /^\/(?:store\/)?games\/[^/]+$/;
/** Where library and catalog game pages live. */
export const LIBRARY_GAMES = "/games";
export const STORE_GAMES = "/store/games";
/** The app pages a game can go back to. */
const PAGES = [
  /^\/$/,
  /^\/library$/,
  /^\/store$/,
  /^\/collections$/,
  /^\/collections\/[^/]+$/,
  /^\/activity$/,
  /^\/plugins\/[^/]+\/[^/]+$/,
  GAME,
];

function parse(value: unknown): URL | null {
  if (typeof value !== "string" || value.length > 4096) return null;
  // Only the app's own paths: no scheme, host or protocol-relative link.
  if (!value.startsWith("/") || value.startsWith("//") || value.includes("\\")) return null;
  try {
    const url = new URL(value, BASE);
    return url.origin === BASE && PAGES.some((page) => page.test(url.pathname)) ? url : null;
  } catch {
    return null;
  }
}

/** The pages `value` goes back through, nearest first, each without its own
 * `from`: game pages, then the browsing page (if any). */
function chain(value: unknown): string[] {
  const pages: string[] = [];
  let next = value;
  // A bound, so a crafted URL can't loop.
  for (let hop = 0; hop < 16; hop++) {
    const url = parse(next);
    if (!url) break;
    if (!GAME.test(url.pathname)) {
      pages.push(url.pathname + url.search);
      break;
    }
    next = url.searchParams.get("from");
    url.searchParams.delete("from");
    pages.push(url.pathname + url.search);
  }
  return pages;
}

const isGame = (page: string) => GAME.test(new URL(page, BASE).pathname);

/** Nests `pages` back into one href, keeping the nearest `MAX_GAMES` games
 * and the browsing page. */
function build(pages: string[]): string | undefined {
  const last = pages.at(-1);
  const root = last !== undefined && !isGame(last) ? last : undefined;
  const games = (root === undefined ? pages : pages.slice(0, -1)).slice(0, MAX_GAMES);
  return games.reduceRight<string | undefined>((from, page) => {
    if (from === undefined) return page;
    const url = new URL(page, BASE);
    url.searchParams.set("from", from);
    return url.pathname + url.search;
  }, root);
}

/** `from` as the game route accepts it: an app page, or nothing. */
export function validReturn(value: unknown): string | undefined {
  return build(chain(value));
}

/** Where game `gameId`'s page (under `base`) goes back to, given its
 * `from`: never to itself. */
export function backTarget(
  gameId: string,
  from: unknown,
  base: string = LIBRARY_GAMES,
): string | undefined {
  const self = `${base}/${encodeURIComponent(gameId)}`;
  const pages = chain(from);
  while (pages[0] !== undefined && new URL(pages[0], BASE).pathname === self) pages.shift();
  return build(pages);
}

/** What the Back link names: the page's own name where it has one. */
export type ReturnPlace =
  | { kind: "page"; name: string }
  | { kind: "collection"; id: string }
  | { kind: "game"; id: string }
  | { kind: "store-game"; id: string }
  | { kind: "unnamed" };

const LIBRARY = msg`Library`;

// The sidebar's names for the pages (`shell/nav.ts`), translated in place.
const NAMES: Record<string, MessageDescriptor> = {
  "/": msg({
    message: "Home",
    context: "navigation",
    comment: "The sidebar link to the home view",
  }),
  "/library": LIBRARY,
  "/store": msg`Store`,
  "/collections": msg`Collections`,
  "/activity": msg`Activity`,
};

export function returnPlace(target: string | undefined): ReturnPlace {
  if (target === undefined) return { kind: "page", name: t(LIBRARY) };
  const { pathname } = new URL(target, BASE);
  const name = NAMES[pathname];
  if (name) return { kind: "page", name: t(name) };
  const segment = pathname.split("/")[2] ?? "";
  let id = segment;
  try {
    id = decodeURIComponent(segment);
  } catch {
    // A malformed escape: the id as written.
  }
  if (pathname.startsWith("/collections/")) return { kind: "collection", id };
  if (pathname.startsWith("/games/")) return { kind: "game", id };
  if (pathname.startsWith(`${STORE_GAMES}/`)) {
    const segment = pathname.split("/")[3] ?? "";
    let storeId = segment;
    try {
      storeId = decodeURIComponent(segment);
    } catch {
      // A malformed escape: the id as written.
    }
    return { kind: "store-game", id: storeId };
  }
  return { kind: "unnamed" };
}

/** Opens a game's page, carrying the page the app shows now as its way
 * back. */
export function useOpenGame() {
  const router = useRouter();
  return useCallback(
    (gameId: string) => {
      // Already there: a second entry would make Back land on itself.
      if (router.state.location.pathname === `/games/${encodeURIComponent(gameId)}`) return;
      // The game page's history entry notes the entry it was opened from,
      // so its Back can step back to it (focus comes back with it). The
      // router's state type has no field for it; `useGoBack` reads it back.
      const state = { returnIndex: router.history.location.state.__TSR_index } as HistoryState;
      void router.navigate({
        to: "/games/$gameId",
        params: { gameId },
        search: { from: backTarget(gameId, router.state.location.href) },
        state,
      });
    },
    [router],
  );
}

/** Shows `gameId`'s page in place of this game page, with the same way
 * back: for a game merged into another that is the same game. */
export function useReplaceGame() {
  const router = useRouter();
  return useCallback(
    (gameId: string, from: string | undefined) => {
      void router.navigate({
        to: "/games/$gameId",
        params: { gameId },
        search: from ? { from } : {},
        state: router.history.location.state,
        replace: true,
      });
    },
    [router],
  );
}

/** Opens a Store catalog game's page (#163), carrying the page the app
 * shows now as its way back. */
export function useOpenStoreGame() {
  const router = useRouter();
  return useCallback(
    (igdbId: string) => {
      if (router.state.location.pathname === `${STORE_GAMES}/${encodeURIComponent(igdbId)}`) return;
      const state = { returnIndex: router.history.location.state.__TSR_index } as HistoryState;
      void router.navigate({
        to: "/store/games/$igdbId",
        params: { igdbId },
        search: { from: backTarget(igdbId, router.state.location.href, STORE_GAMES) },
        state,
      });
    },
    [router],
  );
}

/** Leaves a game's page for `target` (the Library without one): a step
 * back in history when the entry before is the one it was opened from,
 * so that page comes back as it was left; otherwise a replace, so Back
 * and the page never bounce between each other. */
export function useGoBack() {
  const router = useRouter();
  return useCallback(
    (target: string | undefined, fallback = "/library") => {
      const here = router.history.location;
      const returnIndex = "returnIndex" in here.state ? here.state.returnIndex : undefined;
      if (
        target !== undefined &&
        typeof returnIndex === "number" &&
        returnIndex === here.state.__TSR_index - 1
      ) {
        router.history.back();
        return;
      }
      void router.navigate({ href: target ?? fallback, replace: true });
    },
    [router],
  );
}
