import type { Contribution, ContributionFor, PluginContext } from "@caskade/sdk";
import { createContext, useContext, useMemo, useSyncExternalStore } from "react";
import { type Mode, useMode } from "../shell/console/mode";
import type { PluginSlot, UiPlugin } from "./types";

export interface Registered<S extends PluginSlot = PluginSlot> {
  plugin: Pick<UiPlugin, "id" | "name">;
  slot: S;
  contribution: ContributionFor<S>;
  context?: PluginContext;
  /** The manifest's `modes` for it; empty for the slot's default. */
  modes?: readonly string[];
}

/** Slots that console mode shows without the plugin asking (#152): the
 * game page and Home. Views show in both modes unless they say otherwise. */
const consoleSlots: ReadonlySet<PluginSlot> = new Set([
  "views",
  "game.details.tabs",
  "game.details.panels",
  "game.actions",
  "home.sections",
]);

/** Whether `item` shows in `mode`. */
export function shownIn(item: Registered, mode: Mode): boolean {
  if (item.modes?.length) return item.modes.includes(mode);
  return mode === "desktop" || consoleSlots.has(item.slot);
}

export interface PluginNotice {
  id: number;
  pluginId: string;
  pluginName: string;
  message: string;
  tone: "info" | "error";
}

const none: readonly never[] = Object.freeze([]);

/** Contributions by slot. Snapshots are immutable arrays, so React only
 * re-renders a slot when its contributions change. */
export class PluginRegistry {
  private slots = new Map<PluginSlot, readonly Registered[]>();
  private notices: readonly PluginNotice[] = none;
  private listeners = new Set<() => void>();
  private nextNotice = 1;

  /** Adds a contribution the plugin declared in its manifest. */
  register<S extends PluginSlot>(
    plugin: UiPlugin,
    slot: S,
    contribution: ContributionFor<S>,
    context?: PluginContext,
  ): () => void {
    const declared = plugin.contributes.find(
      (item) => item.slot === slot && item.id === contribution.id,
    );
    if (!declared) throw new Error(`${plugin.id} did not declare "${contribution.id}" in ${slot}`);
    const entry: Registered = {
      plugin: { id: plugin.id, name: plugin.name },
      slot,
      contribution: contribution as Contribution,
      context,
      modes: declared.modes,
    };
    const current = (this.slots.get(slot) ?? none).filter(
      (item) => !(item.plugin.id === plugin.id && item.contribution.id === contribution.id),
    );
    this.slots.set(slot, Object.freeze([...current, entry]));
    this.emit();
    return () => {
      const items = this.slots.get(slot) ?? none;
      if (!items.includes(entry)) return;
      this.slots.set(slot, Object.freeze(items.filter((item) => item !== entry)));
      this.emit();
    };
  }

  /** Removes everything a plugin registered. */
  unregisterPlugin(pluginId: string) {
    let changed = false;
    for (const [slot, items] of this.slots) {
      const kept = items.filter((item) => item.plugin.id !== pluginId);
      if (kept.length !== items.length) {
        this.slots.set(slot, Object.freeze(kept));
        changed = true;
      }
    }
    if (changed) this.emit();
  }

  list<S extends PluginSlot>(slot: S): readonly Registered<S>[] {
    return (this.slots.get(slot) ?? none) as readonly Registered<S>[];
  }

  notify(plugin: Pick<UiPlugin, "id" | "name">, message: string, tone: PluginNotice["tone"]) {
    this.notices = Object.freeze([
      ...this.notices,
      { id: this.nextNotice++, pluginId: plugin.id, pluginName: plugin.name, message, tone },
    ]);
    this.emit();
  }

  dismiss(id: number) {
    this.notices = Object.freeze(this.notices.filter((notice) => notice.id !== id));
    this.emit();
  }

  listNotices(): readonly PluginNotice[] {
    return this.notices;
  }

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  private emit() {
    for (const listener of this.listeners) listener();
  }
}

export const PluginRegistryContext = createContext(new PluginRegistry());

export function usePluginRegistry() {
  return useContext(PluginRegistryContext);
}

/** The contributions to one slot that show in the current mode, in
 * registration order; `allModes` includes the hidden ones. */
export function useContributions<S extends PluginSlot>(
  slot: S,
  { allModes = false }: { allModes?: boolean } = {},
): readonly Registered<S>[] {
  const registry = usePluginRegistry();
  const { mode } = useMode();
  const all = useSyncExternalStore(registry.subscribe, () => registry.list(slot));
  return useMemo(
    () => (allModes ? all : all.filter((item) => shownIn(item, mode))),
    [all, allModes, mode],
  );
}

export function usePluginNotices(): readonly PluginNotice[] {
  const registry = usePluginRegistry();
  return useSyncExternalStore(registry.subscribe, () => registry.listNotices());
}
