/** Key presses the input layer sends itself (Escape, arrows, Enter). */

/** Events we dispatch ourselves must not loop back into the keyboard map. */
export const synthetic = new WeakSet<Event>();

/** Presses `key` on `element`; returns whether a handler took the keydown. */
export function press(element: Element, key: string): boolean {
  let taken = false;
  for (const type of ["keydown", "keyup"] as const) {
    const event = new KeyboardEvent(type, { key, bubbles: true, cancelable: true });
    synthetic.add(event);
    element.dispatchEvent(event);
    if (type === "keydown") taken = event.defaultPrevented;
  }
  return taken;
}
