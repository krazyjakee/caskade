/** A row of cards that pages with LT/RT (#148, #150), like every list. It is a focus section,
 * so focus comes back to the card it left; in console mode the header shows
 * the paging buttons. */
import { useLingui } from "@lingui/react/macro";
import type { LucideIcon } from "lucide-react";
import { type ReactNode, type RefObject, useId, useRef } from "react";
import { useInputAction } from "../input";
import { focusables, reducedMotion } from "../input/spatial";
import { glyph } from "../shell/console/glyphs";
import { useHintFamily } from "../shell/console/HintBar";
import { useMode } from "../shell/console/mode";
import { GAME_GRID_ROW_HEIGHT, useGameCardLayout } from "./gameCardLayout";

/** Moves focus a page of cards along `items` (each child is a card). */
export function pageShelf(items: HTMLElement, direction: 1 | -1): boolean {
  const cards = Array.from(items.children).filter(
    (card): card is HTMLElement => card instanceof HTMLElement,
  );
  const at = cards.findIndex((card) => card.contains(document.activeElement));
  if (at < 0) return false;
  const card = cards[at]?.getBoundingClientRect().width ?? 0;
  const visible = Math.max(1, Math.floor(items.getBoundingClientRect().width / (card || 1)));
  const next = Math.min(cards.length - 1, Math.max(0, at + direction * visible));
  const target = cards[next];
  const stop = target && focusables(target)[0];
  if (!stop) return true;
  stop.focus({ preventScroll: true });
  target.scrollIntoView?.({
    inline: "start",
    block: "nearest",
    behavior: reducedMotion() ? "auto" : "smooth",
  });
  return true;
}

/** LT/RT page the shelf while focus is in it. */
export function useShelfPaging(shelf: RefObject<HTMLElement | null>) {
  const { t } = useLingui();
  const items = () => shelf.current?.querySelector<HTMLElement>('[data-part="items"]') ?? null;
  useInputAction(
    "page.prev",
    () => {
      const row = items();
      return row ? pageShelf(row, -1) : false;
    },
    { label: t`Previous page`, owner: shelf },
  );
  useInputAction(
    "page.next",
    () => {
      const row = items();
      return row ? pageShelf(row, 1) : false;
    },
    { label: t`Next page`, owner: shelf },
  );
}

export function ShelfRow({
  id,
  title,
  icon: Icon,
  viewAll,
  notice,
  children,
}: {
  id: string;
  title: string;
  icon: LucideIcon;
  /** The "View all" link. */
  viewAll?: ReactNode;
  /** Loading, error and empty states, above the row. */
  notice?: ReactNode;
  children: ReactNode;
}) {
  const shelf = useRef<HTMLElement>(null);
  const items = useRef<HTMLDivElement>(null);
  const { columns } = useGameCardLayout(items);
  const heading = useId();
  const { mode } = useMode();
  const family = useHintFamily();
  useShelfPaging(shelf);
  return (
    <section
      ref={shelf}
      data-cs="shelf-row"
      data-focus-section={`shelf-${id}`}
      aria-labelledby={heading}
      className="mt-8"
    >
      <header data-part="header" className="mb-4 flex items-center gap-3">
        <Icon data-part="icon" className="size-5 text-accent-alt" />
        <h2 id={heading} data-part="title" className="font-display text-xl font-semibold">
          {title}
        </h2>
        <span className="ml-auto flex items-center gap-4">
          {mode === "console" && (
            <span data-part="pager" aria-hidden className="flex gap-1.5">
              {(["page.prev", "page.next"] as const).map((action) => (
                <kbd
                  key={action}
                  className="rounded-control border border-border-strong px-2 py-0.5 font-mono text-xs text-fg-muted"
                >
                  {glyph(action, family)}
                </kbd>
              ))}
            </span>
          )}
          {viewAll}
        </span>
      </header>
      {notice}
      <div
        ref={items}
        data-part="items"
        className="grid grid-flow-col gap-4 overflow-x-auto pb-4"
        style={{
          gridAutoColumns: `calc((100% - var(--spacing) * ${4 * (columns - 1)}) / ${columns})`,
          gridAutoRows: `calc(${GAME_GRID_ROW_HEIGHT}px - var(--spacing) * 4)`,
        }}
      >
        {children}
      </div>
    </section>
  );
}
