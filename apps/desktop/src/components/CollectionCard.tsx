import { i18n } from "@lingui/core";
import { msg } from "@lingui/core/macro";
import { useLingui } from "@lingui/react/macro";
import { Link } from "@tanstack/react-router";
import { Clock, Gamepad2, ListChecks, Pin, Sparkles } from "lucide-react";
import type { CSSProperties } from "react";
import { formatDuration, formatNumber, formatRelativeDay } from "../api/presentation";
import type { CollectionKind, CollectionSummary } from "../api/types";

const kinds = {
  smart: {
    label: msg({ message: "Smart", context: "collection kind" }),
    icon: Sparkles,
    className: "border-accent-alt/40 text-accent-alt",
  },
  manual: {
    label: msg({ message: "Manual", context: "collection kind" }),
    icon: ListChecks,
    className: "border-border-strong text-fg",
  },
} satisfies Record<CollectionKind["type"], unknown>;

export function KindBadge({ kind }: { kind: CollectionKind["type"] }) {
  const { t } = useLingui();
  const { label, icon: Icon, className } = kinds[kind];
  return (
    <span
      data-part="badge"
      data-kind={kind}
      className={`inline-flex items-center gap-1 rounded-full border bg-surface-base/70 px-2 py-0.5 text-[11px] font-medium backdrop-blur ${className}`}
    >
      <Icon className="size-3" />
      {t(label)}
    </span>
  );
}

/** 2×2 mosaic of the collection's first covers, repeating when it has
 * fewer than four. */
export function CollectionMosaic({
  covers,
  className = "",
}: {
  covers: readonly string[];
  className?: string;
}) {
  return (
    <div
      data-part="art"
      className={`grid grid-cols-2 grid-rows-2 gap-px overflow-hidden bg-surface-base ${className}`}
    >
      {covers.length === 0 ? (
        <div className="col-span-2 row-span-2 grid place-items-center bg-linear-150 from-surface-overlay to-surface-base">
          <Gamepad2 className="size-8 text-fg-subtle" />
        </div>
      ) : (
        [0, 1, 2, 3].map((cell) => (
          <img
            key={cell}
            src={covers[cell % covers.length]}
            alt=""
            loading="lazy"
            decoding="async"
            draggable={false}
            className="size-full min-h-0 object-cover"
          />
        ))
      )}
    </div>
  );
}

/** "Updated today", "Updated 3 days ago"; a smart collection follows the
 * library, so it is "Live". */
export function collectionUpdated(collection: CollectionSummary) {
  if (collection.kind.type === "smart")
    return i18n._(
      msg({
        message: "Live",
        context: "collection",
        comment: "A smart collection, which follows the library as it changes",
      }),
    );
  const when =
    collection.updatedAt && formatRelativeDay(collection.updatedAt, { standalone: false });
  return when
    ? i18n._(msg({ message: `Updated ${when}`, comment: "{when} is a day: “today”, “3 days ago”" }))
    : "";
}

export function CollectionCard({
  collection,
  index = 0,
}: {
  collection: CollectionSummary;
  index?: number;
}) {
  const { t } = useLingui();
  return (
    <Link
      to="/collections/$collectionId"
      params={{ collectionId: collection.id }}
      data-cs="collection-card"
      data-kind={collection.kind.type}
      data-focus-key={`collection-card:${collection.id}`}
      style={{ "--i": index } as CSSProperties}
      className="relative flex w-full animate-enter flex-col overflow-hidden rounded-card border border-border bg-surface-raised outline-none hover:bg-surface-hover focus-visible:focus-ring-over"
    >
      <div className="relative">
        <CollectionMosaic covers={collection.covers} className="aspect-(--cs-card-aspect) w-full" />
        <div
          aria-hidden
          className="absolute inset-0 bg-linear-to-t from-surface-base/80 via-transparent to-transparent"
        />
        <div className="absolute top-2 left-2">
          <KindBadge kind={collection.kind.type} />
        </div>
        {collection.pinned && (
          <Pin
            data-part="pin"
            aria-label={t({ message: "Pinned", context: "collection" })}
            className="absolute top-2.5 right-2.5 size-4 rotate-45 fill-accent-alt/30 text-accent-alt drop-shadow"
          />
        )}
        <span
          data-part="count"
          className="absolute right-3 bottom-2 font-display text-3xl font-semibold text-white/90 drop-shadow"
        >
          {formatNumber(collection.games)}
        </span>
      </div>
      <div className="flex flex-col gap-1 px-3 py-2.5">
        <span data-part="title" className="truncate text-sm font-semibold">
          {collection.name}
        </span>
        <span data-part="subtitle" className="truncate text-xs text-fg-subtle">
          {collection.description || " "}
        </span>
        <div
          data-part="meta"
          className="flex items-center justify-between font-mono text-[11px] text-fg-muted"
        >
          <span className="inline-flex items-center gap-1">
            <Clock className="size-3.5" />
            {collection.playtimeSeconds > 0 ? formatDuration(collection.playtimeSeconds) : "—"}
          </span>
          <span>{collectionUpdated(collection)}</span>
        </div>
      </div>
    </Link>
  );
}
