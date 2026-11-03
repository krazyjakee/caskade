/** Spatial navigation over the real DOM (#147): the next focusable in a
 * direction, by geometry. React Aria already renders every control as a
 * focusable element, so nothing registers itself. `data-focus-section`
 * groups focusables; a section remembers where focus last was and is
 * entered there again. The topmost open dialog or popover is the scope, so
 * focus never leaves an overlay. */
import type { Direction } from "./actions";

/** Elements that take focus: natively focusable ones, plus the items of
 * React Aria collections, which rove their tab stop. */
const FOCUSABLE = [
  "a[href]",
  "button",
  "input",
  "select",
  "textarea",
  "[tabindex]",
  '[role="option"]',
  '[role="tab"]',
  '[role="radio"]',
  '[role="menuitem"]',
  '[role="menuitemradio"]',
  '[role="menuitemcheckbox"]',
  '[role="row"]',
  '[contenteditable="true"]',
].join(",");

/** Overlays, newest last: React Aria portals them to the end of the body. */
const OVERLAY = '[role="dialog"], [role="alertdialog"], [data-placement]';

const memory = new WeakMap<Element, HTMLElement>();

export function section(element: Element): HTMLElement | null {
  return element.closest<HTMLElement>("[data-focus-section]");
}

/** Remembers `element` as the last focus of every section around it. */
export function remember(element: HTMLElement) {
  for (let at = section(element); at; at = section(at.parentElement ?? document.body)) {
    memory.set(at, element);
    if (!at.parentElement) break;
  }
}

export function remembered(of: Element): HTMLElement | null {
  const element = memory.get(of);
  return element && isFocusable(element) && of.contains(element) ? element : null;
}

/** Where focus may go now: the topmost overlay, else the document. */
export function scope(): HTMLElement {
  const overlays = Array.from(document.querySelectorAll<HTMLElement>(OVERLAY)).filter(
    (overlay) => !overlay.closest("[hidden], [inert]") && hasBox(overlay),
  );
  return overlays.at(-1) ?? document.body;
}

export function hasOverlay(): boolean {
  return scope() !== document.body;
}

function hasBox(element: Element) {
  const rect = element.getBoundingClientRect();
  return rect.width > 0 || rect.height > 0;
}

export function isFocusable(element: Element): element is HTMLElement {
  if (!(element instanceof HTMLElement) || !element.isConnected) return false;
  if (!element.matches(FOCUSABLE)) return false;
  // A Tab stop that isn't a place to go by direction (a scroll box).
  if (element.dataset.spatial === "skip") return false;
  if (element.closest("[hidden], [inert], [aria-hidden='true']")) return false;
  if ((element as HTMLButtonElement).disabled || element.getAttribute("aria-disabled") === "true")
    return false;
  if (element.getAttribute("tabindex") === "-1" && !element.matches(FOCUSABLE_ITEMS)) {
    // Programmatic-only targets (dialogs, scroll boxes) aren't stops; a
    // visually hidden input is, when React Aria renders it as the control.
    return false;
  }
  // Hidden inputs count by the box that stands for them.
  return hasBox(box(element));
}

const FOCUSABLE_ITEMS =
  '[role="option"], [role="tab"], [role="radio"], [role="menuitem"], [role="menuitemradio"], [role="menuitemcheckbox"], [role="row"], input';

/** The element whose box stands for `element` on screen: React Aria hides
 * the inputs of switches, checkboxes, radios and slider thumbs, so their
 * nearest visible ancestor (the label, the thumb) stands in. */
function box(element: HTMLElement): HTMLElement {
  if (!(element instanceof HTMLInputElement) || hasBox(element)) return element;
  for (let at = element.parentElement; at && at !== document.body; at = at.parentElement) {
    if (hasBox(at)) return at;
  }
  return element;
}

export function focusables(within: Element = scope()): HTMLElement[] {
  return Array.from(within.querySelectorAll(FOCUSABLE)).filter(isFocusable);
}

interface Box {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

function rect(element: HTMLElement): Box {
  const r = box(element).getBoundingClientRect();
  return { left: r.left, right: r.right, top: r.top, bottom: r.bottom };
}

/** A settings row: its label on the left, its control at the far end. */
const row = (element: Element) => element.closest<HTMLElement>('[data-cs="setting-row"]');

/** Where `element` is when moving from `from`. A control in a settings row
 * stands for its whole row, so a pad goes row by row; within one row, the
 * controls' own boxes tell them apart. */
function place(element: HTMLElement, from: HTMLElement): Box {
  const own = row(element);
  if (!own || own.contains(from)) return rect(element);
  const r = own.getBoundingClientRect();
  return { left: r.left, right: r.right, top: r.top, bottom: r.bottom };
}

/** Added for a target outside the row or column you're in, so one in it wins
 * however far along it is: across a list row to its button, not up to the
 * filter just above. Going up or down, only within the nearest line
 * (`nearestLine`). */
const OFF_AXIS = 100_000;

/** How far `to` is from `from` in `direction`, or null if it isn't that way.
 * The gap along the direction counts once; a sideways offset counts twice,
 * and only targets in the row or column you're in come first. */
function distance(from: Box, to: Box, direction: Direction): number | null {
  const centre = (low: number, high: number) => (low + high) / 2;
  const [along, side] =
    direction === "nav.left" || direction === "nav.right"
      ? [
          direction === "nav.right" ? to.left - from.right : from.left - to.right,
          gap(from.top, from.bottom, to.top, to.bottom),
        ]
      : [
          direction === "nav.down" ? to.top - from.bottom : from.top - to.bottom,
          gap(from.left, from.right, to.left, to.right),
        ];
  // Both edges must move that way, not just the centre: the next row's
  // title, a little wider than this one's, isn't to its right.
  const ahead =
    direction === "nav.right"
      ? centre(to.left, to.right) > centre(from.left, from.right) && to.left > from.left
      : direction === "nav.left"
        ? centre(to.left, to.right) < centre(from.left, from.right) && to.right < from.right
        : direction === "nav.down"
          ? centre(to.top, to.bottom) > centre(from.top, from.bottom) && to.top > from.top
          : centre(to.top, to.bottom) < centre(from.top, from.bottom) && to.bottom < from.bottom;
  if (!ahead) return null;
  return Math.max(along, 0) + (side > 0 ? OFF_AXIS + side * 2 : 0);
}

/** The space between two ranges; 0 when they overlap. */
function gap(aLow: number, aHigh: number, bLow: number, bHigh: number) {
  return Math.max(0, bLow - aHigh, aLow - bHigh);
}

interface Target {
  element: HTMLElement;
  to: Box;
  /** What it stands in when lining targets up (`reach`). */
  reach: Box;
  d: number;
  inHome: boolean;
}

/** The outermost focus section around `target` that focus isn't in (a
 * shelf, the settings tabs), when it has a box; else `target`'s own place. */
function reach(target: HTMLElement, from: HTMLElement, own: Box): Box {
  let entered: HTMLElement | null = null;
  for (let at = section(target); at && !at.contains(from); at = section(at.parentElement ?? from))
    entered = at;
  if (!entered || !hasBox(entered)) return own;
  const r = entered.getBoundingClientRect();
  return { left: r.left, right: r.right, top: r.top, bottom: r.bottom };
}

/** Going up or down, the line of targets nearest that way: those level
 * with the nearest one. A target in the column focus is in wins within the
 * line, never over a line in between (from the hero, the shelf below it, not
 * a card two shelves down). Only targets that reach into the column set the
 * line: a shelf across the view does, the settings tabs beside the panel
 * don't. Left and right keep every target: a list row goes across to its
 * button, however far. */
function nearestLine(targets: Target[], direction: Direction, from: Box): Target[] {
  if (direction === "nav.left" || direction === "nav.right") return targets;
  const down = direction === "nav.down";
  const inColumn = targets.filter(
    (target) => gap(from.left, from.right, target.reach.left, target.reach.right) === 0,
  );
  let first: Target | undefined;
  for (const target of inColumn.length > 0 ? inColumn : targets) {
    if (!first || (down ? target.to.top < first.to.top : target.to.bottom > first.to.bottom))
      first = target;
  }
  if (!first) return [];
  const { top, bottom } = first.to;
  return targets.filter((target) => (down ? target.to.top < bottom : target.to.bottom > top));
}

/** The focusable `direction` of `from` in the current scope, entering
 * another section where it last had focus. */
export function next(from: HTMLElement, direction: Direction): HTMLElement | null {
  // Stay in the area focus is in (the sidebar, the content) while it has
  // something that way; only then cross into another. A control at the
  // content's left edge isn't "above" the sidebar's last item.
  const home = outermost(from);
  const ahead: Target[] = [];
  for (const candidate of focusables()) {
    if (candidate === from) continue;
    const to = place(candidate, from);
    const d = distance(place(from, candidate), to, direction);
    if (d === null) continue;
    ahead.push({
      element: candidate,
      to,
      reach: reach(candidate, from, to),
      d,
      inHome: home?.contains(candidate) ?? false,
    });
  }
  const area = ahead.some((target) => target.inHome)
    ? ahead.filter((target) => target.inHome)
    : ahead;
  let best: HTMLElement | null = null;
  let score = Number.POSITIVE_INFINITY;
  for (const target of nearestLine(area, direction, rect(from))) {
    if (target.d < score) {
      best = target.element;
      score = target.d;
    }
  }
  if (!best) return null;
  // Entering another section: the outermost one entered decides, so going
  // from the sidebar into the content returns to the content's last focus.
  let entered: HTMLElement | null = null;
  for (let at = section(best); at && !at.contains(from); at = section(at.parentElement ?? from)) {
    entered = at;
    if (!at.parentElement) break;
  }
  return (entered && remembered(entered)) ?? best;
}

/** The outermost focus section around `element` (the sidebar, the main
 * content), or null outside any. */
function outermost(element: Element): HTMLElement | null {
  let found: HTMLElement | null = null;
  for (let at = section(element); at; at = at.parentElement ? section(at.parentElement) : null)
    found = at;
  return found;
}

/** Where focus starts when nothing has it: the content's last focus, then
 * the view's default (`data-focus-default`), then its first focusable, then
 * anything in scope. */
export function entry(): HTMLElement | null {
  const within = scope();
  if (within === document.body) {
    const main = document.querySelector('[data-focus-section="main"]');
    if (main) {
      const preferred = main.querySelector<HTMLElement>("[data-focus-default]");
      const found =
        remembered(main) ??
        (preferred && isFocusable(preferred) ? preferred : undefined) ??
        focusables(main)[0];
      if (found) return found;
    }
  }
  const preferred = within.querySelector<HTMLElement>("[data-focus-default]");
  return (
    remembered(within) ??
    (preferred && isFocusable(preferred) ? preferred : undefined) ??
    focusables(within)[0] ??
    null
  );
}

/** A description of `element` that survives the view being rendered again. */
export interface FocusMark {
  key?: string;
  section?: string;
  index: number;
}

export function mark(element: HTMLElement): FocusMark {
  const key = element.closest<HTMLElement>("[data-focus-key]")?.dataset.focusKey;
  const around = section(element);
  const list = focusables(around ?? document.body);
  return { key, section: around?.dataset.focusSection, index: list.indexOf(element) };
}

export function find(at: FocusMark): HTMLElement | null {
  if (at.key) {
    const keyed = document.querySelector<HTMLElement>(`[data-focus-key="${CSS.escape(at.key)}"]`);
    const element = keyed && (isFocusable(keyed) ? keyed : focusables(keyed)[0]);
    if (element) return element;
  }
  const around = at.section
    ? document.querySelector(`[data-focus-section="${CSS.escape(at.section)}"]`)
    : document.body;
  if (!around || at.index < 0) return null;
  return focusables(around)[at.index] ?? null;
}

/** Whether the OS asks for reduced motion: scrolling then jumps. */
export const reducedMotion = () =>
  typeof window.matchMedia === "function" &&
  window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/** Moves focus and keeps it in view. */
export function focus(element: HTMLElement, reducedMotion: boolean) {
  // Shown as keyboard focus even after a click or a tap: a pad sends no
  // key events, so the browser's own guess would hide the ring.
  element.focus({ preventScroll: true, focusVisible: true });
  remember(element);
  box(element).scrollIntoView?.({
    block: "nearest",
    inline: "nearest",
    behavior: reducedMotion ? "auto" : "smooth",
  });
}

/** The scroll box around `element`. */
export function scroller(element: Element | null): HTMLElement | null {
  for (let at = element?.parentElement ?? null; at; at = at.parentElement) {
    const overflow = getComputedStyle(at).overflowY;
    if ((overflow === "auto" || overflow === "scroll") && at.scrollHeight > at.clientHeight)
      return at;
  }
  return document.querySelector<HTMLElement>('[data-cs="view"]');
}
