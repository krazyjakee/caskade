/** Command bindings (#147): who handles an action. A binding belongs to an
 * element (a section, a view) or to nobody (the route or the app). The
 * innermost binding around the focused element wins; among bindings at the
 * same place, the newest wins. */
import type { Action } from "./actions";

export interface Binding {
  action: Action;
  /** Shown in the button-hint bar; unlabelled bindings aren't hinted. */
  label?: string;
  /** The element the binding belongs to; null for the whole view. */
  owner: () => Element | null;
  /** Returns false to let the next binding (or the default) handle it. */
  run: () => boolean;
  /** Also applies inside an open overlay (a menu, a dialog), after the
   * overlay's own bindings: Select and Back, the guide button. */
  global?: boolean;
}

export interface ActiveBinding {
  action: Action;
  label: string;
}

export class Bindings {
  private list: Binding[] = [];
  private listeners = new Set<() => void>();

  add(binding: Binding): () => void {
    this.list.push(binding);
    this.changed();
    return () => {
      this.list = this.list.filter((other) => other !== binding);
      this.changed();
    };
  }

  /** Bindings for `action`, the one to try first first. Inside an open
   * overlay (`within`), only the overlay's own bindings and global ones
   * count. */
  private ordered(focused: Element | null, within: Element | null, action?: Action): Binding[] {
    const depth = (binding: Binding) => {
      const owner = binding.owner();
      if (within && !binding.global && !(owner && within.contains(owner))) return -1;
      if (!owner) return 0;
      if (!focused || !owner.contains(focused)) return -1;
      let level = 1;
      for (let at = owner.parentElement; at; at = at.parentElement) level++;
      return level;
    };
    return this.list
      .map((binding, index) => ({ binding, index, depth: depth(binding) }))
      .filter((entry) => entry.depth >= 0 && (!action || entry.binding.action === action))
      .sort((a, b) => b.depth - a.depth || b.index - a.index)
      .map((entry) => entry.binding);
  }

  /** Runs the bindings for `action` until one handles it. */
  run(action: Action, focused: Element | null, within: Element | null = null): boolean {
    for (const binding of this.ordered(focused, within, action)) {
      if (binding.run()) return true;
    }
    return false;
  }

  /** What each action would do now, for hints: the first labelled binding
   * per action. */
  active(focused: Element | null, within: Element | null = null): ActiveBinding[] {
    const seen = new Map<Action, string>();
    for (const binding of this.ordered(focused, within)) {
      if (binding.label && !seen.has(binding.action)) seen.set(binding.action, binding.label);
    }
    return Array.from(seen, ([action, label]) => ({ action, label }));
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private changed() {
    for (const listener of this.listeners) listener();
  }
}
