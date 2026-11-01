import { Button, ContextMenu, contextMenuTarget, MenuItem, useContextMenu } from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { Link, useNavigate } from "@tanstack/react-router";
import { ListChecks, Pin, Plus, Sparkles, SquareLibrary } from "lucide-react";
import { type CSSProperties, useRef, useState } from "react";
import { useCollections, useCollectionsApi } from "../api/collections";
import type { CollectionSummary } from "../api/types";
import type { MenuAction } from "../components/ActionMenu";
import { CollectionCard } from "../components/CollectionCard";
import { CollectionDialog } from "../components/collections/CollectionDialog";
import { useCollectionActions } from "../components/collections/useCollectionActions";
import { useInputAction, useScreenTabs } from "../input";

export const collectionFilters = ["all", "smart", "manual"] as const;
export type CollectionFilter = (typeof collectionFilters)[number];

const filterDefs: Record<
  CollectionFilter,
  { label: MessageDescriptor; match: (c: CollectionSummary) => boolean }
> = {
  all: { label: msg`All Collections`, match: () => true },
  smart: {
    label: msg({
      message: "Smart",
      context: "collection type",
      comment: "Collections that save a library filter and keep themselves up to date",
    }),
    match: (c) => c.kind.type === "smart",
  },
  manual: {
    label: msg({
      message: "Manual",
      context: "collection type",
      comment: "Collections whose games are picked by hand",
    }),
    match: (c) => c.kind.type === "manual",
  },
};

export function CollectionsView({ filter }: { filter: CollectionFilter }) {
  const { t } = useLingui();
  const api = useCollectionsApi();
  const collections = useCollections();
  // Pinned first, then by name: the core's order.
  const all = collections.data ?? [];
  const visible = all.filter(filterDefs[filter].match);
  const navigate = useNavigate();
  const tablist = useRef<HTMLDivElement>(null);
  const [creating, setCreating] = useState(false);
  const commands = useCollectionActions();
  useScreenTabs(
    collectionFilters,
    filter,
    (next) => void navigate({ to: "/collections", search: { filter: next }, replace: true }),
    { tabs: tablist },
  );
  const open = (id: string) =>
    void navigate({ to: "/collections/$collectionId", params: { collectionId: id } });

  const stats = [
    { id: "all", icon: SquareLibrary, value: all.length, label: t`Collections` },
    {
      id: "smart",
      icon: Sparkles,
      value: all.filter(filterDefs.smart.match).length,
      label: t(filterDefs.smart.label),
    },
    {
      id: "manual",
      icon: ListChecks,
      value: all.filter(filterDefs.manual.match).length,
      label: t(filterDefs.manual.label),
    },
    {
      id: "pinned",
      icon: Pin,
      value: all.filter((c) => c.pinned).length,
      label: t({ message: "Pinned", context: "collection stats" }),
    },
  ];

  return (
    <div data-view="collections">
      <section
        data-cs="panel"
        className="flex flex-wrap items-end justify-between gap-6 rounded-panel border border-border bg-surface-raised p-8"
      >
        <div>
          <div className="flex items-center gap-2 font-mono text-xs tracking-[0.2em] text-accent-alt uppercase">
            <SquareLibrary className="size-4" /> <Trans>Collections</Trans>
          </div>
          <h1 className="mt-2 font-display text-4xl font-semibold tracking-wide">
            <Trans>Your Collections</Trans>
          </h1>
          <p className="mt-2 text-fg-muted">
            <Trans>
              Hand-pick games into lists, or save a filter as a smart collection that keeps itself
              up to date.
            </Trans>
          </p>
          <div className="mt-6 flex flex-wrap gap-3">
            {stats.map(({ id, icon: Icon, value, label }, i) => (
              <div
                key={id}
                style={{ "--i": i } as CSSProperties}
                className="chamfer flex animate-enter items-center gap-3 bg-surface-overlay px-4 py-3"
              >
                <Icon className="size-6 text-accent-alt" />
                <div>
                  <div className="font-mono text-lg font-semibold">{value}</div>
                  <div className="text-xs text-fg-muted">{label}</div>
                </div>
              </div>
            ))}
          </div>
        </div>
        {api && (
          <div data-part="primary-action" className="animate-enter [--i:4]">
            <button
              type="button"
              onClick={() => setCreating(true)}
              className="chamfer relative flex items-center gap-2 bg-linear-to-r from-accent to-accent-strong px-6 py-2.5 font-display font-semibold tracking-wider text-accent-fg uppercase hover:brightness-110 focus-visible:focus-ring-chamfer"
            >
              <Plus className="size-4" />
              <Trans>New Collection</Trans>
            </button>
          </div>
        )}
      </section>

      <div ref={tablist} role="tablist" className="mt-6 mb-4 flex flex-wrap gap-2">
        {collectionFilters.map((f) => {
          const count = all.filter(filterDefs[f].match).length;
          const selected = f === filter;
          return (
            <Link
              key={f}
              to="/collections"
              search={{ filter: f }}
              replace
              role="tab"
              aria-selected={selected}
              data-selected={selected || undefined}
              className="rounded-full border border-border bg-surface-raised px-4 py-1.5 text-sm text-fg-muted transition-colors duration-(--cs-motion-duration-fast) hover:text-fg data-selected:border-accent-alt/50 data-selected:bg-accent/20 data-selected:text-fg data-selected:shadow-[0_0_16px_-6px] data-selected:shadow-accent-alt"
            >
              {t(filterDefs[f].label)}{" "}
              <span className="font-mono text-xs text-fg-subtle">{count}</span>
            </Link>
          );
        })}
      </div>

      {!api ? (
        <p data-part="empty" className="text-fg-muted">
          <Trans>Collections aren't available here.</Trans>
        </p>
      ) : collections.error ? (
        <div role="alert" className="rounded-panel border border-border p-8">
          <p>
            <Trans>Could not load your collections.</Trans>
          </p>
          <p className="mt-2 text-sm text-fg-muted">{collections.error.message}</p>
          <Button className="mt-4" onPress={() => void collections.refetch()}>
            <Trans>Try again</Trans>
          </Button>
        </div>
      ) : collections.isPending ? (
        <p role="status" className="text-fg-muted">
          <Trans>Loading collections…</Trans>
        </p>
      ) : (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(13rem,1fr))] gap-4">
          {visible.map((collection, i) => (
            <CollectionTile
              key={collection.id}
              collection={collection}
              index={i}
              actions={commands.actions(collection)}
            />
          ))}
          <button
            type="button"
            data-cs="new-collection"
            onClick={() => setCreating(true)}
            style={{ "--i": visible.length } as CSSProperties}
            className="flex min-h-48 animate-enter flex-col items-center justify-center gap-2 rounded-card border border-dashed border-border-strong text-fg-subtle transition-colors duration-(--cs-motion-duration-fast) hover:border-accent-alt/50 hover:bg-surface-raised hover:text-accent-alt"
          >
            <Plus className="size-6" />
            <span className="text-sm font-medium">
              <Trans>New Collection</Trans>
            </span>
            <span className="text-xs">
              {all.length === 0 ? t`Make your first one` : t`Manual list or smart filter`}
            </span>
          </button>
        </div>
      )}
      {commands.error && (
        <p role="alert" className="mt-4 text-sm text-danger">
          {commands.error.message}
        </p>
      )}
      {creating && (
        <CollectionDialog
          preset={
            filter === "smart"
              ? { type: "smart", query: { filter: "installed", sort: "title" } }
              : undefined
          }
          close={() => setCreating(false)}
          onSaved={open}
        />
      )}
      {commands.dialogs}
    </div>
  );
}

/** A collection's card, with its menu at a right click, the context menu
 * key or a pad's Start. */
function CollectionTile({
  collection,
  index,
  actions,
}: {
  collection: CollectionSummary;
  index: number;
  actions: MenuAction[];
}) {
  const { t } = useLingui();
  const owner = useRef<HTMLDivElement>(null);
  const menu = useContextMenu();
  useInputAction("menu", () => owner.current && menu.open(owner.current), {
    label: t`Options`,
    owner,
  });
  return (
    <div ref={owner} {...contextMenuTarget(menu.open)}>
      <CollectionCard collection={collection} index={index} />
      {menu.anchor && (
        <ContextMenu aria-label={collection.name} anchor={menu.anchor} onClose={menu.close}>
          {actions.map(({ id, label, onAction, ...item }) => (
            <MenuItem key={id} id={id} {...item} onAction={onAction}>
              {label}
            </MenuItem>
          ))}
        </ContextMenu>
      )}
    </div>
  );
}
