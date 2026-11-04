/** Emulation's wide tables (#61), one DOM in two layouts: a table where its
 * frame has room, and below `@6xl` each row a card with its cells labelled,
 * so a 1024 px window (a panel about 490 px wide) doesn't scroll sideways.
 * The controls are the same elements in both. Display changes drop a table's
 * semantics in the cards, which is why those cells carry their own labels. */
import type { ComponentProps, ReactNode } from "react";

export function StackedTable({
  head,
  headClassName,
  className = "",
  children,
}: {
  /** The header cells. */
  head: ReactNode;
  headClassName: string;
  className?: string;
  children: ReactNode;
}) {
  return (
    <div className={`@container overflow-x-auto rounded-control border border-border ${className}`}>
      <table className="block w-full text-left text-sm @6xl:table">
        <thead className={`hidden @6xl:table-header-group ${headClassName}`}>
          <tr>{head}</tr>
        </thead>
        <tbody className="block @6xl:table-row-group">{children}</tbody>
      </table>
    </div>
  );
}

export function StackedRow({ className = "", ...props }: ComponentProps<"tr">) {
  return (
    <tr
      {...props}
      className={`grid grid-cols-2 gap-x-4 gap-y-3 border-t border-border p-4 first:border-t-0 @6xl:table-row @6xl:first:border-t ${className}`}
    />
  );
}

export function StackedCell({
  label,
  full = false,
  className = "",
  children,
}: {
  /** Its column's name, shown in the card. */
  label?: ReactNode;
  /** Takes the card's whole width. */
  full?: boolean;
  className?: string;
  children: ReactNode;
}) {
  return (
    <td
      className={`block min-w-0 break-words @6xl:table-cell ${full ? "col-span-full" : ""} ${className}`}
    >
      {label && (
        <div data-part="column-label" className="mb-1 text-xs text-fg-muted @6xl:hidden">
          {label}
        </div>
      )}
      {children}
    </td>
  );
}
