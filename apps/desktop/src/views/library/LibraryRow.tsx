import { artBackground, StatusBadge } from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";
import { useLingui } from "@lingui/react/macro";
import { Link } from "@tanstack/react-router";
import { Check, Ellipsis, Heart, TriangleAlert } from "lucide-react";
import { type CSSProperties, memo, useState } from "react";
import {
  formatDate,
  formatDay,
  formatDuration,
  formatPlaytime,
  formatSize,
} from "../../api/presentation";
import { useStorageAvailability } from "../../api/StorageProvider";
import type { GameSummary, LibraryFilter, LibraryLayout, LibrarySort } from "../../api/types";
import { PluginRowBadges } from "../../plugins/GameSlots";
import { AgeLockLabel } from "../game/AgeGate";
import { useGameActions } from "./GameActions";

/** Title, status, last played, playtime, size, actions. */
const listColumns = "minmax(0,1fr) 7.5rem 7.5rem 5rem 5.5rem 2rem";
export const LIST_ROW_HEIGHT = 44;
export const LIST_HEADER_HEIGHT = 32;

/** A game in the library's compact table: its icon, never its cover. */
export const LibraryRow = memo(function LibraryRow({
  game,
  rowIndex,
  style,
}: {
  game: GameSummary;
  /** 1-based, counting the header row. */
  rowIndex?: number;
  /** Placement in the virtualized table. */
  style?: CSSProperties;
}) {
  const { t } = useLingui();
  const unavailableCopy = useStorageAvailability(game.id);
  const actions = useGameActions<HTMLTableRowElement>(
    game,
    rowIndex === undefined ? undefined : rowIndex - 2,
  );
  const title = game.title;
  const lastPlayed = game.lastPlayedAt ? new Date(game.lastPlayedAt) : null;
  // An icon whose cached file is gone falls back to the initial.
  const [brokenIcon, setBrokenIcon] = useState<string | null>(null);
  const icon = game.icon && game.icon !== brokenIcon ? game.icon : null;
  return (
    <tr
      ref={actions.owner}
      aria-rowindex={rowIndex}
      // Focus comes back to this game's row after its details (#150).
      data-focus-key={`game-card:${game.id}`}
      data-cs="game-row"
      data-selected={actions.chosen.selected || undefined}
      data-status={game.status}
      data-storage-status={unavailableCopy ? "unavailable" : undefined}
      className="relative grid h-11 w-full items-center gap-3 border-b border-border px-2 text-sm transition-colors duration-(--cs-motion-duration-fast) hover:bg-surface-hover"
      style={{ ...style, gridTemplateColumns: listColumns }}
    >
      <td className="flex min-w-0 items-center gap-3">
        {actions.chosen.mode && (
          <span
            data-part="select"
            aria-hidden="true"
            className={`grid size-5 shrink-0 place-items-center rounded-full border-2 ${actions.chosen.selected ? "border-accent-alt bg-accent text-accent-fg" : "border-fg-subtle"}`}
          >
            {actions.chosen.selected && <Check className="size-3.5" strokeWidth={3} />}
          </span>
        )}
        <span
          data-part="icon"
          aria-hidden="true"
          className="grid size-7 shrink-0 place-items-center overflow-hidden rounded-control font-display text-xs font-semibold text-fg uppercase"
          style={icon ? undefined : { background: artBackground({}) }}
        >
          {icon ? (
            <img
              src={icon}
              onError={() => setBrokenIcon(icon)}
              alt=""
              loading="lazy"
              decoding="async"
              className="size-full object-contain"
            />
          ) : (
            game.title.trim().charAt(0)
          )}
        </span>
        <Link
          {...actions.targetProps}
          to="/games/$gameId"
          params={{ gameId: game.id }}
          // Desktop: its summary panel; console: its page (`useSelectGame`).
          // In selection mode: chooses it.
          onClick={actions.onClick}
          aria-pressed={actions.chosen.mode ? actions.chosen.selected : undefined}
          data-part="title"
          // The link covers the row, so the whole row opens the game.
          className="min-w-0 truncate font-medium outline-none after:absolute after:inset-0 after:rounded-control focus-visible:after:ring-[length:var(--cs-focus-ring-width)] focus-visible:after:ring-[color:var(--cs-focus-ring-glow)] focus-visible:after:ring-inset"
        >
          {game.title}
        </Link>
        {game.adminPinNeeded && <AgeLockLabel />}
        {game.favorite && (
          <Heart
            data-part="favorite"
            aria-label={t`Favorite`}
            className="size-3.5 shrink-0 fill-accent-alt text-accent-alt"
          />
        )}
        {unavailableCopy && (
          <span
            data-part="storage-availability"
            title={t`Managed copy unavailable`}
            className="inline-flex shrink-0 items-center gap-1 text-xs text-warning"
          >
            <TriangleAlert className="size-3.5" aria-label={t`Managed copy unavailable`} />
          </span>
        )}
        <div data-part="plugins" className="contents">
          <PluginRowBadges gameId={game.id} />
        </div>
      </td>
      <td>
        <StatusBadge part="badge" status={game.status} />
      </td>
      <td
        data-part="last-played"
        className="truncate font-mono text-xs text-fg-muted"
        title={formatDate(game.lastPlayedAt)}
      >
        {lastPlayed && !Number.isNaN(lastPlayed.getTime()) ? formatDay(lastPlayed) : "—"}
      </td>
      <td
        data-part="playtime"
        className="text-right font-mono text-xs text-fg-muted"
        title={formatPlaytime(game.playtimeSeconds)}
      >
        {game.playtimeSeconds ? formatDuration(game.playtimeSeconds) : "—"}
      </td>
      <td
        data-part="size"
        className="text-right font-mono text-xs text-fg-muted"
        title={formatSize(game.sizeBytes)}
      >
        {game.sizeBytes === null ? "—" : formatSize(game.sizeBytes)}
      </td>
      <td className="relative z-10 flex justify-end">
        <button
          type="button"
          aria-label={t`Actions for ${title}`}
          aria-haspopup="menu"
          data-part="actions"
          aria-expanded={actions.isOpen}
          onClick={(event) => actions.open(event.currentTarget)}
          data-spatial="focusable"
          className="grid size-7 place-items-center rounded-control text-fg-muted outline-none hover:bg-surface-overlay hover:text-fg focus-visible:focus-ring"
        >
          <Ellipsis className="size-4" />
        </button>
      </td>
    </tr>
  );
});

const columns: {
  id: string;
  label: MessageDescriptor;
  sort?: LibrarySort;
  order?: "ascending" | "descending";
  className?: string;
}[] = [
  {
    id: "title",
    label: msg({ message: "Title", context: "library column" }),
    sort: "title",
    order: "ascending",
    className: "pl-10 text-left",
  },
  { id: "status", label: msg({ message: "Status", context: "library column" }) },
  {
    id: "last-played",
    label: msg({ message: "Last played", context: "library column" }),
    sort: "recent",
    order: "descending",
  },
  {
    id: "playtime",
    label: msg({ message: "Playtime", context: "library column" }),
    sort: "playtime",
    order: "descending",
    className: "text-right",
  },
  {
    id: "size",
    label: msg({ message: "Size", context: "library column" }),
    className: "text-right",
  },
  {
    id: "actions",
    label: msg({ message: "Actions", context: "library column" }),
    className: "sr-only",
  },
];

/** The table's column headers; the sortable ones change the library's sort. */
export function LibraryTableHeader({
  sort,
  search,
}: {
  sort: LibrarySort;
  /** The rest of the library's search params, kept when the sort changes. */
  search: { filter: LibraryFilter; q?: string; layout?: LibraryLayout; platform?: string };
}) {
  const { t } = useLingui();
  return (
    <tr
      aria-rowindex={1}
      className="grid h-8 w-full items-center gap-3 border-b border-border-strong px-2 font-mono text-[11px] font-normal tracking-widest text-fg-subtle uppercase"
      style={{ gridTemplateColumns: listColumns }}
    >
      {columns.map((column) => {
        const active = column.sort === sort;
        return (
          <th
            key={column.id}
            scope="col"
            aria-sort={active ? column.order : undefined}
            className={`truncate font-normal ${column.className ?? "text-left"}`}
          >
            {column.sort ? (
              <Link
                to="/library"
                search={{ ...search, sort: column.sort }}
                data-selected={active || undefined}
                className="rounded-control outline-none hover:text-fg focus-visible:focus-ring data-selected:text-accent-alt"
              >
                {t(column.label)}
                {active && (
                  <span aria-hidden="true">{column.order === "ascending" ? " ↑" : " ↓"}</span>
                )}
              </Link>
            ) : (
              t(column.label)
            )}
          </th>
        );
      })}
    </tr>
  );
}
