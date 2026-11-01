/** The action dispatcher (#147): pad actions from the core (`inputActionEvent`)
 * and the keyboard map feed one `dispatch`. Command bindings get each
 * action first; otherwise directions move focus spatially, `confirm`
 * presses the focused control, `back` closes the open overlay or goes back,
 * and `page.*` scrolls. Desktop mode stays mouse-and-keyboard first: the
 * keyboard map is off until console mode or the first pad action. */
import { isTauri } from "@tauri-apps/api/core";
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { setInteractionModality } from "react-aria-components";
import { events } from "../api/bindings";
import {
  type Action,
  type GlyphFamily,
  type InputSource,
  isDirection,
  isTextEntry,
  keyAction,
} from "./actions";
import { Bindings } from "./bindings";
import { press, synthetic } from "./keys";
import { OnScreenKeyboard, type TextTarget, typesText } from "./OnScreenKeyboard";
import {
  entry,
  type FocusMark,
  find,
  focus,
  focusables,
  hasOverlay,
  isFocusable,
  mark,
  next,
  reducedMotion,
  remember,
  scope,
  scroller,
} from "./spatial";

export interface PadAction {
  action: Action;
  glyphs: GlyphFamily;
}

export interface InputState {
  /** Spatial navigation and the keyboard map are on. */
  active: boolean;
  /** What sent the last action. */
  source: InputSource | null;
  /** The labels of the pad used last, for hints. */
  glyphs: GlyphFamily;
  dispatch(action: Action, source?: InputSource, glyphs?: GlyphFamily): void;
  bindings: Bindings;
  /** What focus was put on when a view opened, until the user moves it. */
  entered: { current: HTMLElement | null };
  /** Opens the on-screen keyboard on `target`, a text field, when a pad
   * sent the action being handled; returns whether it opened. */
  keyboard(target: Element | null): boolean;
  /** Whether the action being handled came from a pad or a tap, which type
   * on the on-screen keyboard; keys type themselves. */
  typesOnScreen(): boolean;
}

const Context = createContext<InputState | null>(null);

export function useOptionalInput(): InputState | null {
  return useContext(Context);
}

declare global {
  /** The browser preview's stand-in pad, in dev builds: `__caskadePad("search")`. */
  var __caskadePad: ((action: Action, glyphs?: GlyphFamily) => void) | undefined;
}

/** Native pad actions, when there is a core. Without one, a dev build's
 * browser preview takes them from `__caskadePad`, for live checks. */
function nativeActions(listener: (action: PadAction) => void): Promise<() => void> {
  if (!isTauri()) {
    if (!import.meta.env.DEV) return Promise.resolve(() => {});
    const pad = (action: Action, glyphs: GlyphFamily = "xbox") => listener({ action, glyphs });
    globalThis.__caskadePad = pad;
    return Promise.resolve(() => {
      if (globalThis.__caskadePad === pad) globalThis.__caskadePad = undefined;
    });
  }
  return events.inputActionEvent.listen(({ payload }) =>
    listener({ action: payload.action, glyphs: payload.glyphs }),
  );
}

/** Where pad actions come from when `InputProvider` isn't told: the core.
 * Tests and the end-to-end harness put their own source here. */
export const PadSource =
  createContext<(listener: (action: PadAction) => void) => Promise<() => void>>(nativeActions);

/** Whether `element` drives `overlay` from outside it: a combobox keeps
 * focus in its input while its suggestions are open. */
function controls(element: HTMLElement, overlay: HTMLElement): boolean {
  for (const attribute of ["aria-controls", "aria-activedescendant"]) {
    const id = element.getAttribute(attribute);
    const target = id ? document.getElementById(id) : null;
    if (target && overlay.contains(target)) return true;
  }
  return false;
}

const DIALOG = '[role="dialog"], [role="alertdialog"]';

export function InputProvider({
  children,
  console: consoleMode = false,
  locationKey,
  onBack = () => window.history.back(),
  subscribe: given,
}: {
  children: ReactNode;
  /** Console mode: navigation is always on. */
  console?: boolean;
  /** The current history entry; focus is restored when it comes back. */
  locationKey?: string;
  /** `back` with no overlay and no binding. */
  onBack?: () => void;
  /** Where pad actions come from (the core by default). */
  subscribe?: (listener: (action: PadAction) => void) => Promise<() => void>;
}) {
  const fallback = useContext(PadSource);
  const subscribe = given ?? fallback;
  const bindings = useMemo(() => new Bindings(), []);
  const [padUsed, setPadUsed] = useState(false);
  const [source, setSource] = useState<InputSource | null>(null);
  const [glyphs, setGlyphs] = useState<GlyphFamily>("generic");
  const active = consoleMode || padUsed;
  const back = useRef(onBack);
  back.current = onBack;

  // What sent the action being handled, for the on-screen keyboard: a pad
  // has no keys to type with.
  const from = useRef<InputSource | "touch" | null>(null);
  const [typing, setTyping] = useState<TextTarget | null>(null);
  const typesOnScreen = useCallback(() => from.current === "pad" || from.current === "touch", []);
  const keyboard = useCallback(
    (target: Element | null) => {
      if (!typesOnScreen()) return false;
      if (!typesText(target)) return false;
      setTyping(target);
      return true;
    },
    [typesOnScreen],
  );
  // It closes when the field loses focus.
  useEffect(() => {
    if (!typing) return;
    const close = () => setTyping(null);
    typing.addEventListener("blur", close);
    return () => typing.removeEventListener("blur", close);
  }, [typing]);
  // In console mode a tap on a field opens it too.
  useEffect(() => {
    if (!consoleMode) return;
    const onTap = (event: PointerEvent) => {
      if (event.pointerType !== "touch") return;
      from.current = "touch";
      const target = event.target instanceof Element ? event.target : null;
      keyboard(target?.closest("input, textarea") ?? null);
    };
    document.addEventListener("pointerup", onTap);
    return () => document.removeEventListener("pointerup", onTap);
  }, [consoleMode, keyboard]);

  const dispatch = useCallback(
    (action: Action, sender: InputSource = "keyboard", family?: GlyphFamily) => {
      from.current = sender;
      if (sender === "pad") {
        setPadUsed(true);
        if (family) setGlyphs(family);
      }
      setSource(sender);
      // Pad actions aren't key presses, but focus rings should show.
      setInteractionModality("keyboard");
      const within = scope();
      const current = document.activeElement;
      const focused =
        current instanceof HTMLElement &&
        current !== document.body &&
        (within.contains(current) || controls(current, within))
          ? current
          : null;
      if (bindings.run(action, focused, within === document.body ? null : within)) return;
      runDefault(action, focused, () => back.current(), keyboard);
    },
    [bindings, keyboard],
  );

  // Pad actions from the core.
  useEffect(() => {
    let stop: (() => void) | undefined;
    let gone = false;
    void subscribe(({ action, glyphs }) => dispatch(action, "pad", glyphs)).then((unlisten) => {
      if (gone) unlisten();
      else stop = unlisten;
    });
    return () => {
      gone = true;
      stop?.();
    };
  }, [subscribe, dispatch]);

  // The keyboard produces the same actions while navigation is on.
  useEffect(() => {
    if (!active) return;
    const onKey = (event: KeyboardEvent) => {
      if (synthetic.has(event)) return;
      const action = keyAction(event);
      if (!action) return;
      event.preventDefault();
      event.stopPropagation();
      dispatch(action, "keyboard");
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [active, dispatch]);

  // Where focus was in each history entry, to put it back on return.
  const marks = useRef(new Map<string, FocusMark>());
  const lastFocus = useRef<HTMLElement | null>(null);
  const here = useRef(locationKey);
  if (here.current !== locationKey) {
    // Leaving `here`. The old view is still on screen while this renders,
    // so this is where the user left focus; focus that moves while views
    // change over isn't.
    const left = lastFocus.current;
    if (here.current && left?.isConnected) marks.current.set(here.current, mark(left));
    here.current = locationKey;
  }
  const activeRef = useRef(active);
  activeRef.current = active;
  useEffect(() => {
    const onFocus = (event: FocusEvent) => {
      const target = event.target;
      if (!(target instanceof HTMLElement)) return;
      remember(target);
      if (!target.closest("[role='dialog'], [data-placement]")) lastFocus.current = target;
      // A dialog opens with focus on itself, where A does nothing: a pad
      // starts on its default control, or its first.
      if (activeRef.current && target.matches(DIALOG)) {
        requestAnimationFrame(() => {
          if (document.activeElement !== target) return;
          const preferred = target.querySelector<HTMLElement>("[data-focus-default]");
          const first = preferred && isFocusable(preferred) ? preferred : focusables(target).at(0);
          if (first) focus(first, reducedMotion());
        });
      }
    };
    document.addEventListener("focusin", onFocus);
    return () => document.removeEventListener("focusin", onFocus);
  }, []);
  const entered = useRef<HTMLElement | null>(null);
  useEffect(() => {
    if (!locationKey) return;
    const saved = marks.current.get(locationKey);
    if (!saved && !activeRef.current) return;
    // The view renders after the location changes, its data maybe later;
    // wait up to half a second.
    let frames = 0;
    let handle = 0;
    const attempt = () => {
      // A dialog opened meanwhile: it has focus now, not the view.
      if (hasOverlay() || document.activeElement?.closest(DIALOG)) return;
      // The view already chose (useDefaultFocus): keep it.
      const current = document.activeElement;
      if (!saved && current && current === entered.current && current.isConnected) return;
      const target = saved ? find(saved) : entry();
      if (target) {
        focus(target, reducedMotion());
        entered.current = saved ? null : target;
      } else if (++frames < 30) handle = requestAnimationFrame(attempt);
    };
    handle = requestAnimationFrame(attempt);
    return () => cancelAnimationFrame(handle);
  }, [locationKey]);

  const state = useMemo(
    () => ({ active, source, glyphs, dispatch, bindings, entered, keyboard, typesOnScreen }),
    [active, source, glyphs, dispatch, bindings, keyboard, typesOnScreen],
  );
  return (
    <Context.Provider value={state}>
      {children}
      {typing && (
        <OnScreenKeyboard
          key={typing.id || "keyboard"}
          target={typing}
          close={() => setTyping(null)}
        />
      )}
    </Context.Provider>
  );
}

/** What an action does when no binding takes it. */
function runDefault(
  action: Action,
  focused: HTMLElement | null,
  goBack: () => void,
  keyboard: (target: Element | null) => boolean,
) {
  if (isDirection(action)) {
    const horizontal = action === "nav.left" || action === "nav.right";
    // A combobox keeps focus in its input and moves through its suggestions
    // with the arrows (the search, #150).
    // Down opens them once there's text; up only moves in an open list,
    // otherwise it leaves the field.
    const open = focused?.getAttribute("aria-expanded") === "true";
    const typed = focused instanceof HTMLInputElement && focused.value.trim() !== "";
    if (
      !horizontal &&
      focused?.getAttribute("role") === "combobox" &&
      (open || (action === "nav.down" && typed))
    ) {
      press(focused, action === "nav.down" ? "ArrowDown" : "ArrowUp");
      return;
    }
    if (
      horizontal &&
      focused &&
      (focused.getAttribute("role") === "slider" ||
        (focused instanceof HTMLInputElement && focused.type === "range"))
    ) {
      press(focused, action === "nav.left" ? "ArrowLeft" : "ArrowRight");
      return;
    }
    const target = focused ? next(focused, action) : entry();
    if (target) focus(target, reducedMotion());
    return;
  }
  switch (action) {
    case "confirm":
      if (!focused) {
        const target = entry();
        if (target) focus(target, reducedMotion());
      } else if (isTextEntry(focused)) {
        // A pad types on the on-screen keyboard, unless a suggestion is
        // there to pick; keys press Enter.
        const picking = focused.getAttribute("aria-expanded") === "true";
        if (picking || !keyboard(focused)) press(focused, "Enter");
      } else {
        focused.click();
      }
      return;
    case "back":
      if (hasOverlay()) press(focused ?? scope(), "Escape");
      else if (focused && isTextEntry(focused)) focused.blur();
      else goBack();
      return;
    case "page.prev":
    case "page.next": {
      const box = scroller(focused);
      if (!box) return;
      const sign = action === "page.next" ? 1 : -1;
      box.scrollBy?.({ top: sign * box.clientHeight * 0.9, behavior: "auto" });
      requestAnimationFrame(() => {
        const view = box.getBoundingClientRect();
        const visible = focusables(box).find((element) => {
          const rect = element.getBoundingClientRect();
          return rect.top >= view.top && rect.bottom <= view.bottom;
        });
        if (visible) focus(visible, true);
      });
      return;
    }
    default:
  }
}
