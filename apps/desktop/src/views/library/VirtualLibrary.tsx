import { useLingui } from "@lingui/react/macro";
import { useVirtualizer } from "@tanstack/react-virtual";
import { type ReactNode, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { GameSummary, LibraryLayout, TitleLetter } from "../../api/types";
import { GAME_GRID_ROW_HEIGHT, useGameCardLayout } from "../../components/gameCardLayout";
import { useDefaultFocus, useInputAction } from "../../input";
import { focus } from "../../input/spatial";
import { AlphabetBar } from "./AlphabetBar";
import { LibraryCard } from "./LibraryCard";
import { LIST_HEADER_HEIGHT, LIST_ROW_HEIGHT, LibraryRow } from "./LibraryRow";

/** Rows are virtualized in both modes: artwork and DOM cost follow the
 * viewport, and the games come from `game` by index, loaded around the
 * range reported to `onRange`. The list is a compact table under a sticky
 * `header`. With `letters`, an alphabet bar beside it jumps to them. */
export function VirtualLibrary({
  game: gameAt,
  total,
  layout,
  header,
  loading,
  onRange,
  letters,
}: {
  /** The game at an index, or undefined while it loads. */
  game: (index: number) => GameSummary | undefined;
  total: number;
  layout: LibraryLayout;
  /** The list's column headers. */
  header?: ReactNode;
  loading: boolean;
  /** The games on screen, as [start, end) indexes. */
  onRange: (start: number, end: number) => void;
  /** Where each initial letter starts, when the games are sorted by title. */
  letters?: readonly TitleLetter[];
}) {
  const { t } = useLingui();
  const scrollRef = useRef<HTMLDivElement>(null);
  const cardLayout = useGameCardLayout(scrollRef);
  const list = layout === "list";
  const columns = list ? 1 : cardLayout.columns;
  const rowHeight = list ? LIST_ROW_HEIGHT : GAME_GRID_ROW_HEIGHT;
  // The rows start below the sticky header.
  const scrollMargin = list ? LIST_HEADER_HEIGHT : 0;
  const virtualizer = useVirtualizer({
    count: Math.ceil(total / columns),
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowHeight,
    scrollMargin,
    // A jump puts its row below the sticky header, not under it.
    scrollPaddingStart: scrollMargin,
    // One extra grid row on each side keeps keyboard/scroll continuity while
    // avoiding mounting controls for two additional invisible rows. List
    // rows are short, so a few more cost little.
    overscan: list ? 6 : 1,
    getItemKey: (row) => `${layout}-${columns}-${row}`,
  });
  useEffect(() => {
    if (rowHeight > 0) virtualizer.measure();
  }, [virtualizer, rowHeight]);
  const rows = virtualizer.getVirtualItems();
  const firstRow = rows[0]?.index ?? 0;
  const lastRow = rows.at(-1)?.index ?? 0;
  useEffect(() => {
    onRange(firstRow * columns, (lastRow + 1) * columns);
  }, [firstRow, lastRow, columns, onRange]);
  const jump = useCallback(
    (offset: number) => virtualizer.scrollToIndex(Math.floor(offset / columns), { align: "start" }),
    [virtualizer, columns],
  );
  // The letter of the first game below the header (letters are in order).
  const top = virtualizer.getVirtualItemForOffset((virtualizer.scrollOffset ?? 0) + scrollMargin);
  const firstShown = (top?.index ?? 0) * columns;
  const current = letters?.findLast((letter) => letter.offset <= firstShown)?.letter ?? null;

  // A pad starts on the first game once it is on screen (its row can
  // render after its data).
  const [firstStop, setFirstStop] = useState<HTMLElement | null>(null);
  useLayoutEffect(() => {
    const stop = scrollRef.current ? stopAt(scrollRef.current, 0, columns, list) : null;
    setFirstStop((before) => (before === stop ? before : stop));
  });
  const first = useRef<HTMLElement | null>(null);
  first.current = firstStop;
  useDefaultFocus(first, Boolean(firstStop), gameAt(0)?.id);
  // LT/RT page through the games, or go letter by letter when sorted by
  // title; focus goes to the first game there once it has loaded.
  const focusIndex = (index: number) => {
    let frames = 0;
    const attempt = () => {
      const box = scrollRef.current;
      const target = box && stopAt(box, index, columns, list);
      if (target) focus(target, true);
      else if (++frames < 90) requestAnimationFrame(attempt);
    };
    requestAnimationFrame(attempt);
  };
  const page = (direction: 1 | -1) => {
    const box = scrollRef.current;
    if (!box || total === 0) return false;
    const from = indexOf(document.activeElement, box, columns, list) ?? firstShown;
    if (letters?.length) {
      const at = letters.findLastIndex((letter) => letter.offset <= from);
      // Back goes to the start of this letter first, then the one before.
      const here = letters[at];
      const target =
        direction > 0 ? letters[at + 1] : here && from > here.offset ? here : letters[at - 1];
      if (target) {
        jump(target.offset);
        focusIndex(target.offset);
      }
      return true;
    }
    const perPage =
      Math.max(1, Math.floor((box.clientHeight - scrollMargin) / rowHeight)) * columns;
    const to = Math.min(total - 1, Math.max(0, from + direction * perPage));
    virtualizer.scrollToIndex(Math.floor(to / columns), { align: "start" });
    focusIndex(to);
    return true;
  };
  const byLetter = Boolean(letters?.length);
  useInputAction("page.prev", () => page(-1), {
    label: byLetter ? t`Previous letter` : t`Previous page`,
  });
  useInputAction("page.next", () => page(1), {
    label: byLetter ? t`Next letter` : t`Next page`,
  });
  // The box is the same element with the bar and without: the width is
  // observed on the one it mounted with.
  return (
    <div className="flex min-h-96 grow basis-0 gap-2">
      <section
        ref={scrollRef}
        data-cs="virtual-library"
        data-layout={layout}
        // A pad moves between the cards; the box itself is only a Tab stop.
        data-focus-section="library-grid"
        data-spatial="skip"
        aria-label={t`Library games`}
        aria-busy={loading}
        className="min-w-0 grow overflow-auto overscroll-contain"
        // biome-ignore lint/a11y/noNoninteractiveTabindex: keyboard users must be able to scroll this bounded viewport.
        tabIndex={0}
      >
        {list ? (
          <table aria-label={t`Games`} aria-rowcount={total + 1} className="block w-full">
            <thead
              data-part="table-header"
              className="sticky top-0 z-20 block bg-surface-base"
              style={{ height: LIST_HEADER_HEIGHT }}
            >
              {header}
            </thead>
            <tbody className="relative block w-full" style={{ height: virtualizer.getTotalSize() }}>
              {rows.map((row) => {
                const game = gameAt(row.index);
                const place = {
                  position: "absolute",
                  top: 0,
                  left: 0,
                  height: rowHeight,
                  transform: `translateY(${row.start - scrollMargin}px)`,
                } as const;
                return game ? (
                  <LibraryRow key={row.key} game={game} rowIndex={row.index + 2} style={place} />
                ) : (
                  <tr
                    key={row.key}
                    aria-rowindex={row.index + 2}
                    data-part="placeholder"
                    className="block w-full border-b border-border"
                    style={place}
                  >
                    <td className="sr-only">{t`Loading game`}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        ) : (
          <div className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
            {rows.map((row) => (
              <div
                key={row.key}
                data-part="virtual-row"
                data-row={row.index}
                className="absolute top-0 left-0 grid w-full gap-4 pb-4"
                style={{
                  height: rowHeight,
                  transform: `translateY(${row.start}px)`,
                  gridTemplateColumns: `repeat(${columns},minmax(0,1fr))`,
                }}
              >
                {Array.from(
                  { length: Math.min(columns, total - row.index * columns) },
                  (_, col) => {
                    const index = row.index * columns + col;
                    const game = gameAt(index);
                    return game ? (
                      <LibraryCard key={game.id} game={game} index={index} />
                    ) : (
                      <div
                        key={`loading-${index}`}
                        data-part="placeholder"
                        role="status"
                        className="rounded-card border border-border bg-surface-raised"
                        aria-label={t`Loading game`}
                      />
                    );
                  },
                )}
              </div>
            ))}
          </div>
        )}
      </section>
      {letters && <AlphabetBar letters={letters} current={current} onJump={jump} />}
    </div>
  );
}

/** The focus stop of the game at `index`: its card's or row's link. */
function stopAt(box: HTMLElement, index: number, columns: number, list: boolean) {
  if (list) return box.querySelector<HTMLElement>(`tr[aria-rowindex="${index + 2}"] a[href]`);
  const row = box.querySelector(
    `[data-part="virtual-row"][data-row="${Math.floor(index / columns)}"]`,
  );
  return row?.children[index % columns]?.querySelector<HTMLElement>("a[href]") ?? null;
}

/** The index of the game `element` is in, or null outside the games. */
function indexOf(element: Element | null, box: HTMLElement, columns: number, list: boolean) {
  if (!element || !box.contains(element)) return null;
  if (list) {
    const row = element.closest("tr[aria-rowindex]")?.getAttribute("aria-rowindex");
    return row ? Number(row) - 2 : null;
  }
  const row = element.closest<HTMLElement>('[data-part="virtual-row"]');
  if (!row?.dataset.row) return null;
  const cell = Array.from(row.children).findIndex((child) => child.contains(element));
  return cell < 0 ? null : Number(row.dataset.row) * columns + cell;
}
