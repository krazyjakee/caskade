/** Game editing (#171, #172): the editor's view of a game and validated
 * patches. Fields the user sets stay theirs through automatic refreshes
 * until they reset them. */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { assetUrl, thumbnailPath } from "./artwork";
import type {
  ArtworkSlot as ArtworkSlotDto,
  BulkGameResult,
  BulkStatus,
  FieldError,
  FieldProblem,
  Game_Serialize,
  GameEdit_Deserialize,
  GameEditOutcome_Serialize,
  GameEditState_Serialize,
  GameField,
  ListEdit as ListEditDto,
  RefreshMode,
  RefreshStatus,
  TextEdit as TextEditDto,
} from "./bindings";
import { commands, events } from "./bindings";
import { libraryKeys, useOptionalLibraryApi } from "./provider";
import { unwrap } from "./reply";
import { effectSubscription } from "./subscription";
import type { LibraryQuery } from "./types";

export type {
  BulkGameResult,
  BulkStatus,
  FieldError,
  FieldProblem,
  GameField,
  RefreshMode,
  RefreshStatus,
};
/** A game as the core stores it. */
export type EditableGame = Game_Serialize;
export type GameEdit = GameEdit_Deserialize;
export type TextEdit = TextEditDto;
export type ListEdit = ListEditDto;
/** The game as the editor shows it: `game` is what the library shows,
 * `automatic` the values beneath, `manual` the fields the user set. */
export type GameEditState = GameEditState_Serialize;
/** What became of a patch; a rejected one saved nothing. */
type GameEditOutcome = GameEditOutcome_Serialize;

/** The artwork kinds the editor changes. */
export type EditableArtworkKind = "cover" | "hero" | "icon";
/** One image, ready to show. */
export interface ArtworkImage {
  id: string;
  /** `user` for the user's own; otherwise the source that supplied it. */
  source: string;
  /** Its 320px thumbnail; null while there is none (a placeholder shows). */
  url: string | null;
  width: number;
  height: number;
}
/** One kind of a game's artwork as the editor shows it. */
export interface ArtworkSlot {
  kind: EditableArtworkKind;
  /** Who decides it: automatic sources, or the user (an image, or none). */
  choice: "automatic" | "chosen" | "removed";
  shown: ArtworkImage | null;
  /** The images automatic sources supplied; the first is what Reset shows. */
  automatic: ArtworkImage[];
}
export type ArtworkChange =
  | { op: "choose"; artworkId: string }
  | { op: "import"; path: string }
  | { op: "remove" }
  | { op: "reset" };

export interface GameEditApi {
  /** `null` when the game is gone. */
  state(gameId: string): Promise<GameEditState | null>;
  /** Saves the changed fields, all or none; `base` is the game as the
   * editor loaded it, so a field changed since is a conflict. */
  save(gameId: string, edit: GameEdit, base: EditableGame | null): Promise<GameEditOutcome>;
  /** The game's cover, background and icon (#175); `null` when it is gone. */
  artwork(gameId: string): Promise<ArtworkSlot[] | null>;
  /** Changes one kind at once; refreshes never undo it. A failed import
   * changes nothing. */
  setArtwork(
    gameId: string,
    kind: EditableArtworkKind,
    change: ArtworkChange,
  ): Promise<ArtworkSlot[]>;
  /** One edit on many games (#173): previewed, or applied (`apply`). */
  bulkEdit(gameIds: string[], edit: GameEdit, apply: boolean): Promise<BulkGameResult[]>;
  /** Every game a library query finds, in its order. */
  gameIds(query: LibraryQuery): Promise<string[]>;
  /** Refreshes the games' details and artwork from the built-in sources
   * in the background (#176); refused while a refresh runs. */
  refreshStart(gameIds: string[], mode: RefreshMode): Promise<void>;
  /** Stops it after its current batch. */
  refreshCancel(): Promise<boolean>;
  /** The last refresh's status; null when none ran. */
  refreshStatus(): Promise<RefreshStatus | null>;
  subscribeRefresh(listener: (status: RefreshStatus) => void): () => void;
}

type ArtworkRecord = ArtworkSlotDto["automatic"][number];
function image(record: ArtworkRecord): ArtworkImage {
  const path = thumbnailPath(record, 320);
  return {
    id: record.id,
    source: record.source,
    url: path ? assetUrl(path) : null,
    width: record.width,
    height: record.height,
  };
}
function slot(dto: ArtworkSlotDto): ArtworkSlot {
  return {
    kind: dto.kind as EditableArtworkKind,
    choice: dto.choice,
    shown: dto.shown ? image(dto.shown) : null,
    automatic: dto.automatic.map(image),
  };
}

export const nativeGameEdits: GameEditApi = {
  async state(gameId) {
    if (!isTauri()) throw new Error("Open Caskade desktop to edit games.");
    return unwrap(commands.gameEditState(gameId));
  },
  async save(gameId, edit, base) {
    if (!isTauri()) throw new Error("Open Caskade desktop to edit games.");
    return unwrap(commands.gameEdit(gameId, edit, base));
  },
  async artwork(gameId) {
    if (!isTauri()) throw new Error("Open Caskade desktop to edit games.");
    const slots = await unwrap(commands.gameArtwork(gameId));
    return slots ? slots.map(slot) : null;
  },
  async setArtwork(gameId, kind, change) {
    if (!isTauri()) throw new Error("Open Caskade desktop to edit games.");
    return (await unwrap(commands.gameArtworkSet(gameId, kind, change))).map(slot);
  },
  async bulkEdit(gameIds, edit, apply) {
    if (!isTauri()) throw new Error("Open Caskade desktop to edit games.");
    return unwrap(commands.gamesBulkEdit(gameIds, edit, apply));
  },
  async refreshStart(gameIds, mode) {
    if (!isTauri()) throw new Error("Open Caskade desktop to edit games.");
    await unwrap(commands.gamesRefreshStart(gameIds, mode));
  },
  async refreshCancel() {
    return isTauri() ? commands.gamesRefreshCancel() : false;
  },
  async refreshStatus() {
    return isTauri() ? commands.gamesRefreshStatus() : null;
  },
  subscribeRefresh(listener) {
    if (!isTauri()) return () => {};
    return effectSubscription(events.gamesRefreshChanged.listen, ({ payload }) =>
      listener(payload),
    );
  },
  async gameIds(query) {
    if (!isTauri()) throw new Error("Open Caskade desktop to edit games.");
    return unwrap(
      commands.libraryGameIds({
        filter: query.filter,
        sort: query.sort,
        query: query.query ?? null,
        platform: query.platform ?? null,
        offset: 0,
        limit: 1000,
      }),
    );
  },
};

const gameEditKeys = {
  /** Under the library's keys, so a refresh shows the latest automatic
   * values; the editor keeps the game it opened with as its base. */
  state: (gameId: string) => [...libraryKeys.all, "edit", gameId] as const,
  artwork: (gameId: string) => [...libraryKeys.all, "edit", gameId, "artwork"] as const,
};

/** The library's editing, or `null` where games can't be edited. */
export function useGameEditApi(): GameEditApi | null {
  return useOptionalLibraryApi()?.edits ?? null;
}

export function useGameEditState(gameId: string) {
  const api = useGameEditApi();
  return useQuery({
    queryKey: gameEditKeys.state(gameId),
    queryFn: () => {
      if (!api) throw new Error("Editing games isn't available here");
      return api.state(gameId);
    },
    enabled: Boolean(api),
  });
}

/** Saves a patch; a saved one refreshes the whole library (cards, Home,
 * collections, the game page). */
export function useSaveGameEdit(gameId: string) {
  const api = useGameEditApi();
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ edit, base }: { edit: GameEdit; base: EditableGame | null }) => {
      if (!api) throw new Error("Editing games isn't available here");
      return api.save(gameId, edit, base);
    },
    onSuccess: (outcome) => {
      client.setQueryData(gameEditKeys.state(gameId), outcome.state);
      if (outcome.status === "saved" && outcome.changed.length > 0) {
        void client.invalidateQueries({ queryKey: libraryKeys.all });
      }
    },
  });
}

export function useGameArtwork(gameId: string) {
  const api = useGameEditApi();
  return useQuery({
    queryKey: gameEditKeys.artwork(gameId),
    queryFn: () => {
      if (!api) throw new Error("Editing games isn't available here");
      return api.artwork(gameId);
    },
    enabled: Boolean(api),
  });
}

/** Changes one kind of a game's artwork; the library shows it at once. */
export function useSetArtwork(gameId: string) {
  const api = useGameEditApi();
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({ kind, change }: { kind: EditableArtworkKind; change: ArtworkChange }) => {
      if (!api) throw new Error("Editing games isn't available here");
      return api.setArtwork(gameId, kind, change);
    },
    onSuccess: (slots) => {
      client.setQueryData(gameEditKeys.artwork(gameId), slots);
      void client.invalidateQueries({ queryKey: libraryKeys.all });
    },
  });
}

/** Previews or applies one edit on many games; an applied one refreshes
 * the library once. */
export function useBulkEdit() {
  const api = useGameEditApi();
  const client = useQueryClient();
  return useMutation({
    mutationFn: ({
      gameIds,
      edit,
      apply,
    }: {
      gameIds: string[];
      edit: GameEdit;
      apply: boolean;
    }) => {
      if (!api) throw new Error("Editing games isn't available here");
      return api.bulkEdit(gameIds, edit, apply);
    },
    onSuccess: (results, { apply }) => {
      if (apply && results.some((result) => result.status === "changed"))
        void client.invalidateQueries({ queryKey: libraryKeys.all });
    },
  });
}

/** The refresh of chosen games' details (#176): its status as it goes, and
 * starting, cancelling and retrying it. `null` where games can't be edited. */
export function useGamesRefresh() {
  const api = useGameEditApi();
  const client = useQueryClient();
  const [status, setStatus] = useState<RefreshStatus | null>(null);
  useEffect(() => {
    if (!api) return;
    let live = true;
    void api.refreshStatus().then((current) => {
      if (live && current) setStatus((known) => known ?? current);
    });
    const stop = api.subscribeRefresh((next) => {
      setStatus(next);
      if (!next.running) void client.invalidateQueries({ queryKey: libraryKeys.all });
    });
    return () => {
      live = false;
      stop();
    };
  }, [api, client]);
  const start = useMutation({
    mutationFn: ({ gameIds, mode }: { gameIds: string[]; mode: RefreshMode }) => {
      if (!api) throw new Error("Editing games isn't available here");
      return api.refreshStart(gameIds, mode);
    },
  });
  if (!api) return null;
  return { status, start, cancel: () => api.refreshCancel() };
}
