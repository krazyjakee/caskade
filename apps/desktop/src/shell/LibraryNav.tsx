import { useLingui } from "@lingui/react/macro";
import { Link, useRouterState } from "@tanstack/react-router";
import { useLibraryStats } from "../api/hooks";
import { filterLabels, libraryFilters } from "../views/library/LibraryView";

/** The library's filters, under its link in the sidebar. They slide open
 * while the library is open and closed elsewhere. A filter keeps the
 * library's search; its sort and layout are the remembered ones. */
export function LibraryNav({ console = false }: { console?: boolean }) {
  const { t } = useLingui();
  const stats = useLibraryStats();
  const location = useRouterState({ select: ({ location }) => location });
  const open = location.pathname === "/library";
  const query = open ? (location.search as { q?: string }).q : undefined;
  return (
    // Rows from 0fr to 1fr animate the list's height to its content.
    <div
      data-part="sub-nav"
      data-open={open || undefined}
      // Closed, it also takes back the sidebar's gap above it.
      className={`grid grid-rows-[0fr] opacity-0 transition-[grid-template-rows,opacity,margin] duration-(--cs-motion-duration-base) ease-(--cs-motion-ease-out) data-open:mb-0 data-open:grid-rows-[1fr] data-open:opacity-100 ${console ? "-mb-1.5" : "-mb-1"}`}
    >
      <nav
        aria-label={t`Library filters`}
        // Closed, its links are out of the tab order and the pad's reach.
        inert={!open}
        className={`flex min-h-0 flex-col gap-0.5 overflow-hidden border-l border-border ${console ? "ml-7 pl-2" : "ml-5 pl-2"}`}
      >
        {libraryFilters.map((filter) => (
          <Link
            key={filter}
            to="/library"
            search={{ filter, q: query }}
            data-part="sub-item"
            className={`flex items-center gap-2 rounded-control px-3 text-fg-muted transition-colors duration-(--cs-motion-duration-fast) hover:bg-surface-hover hover:text-fg focus-visible:bg-surface-hover focus-visible:text-fg data-[status=active]:bg-accent/15 data-[status=active]:text-fg ${console ? "py-2 text-base" : "py-1.5 text-sm"}`}
          >
            <span
              data-part="label"
              // Console mode's text is larger: a long name takes two lines.
              className={console ? "min-w-0 flex-1 leading-tight" : "flex-1 truncate"}
            >
              {t(filterLabels[filter])}
            </span>
            <span data-part="count" className="font-mono text-xs text-fg-subtle">
              {stats.data?.[filter === "all" ? "total" : filter] ?? ""}
            </span>
          </Link>
        ))}
      </nav>
    </div>
  );
}
