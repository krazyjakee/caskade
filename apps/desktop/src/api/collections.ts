import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { usePageWindow } from "./hooks";
import { libraryKeys, useLibraryApi } from "./provider";
import type { CollectionsApi, SaveCollectionInput } from "./types";

/** Under the library's keys, so library events refresh smart collections. */
export const collectionKeys = {
  all: [...libraryKeys.all, "collections"] as const,
  members: (id: string) => [...libraryKeys.all, "collections", "members", id] as const,
  games: (id: string) => [...libraryKeys.all, "collections", "games", id] as const,
  ofGame: (gameId: string) => [...libraryKeys.all, "collections", "of", gameId] as const,
};

/** The library's collections, or `null` where it keeps none. */
export function useCollectionsApi(): CollectionsApi | null {
  return useLibraryApi().collections ?? null;
}

function required(api: CollectionsApi | null): CollectionsApi {
  if (!api) throw new Error("Collections aren't available here");
  return api;
}

export function useCollections() {
  const api = useCollectionsApi();
  return useQuery({
    queryKey: [...collectionKeys.all, "list"],
    queryFn: () => required(api).list(),
    enabled: Boolean(api),
  });
}

/** One collection's summary, from the list. `data` is undefined while it
 * loads and `null` when there is no such collection. */
export function useCollection(id: string) {
  const list = useCollections();
  return {
    ...list,
    data: list.data ? (list.data.find((collection) => collection.id === id) ?? null) : undefined,
  };
}

/** A manual collection's games, as ids. */
export function useCollectionMembers(id: string, { enabled = true } = {}) {
  const api = useCollectionsApi();
  return useQuery({
    queryKey: collectionKeys.members(id),
    queryFn: () => required(api).memberIds(id),
    enabled: Boolean(api) && enabled,
  });
}

/** A collection's games for a scrolling view, by the page around the rows
 * on screen (see `useGameWindow`). */
export function useCollectionWindow(id: string) {
  const api = useCollectionsApi();
  return usePageWindow({
    key: collectionKeys.games(id),
    token: id,
    fetchPage: (offset, limit) => required(api).games(id, offset, limit),
  });
}

/** The manual collections a game is in. */
export function useGameCollections(gameId: string) {
  const api = useCollectionsApi();
  return useQuery({
    queryKey: collectionKeys.ofGame(gameId),
    queryFn: () => required(api).ofGame(gameId),
    enabled: Boolean(api),
  });
}

/** A collection change: every collection query refreshes after it. */
function useCollectionMutation<T, R>(run: (api: CollectionsApi, input: T) => Promise<R>) {
  const api = useCollectionsApi();
  const client = useQueryClient();
  return useMutation({
    mutationFn: (input: T) => run(required(api), input),
    // Not awaited: the caller's own onSuccess (leaving a deleted
    // collection's page) runs before the refetch unmounts it.
    onSuccess: () => void client.invalidateQueries({ queryKey: collectionKeys.all }),
  });
}

/** Resolves to the collection's id. */
export function useSaveCollection() {
  return useCollectionMutation((api, input: SaveCollectionInput) => api.save(input));
}

export function useSetCollectionPinned() {
  return useCollectionMutation((api, input: { id: string; pinned: boolean }) =>
    api.setPinned(input.id, input.pinned),
  );
}

export function useDeleteCollection() {
  return useCollectionMutation((api, id: string) => api.delete(id));
}

/** Adds games to a manual collection, or takes them out. */
export function useCollectionMembership() {
  return useCollectionMutation((api, input: { id: string; gameIds: string[]; member: boolean }) =>
    input.member ? api.addGames(input.id, input.gameIds) : api.removeGames(input.id, input.gameIds),
  );
}
