import { act } from "@testing-library/react";
import type { DropSource, NativeDrag } from "./source";

/** Native drags a test performs: `over` and `drop` land on the element
 * `target` returns (jsdom has no layout to hit-test). */
export function fakeDrops(target: () => Element | null) {
  let send: (event: NativeDrag) => void = () => {};
  const source: DropSource = async (listener) => {
    send = listener;
    return () => {
      send = () => {};
    };
  };
  const position = { x: 10, y: 10 };
  return {
    source,
    hitTest: () => target(),
    over: () => act(() => send({ type: "over", position })),
    leave: () => act(() => send({ type: "leave" })),
    drop: (...paths: string[]) => act(() => send({ type: "drop", paths, position })),
  };
}
