/** The pieces views and plugins use (#147): bindings for actions, focus
 * sections and custom focusables. Built-in controls need none of these:
 * anything React Aria renders is already reachable. */
import { useLingui } from "@lingui/react/macro";
import {
  type RefObject,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useSyncExternalStore,
} from "react";
import type { Action } from "./actions";
import type { ActiveBinding } from "./bindings";
import { useOptionalInput } from "./InputProvider";
import { focus, scope } from "./spatial";

/** Handles `action` while focus is inside `owner` (or anywhere, without
 * one). Return false from `handler` to pass the action on. A `global`
 * binding also applies inside menus and dialogs. */
export function useInputAction(
  action: Action,
  handler: () => unknown,
  options: {
    label?: string;
    owner?: RefObject<Element | null>;
    enabled?: boolean;
    global?: boolean;
  } = {},
) {
  const input = useOptionalInput();
  const latest = useRef(handler);
  latest.current = handler;
  const { label, owner, enabled = true, global = false } = options;
  useEffect(() => {
    if (!input || !enabled) return;
    return input.bindings.add({
      action,
      label,
      global,
      owner: () => owner?.current ?? null,
      run: () => latest.current() !== false,
    });
  }, [input, action, label, owner, enabled, global]);
}

const none: ActiveBinding[] = [];

const sameBindings = (a: ActiveBinding[], b: ActiveBinding[]) =>
  a.length === b.length &&
  a.every((binding, i) => binding.action === b[i]?.action && binding.label === b[i]?.label);

/** The labelled bindings that apply where focus is now, for hints. They are
 * worked out when focus or the bindings change, not on every render:
 * finding the scope measures overlays, which forces a layout. */
export function useActiveBindings(): ActiveBinding[] {
  const input = useOptionalInput();
  // biome-ignore lint/correctness/useExhaustiveDependencies: a new input starts a new snapshot.
  const snapshot = useMemo<{ value: ActiveBinding[] | null }>(() => ({ value: null }), [input]);
  const compute = useCallback(() => {
    if (!input) return none;
    const within = scope();
    return input.bindings.active(document.activeElement, within === document.body ? null : within);
  }, [input]);
  const subscribe = useCallback(
    (changed: () => void) => {
      if (!input) return () => {};
      const update = () => {
        const value = compute();
        if (snapshot.value && sameBindings(value, snapshot.value)) return;
        snapshot.value = value;
        changed();
      };
      // Focus leaving for nowhere has no focusin; look once it has gone.
      const left = () => queueMicrotask(update);
      const stop = input.bindings.subscribe(update);
      document.addEventListener("focusin", update);
      document.addEventListener("focusout", left);
      // Anything that changed between the render and now.
      update();
      return () => {
        stop();
        document.removeEventListener("focusin", update);
        document.removeEventListener("focusout", left);
      };
    },
    [input, compute, snapshot],
  );
  const read = () => {
    snapshot.value ??= compute();
    return snapshot.value;
  };
  return useSyncExternalStore(subscribe, read, () => none);
}

/** Puts focus on `ref` once it is `ready` (its data loaded), unless the
 * user has already moved focus in this view: a view's natural first stop
 * (the hero's Play) can render after the view opened. A new `key` (Play
 * became Stop) puts it there again when the old one took focus with it. */
export function useDefaultFocus(ref: RefObject<HTMLElement | null>, ready = true, key?: string) {
  const input = useOptionalInput();
  // biome-ignore lint/correctness/useExhaustiveDependencies: a new `key` re-runs it on purpose.
  useEffect(() => {
    if (!ready || !input?.active) return;
    const target = ref.current;
    const current = document.activeElement;
    if (!target || target === current) return;
    if (current && current !== document.body && current !== input.entered.current) return;
    focus(target, true);
    input.entered.current = target;
  }, [ready, input, ref, key]);
}

/** LB/RB step through a view's main tabs: the one rule for every view (the
 * Library's filters, Settings' tabs, a game's plugin tabs). `go` opens a tab
 * and shouldn't add to history, so Back leaves the view. Focus follows to
 * the new tab when it was on a tab, or was lost with the old content;
 * `tabs` holds the tab elements and marks the open one with `aria-current`
 * or `aria-selected`. */
export function useScreenTabs<Tab>(
  ids: readonly Tab[],
  current: Tab,
  go: (tab: Tab) => void,
  options: { tabs?: RefObject<Element | null>; enabled?: boolean } = {},
) {
  const { t } = useLingui();
  const { tabs, enabled = ids.length > 1 } = options;
  const step = (by: number) => {
    if (ids.length < 2) return false;
    const at = Math.max(0, ids.indexOf(current));
    const next = ids[(at + by + ids.length) % ids.length];
    if (next === undefined) return false;
    const before = document.activeElement;
    const onTab = Boolean(before && tabs?.current?.contains(before));
    go(next);
    let frames = 0;
    const settle = () => {
      const open = tabs?.current?.querySelector<HTMLElement>(
        '[aria-current="page"], [aria-selected="true"]',
      );
      const lost = !document.activeElement || document.activeElement === document.body;
      if (open && open !== before && (onTab || lost)) focus(open, true);
      else if (++frames < 10) requestAnimationFrame(settle);
    };
    if (tabs) requestAnimationFrame(settle);
    return true;
  };
  useInputAction("tab.prev", () => step(-1), { label: t`Previous tab`, enabled });
  useInputAction("tab.next", () => step(1), { label: t`Next tab`, enabled });
}
