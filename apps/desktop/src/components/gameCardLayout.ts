import { type RefObject, useLayoutEffect, useState } from "react";

const GAME_GRID_GAP = 16;
export const GAME_GRID_ROW_HEIGHT = 312;

/** Home shelves and the library use the same card dimensions at each width. */
export function useGameCardLayout(container: RefObject<HTMLElement | null>) {
  const [width, setWidth] = useState(900);
  // Measure before paint so cards do not mount at the wrong column count.
  useLayoutEffect(() => {
    const element = container.current;
    if (!element) return;
    if (element.clientWidth > 0) setWidth(element.clientWidth);
    const observer = new ResizeObserver(([entry]) => {
      if (entry) setWidth(entry.contentRect.width);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [container]);
  const columns = Math.max(1, Math.floor((width + GAME_GRID_GAP) / (176 + GAME_GRID_GAP)));
  return { columns };
}
