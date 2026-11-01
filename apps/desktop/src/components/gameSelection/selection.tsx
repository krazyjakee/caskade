/** Choosing several games in a library view (#173): a selection mode, games
 * chosen by id (so scrolling a virtualized grid never loses them), a click
 * to toggle one, Shift for a range, and a pad's A to toggle. Each view
 * owns its selection; cards and rows read whether they are chosen without
 * re-rendering the others. */
import {
  createContext,
  type ReactNode,
  useContext,
  useMemo,
  useRef,
  useSyncExternalStore,
} from "react";
import type { GameSummary } from "../../api/types";

/** A view's chosen games. */
export class GameSelection {
  private chosen = new Set<string>();
  private on = false;
  private anchor: number | null = null;
  private listeners = new Set<() => void>();
  private version = 0;

  /** `gameAt` is the view's game at an index, when it is loaded. */
  constructor(private gameAt: (index: number) => GameSummary | undefined) {}

  setGameAt(gameAt: (index: number) => GameSummary | undefined) {
    this.gameAt = gameAt;
  }

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };
  getVersion = () => this.version;

  private changed() {
    this.version += 1;
    for (const listener of this.listeners) listener();
  }

  get active() {
    return this.on;
  }
  get count() {
    return this.chosen.size;
  }
  has(id: string) {
    return this.chosen.has(id);
  }
  /** The chosen games, in the order they were chosen. */
  ids(): string[] {
    return [...this.chosen];
  }

  start() {
    if (this.on) return;
    this.on = true;
    this.changed();
  }
  /** Leaves selection mode, forgetting the choice. */
  stop() {
    this.on = false;
    this.chosen.clear();
    this.anchor = null;
    this.changed();
  }
  clear() {
    this.chosen.clear();
    this.anchor = null;
    this.changed();
  }
  toggle(id: string, index?: number) {
    if (this.chosen.has(id)) this.chosen.delete(id);
    else this.chosen.add(id);
    this.anchor = index ?? this.anchor;
    this.on = true;
    this.changed();
  }
  /** Chooses every loaded game from the last one toggled to `index`. Games
   * of the range not loaded yet aren't chosen. */
  extend(index: number) {
    const from = this.anchor ?? index;
    for (let at = Math.min(from, index); at <= Math.max(from, index); at++) {
      const game = this.gameAt(at);
      if (game) this.chosen.add(game.id);
    }
    this.anchor = index;
    this.on = true;
    this.changed();
  }
  /** Opens the bulk editor for the chosen games (a pad's Start in
   * selection mode); the view's selection bar sets it. */
  onEdit: (() => void) | null = null;
  edit() {
    if (this.chosen.size > 0) this.onEdit?.();
  }
  add(ids: readonly string[]) {
    for (const id of ids) this.chosen.add(id);
    this.on = true;
    this.changed();
  }
  /** Drops games no longer there (deleted, or gone from the view). */
  keep(ids: ReadonlySet<string>) {
    const before = this.chosen.size;
    for (const id of this.chosen) if (!ids.has(id)) this.chosen.delete(id);
    if (this.chosen.size !== before) this.changed();
  }
}

const Context = createContext<GameSelection | null>(null);

/** A view's selection, kept across renders; `gameAt` stays current. */
export function useGameSelectionStore(gameAt: (index: number) => GameSummary | undefined) {
  const store = useRef<GameSelection | null>(null);
  store.current ??= new GameSelection(gameAt);
  store.current.setGameAt(gameAt);
  return store.current;
}

export function SelectionScope({
  selection,
  children,
}: {
  selection: GameSelection;
  children: ReactNode;
}) {
  return <Context value={selection}>{children}</Context>;
}

/** The view's selection, or null outside one. */
export function useSelection(): GameSelection | null {
  return useContext(Context);
}

/** Re-renders on every change of the selection: for its bar. */
export function useSelectionVersion(selection: GameSelection | null) {
  return useSyncExternalStore(
    selection?.subscribe ?? noop,
    selection?.getVersion ?? zero,
    selection?.getVersion ?? zero,
  );
}

/** Whether selection mode is on and whether `id` is chosen; re-renders
 * only when those change for this game. */
export function useGameSelected(id: string): { mode: boolean; selected: boolean } {
  const selection = useSelection();
  const state = useSyncExternalStore(
    selection?.subscribe ?? noop,
    () => (selection?.active ? (selection.has(id) ? 2 : 1) : 0),
    () => 0,
  );
  return useMemo(() => ({ mode: state > 0, selected: state === 2 }), [state]);
}

const noop = () => () => {};
const zero = () => 0;
