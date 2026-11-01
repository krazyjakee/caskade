import { Menu, MenuItem, MenuPopover } from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg, plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { ArrowLeft, CheckSquare, Ellipsis, Pencil, Plus, SearchX, Shuffle } from "lucide-react";
import { useRef } from "react";
import { Button, MenuTrigger } from "react-aria-components";
import { useCollection, useCollectionsApi, useCollectionWindow } from "../api/collections";
import { useGameEditApi } from "../api/gameEdit";
import { formatDuration, formatNumber } from "../api/presentation";
import type { CollectionSummary, CollectionsApi, GameSummary, LibrarySort } from "../api/types";
import { CollectionMosaic, collectionUpdated, KindBadge } from "../components/CollectionCard";
import { describeRules } from "../components/collections/rules";
import { useCollectionActions } from "../components/collections/useCollectionActions";
import { usePlayGame } from "../components/gameMenu/GameMenuHost";
import { SelectionBar } from "../components/gameSelection/SelectionBar";
import {
  SelectionScope,
  useGameSelectionStore,
  useSelectionVersion,
} from "../components/gameSelection/selection";
import { useDefaultFocus, useInputAction } from "../input";
import { loadedIds } from "./library/LibraryView";
import { VirtualLibrary } from "./library/VirtualLibrary";

/** Play Random looks at this many games at most, and picks an installed
 * one where it can. */
const RANDOM_SAMPLE = 1000;

/** A smart collection's sort, in its header. */
const sortedBy: Record<LibrarySort, MessageDescriptor> = {
  title: msg`Sorted by title`,
  recent: msg`Sorted by recently played`,
  playtime: msg`Sorted by playtime`,
  added: msg`Sorted by recently added`,
  released: msg`Sorted by release date`,
  popular: msg`Sorted by popularity`,
};

export function CollectionView({ collectionId }: { collectionId: string }) {
  const collection = useCollection(collectionId);
  if (collection.error)
    return (
      <div role="alert" className="rounded-panel border border-border p-8">
        <p>
          <Trans>Could not load this collection.</Trans>
        </p>
        <p className="mt-2 text-sm text-fg-muted">{collection.error.message}</p>
      </div>
    );
  if (collection.data === undefined)
    return (
      <p role="status" className="text-fg-muted">
        <Trans>Loading collection…</Trans>
      </p>
    );
  if (collection.data === null)
    return (
      <div data-view="collection" className="rounded-panel border border-border p-8">
        <p>
          <Trans>This collection doesn't exist any more.</Trans>
        </p>
        <BackLink />
      </div>
    );
  return <CollectionPage collection={collection.data} />;
}

function BackLink() {
  return (
    <Link
      to="/collections"
      search={{ filter: "all" }}
      className="group mb-3 inline-flex items-center gap-1.5 font-mono text-xs tracking-wider text-fg-muted uppercase hover:text-accent-alt"
    >
      <ArrowLeft className="size-4 transition-transform duration-(--cs-motion-duration-fast) group-hover:-translate-x-1" />
      <Trans>All Collections</Trans>
    </Link>
  );
}

function CollectionPage({ collection }: { collection: CollectionSummary }) {
  const { t } = useLingui();
  const navigate = useNavigate();
  const api = useCollectionsApi();
  const games = useCollectionWindow(collection.id);
  const commands = useCollectionActions({
    onDeleted: () => void navigate({ to: "/collections", search: { filter: "all" } }),
  });
  const play = usePlayGame();
  const random = useMutation({
    mutationFn: async () => {
      if (!api) throw new Error(t`Collections aren't available here`);
      const game = await pickRandom(api, collection);
      if (!game) throw new Error(t`This collection has no games.`);
      play?.({ gameId: game.id, title: game.title });
    },
  });
  const primary = useRef<HTMLButtonElement>(null);
  useDefaultFocus(primary);
  // Choosing several of its games for one edit (#173).
  const selection = useGameSelectionStore(games.game);
  useSelectionVersion(selection);
  const edits = useGameEditApi();
  useInputAction("back", () => selection.stop(), {
    label: t`Done selecting`,
    enabled: selection.active,
  });
  const smart = collection.kind.type === "smart" ? collection.kind.query : null;
  const total = games.total ?? collection.games;
  const played = formatDuration(collection.playtimeSeconds);
  const meta = [
    plural(total, { one: "# game", other: "# games" }),
    collection.playtimeSeconds > 0 &&
      t({ message: `${played} played`, comment: "{played} is a duration: “12h 30m”" }),
    smart && t(sortedBy[smart.sort]),
    collectionUpdated(collection),
  ].filter(Boolean);
  const actions = commands.actions(collection);
  const name = collection.name;
  const count = formatNumber(total);
  const failure = random.error?.message ?? commands.error?.message;

  return (
    <div data-view="collection" data-kind={collection.kind.type} className="flex flex-1 flex-col">
      <div>
        <BackLink />
      </div>
      <section
        data-cs="panel"
        className="relative shrink-0 overflow-hidden rounded-panel border border-border bg-surface-raised p-8"
      >
        <CollectionMosaic
          covers={collection.covers}
          className="absolute inset-y-0 right-0 w-1/2 opacity-60 mask-l-from-30% mask-l-to-100%"
        />

        <div className="relative max-w-xl">
          <div className="animate-enter">
            <KindBadge kind={collection.kind.type} />
          </div>
          <h1 className="mt-3 animate-enter font-display text-4xl font-semibold tracking-wide [--i:1]">
            {collection.name}
          </h1>
          {collection.description && (
            <p className="mt-2 animate-enter text-fg-muted [--i:2]">{collection.description}</p>
          )}
          <div
            data-part="meta"
            className="mt-3 flex animate-enter flex-wrap gap-2 font-mono text-[11px] tracking-wide text-fg-muted uppercase [--i:3]"
          >
            {meta.map((item, i) => (
              <span key={String(item)} className="flex gap-2">
                {i > 0 && <span className="text-accent-alt/60">{"//"}</span>}
                {item}
              </span>
            ))}
          </div>

          {smart && (
            <div data-part="rules" className="mt-5 animate-enter [--i:4]">
              <div className="mb-2 font-mono text-[11px] tracking-[0.2em] text-fg-subtle uppercase">
                <Trans>Matches all rules</Trans>
              </div>
              <div className="flex flex-wrap gap-2">
                {describeRules(smart).map(({ field, op, value }) => (
                  <span
                    key={`${field}-${op}-${value}`}
                    data-part="rule"
                    className="inline-flex items-center gap-1.5 rounded-control border border-accent-alt/30 bg-surface-overlay/80 px-2.5 py-1 text-xs backdrop-blur"
                  >
                    <span className="text-fg-muted">{field}</span>
                    <span className="font-mono text-accent-alt">{op}</span>
                    <span className="font-medium">{value}</span>
                  </span>
                ))}
              </div>
            </div>
          )}

          <div className="mt-6 flex animate-enter items-center gap-3 [--i:5]">
            {play && (
              <div data-part="primary-action">
                <Button
                  ref={primary}
                  data-focus-default
                  isDisabled={total === 0 || random.isPending}
                  onPress={() => random.mutate()}
                  className="chamfer relative flex items-center gap-2 bg-linear-to-r from-accent to-accent-strong px-6 py-2.5 font-display font-semibold tracking-wider text-accent-fg uppercase outline-none data-disabled:opacity-40 data-focus-visible:focus-ring-chamfer data-hovered:brightness-110"
                >
                  <Shuffle className="size-4" />
                  <Trans>Play Random</Trans>
                </Button>
              </div>
            )}
            <Button
              onPress={() => commands.open(smart ? "edit" : "add", collection)}
              className="flex items-center gap-2 rounded-control border border-border-strong bg-surface-overlay/70 px-5 py-2.5 text-sm outline-none backdrop-blur data-focus-visible:focus-ring data-hovered:border-accent-alt/50 data-hovered:bg-surface-hover"
            >
              {smart ? (
                <>
                  <Pencil className="size-4" /> <Trans>Edit Rules</Trans>
                </>
              ) : (
                <>
                  <Plus className="size-4" /> <Trans>Add Games</Trans>
                </>
              )}
            </Button>
            <MenuTrigger>
              <Button
                aria-label={t({ message: "More", comment: "The button that opens more actions" })}
                className="rounded-control border border-border-strong bg-surface-overlay/70 p-2.5 outline-none backdrop-blur data-focus-visible:focus-ring data-hovered:border-accent-alt/50 data-hovered:bg-surface-hover"
              >
                <Ellipsis className="size-4" />
              </Button>
              <MenuPopover>
                <Menu
                  aria-label={t`${name} options`}
                  onAction={(id) => actions.find((action) => action.id === id)?.onAction()}
                >
                  {actions.map(({ id, label, icon, tone, isDisabled }) => (
                    <MenuItem key={id} id={id} icon={icon} tone={tone} isDisabled={isDisabled}>
                      {label}
                    </MenuItem>
                  ))}
                </Menu>
              </MenuPopover>
            </MenuTrigger>
          </div>
          {failure && (
            <p role="alert" className="mt-3 text-sm text-danger">
              {failure}
            </p>
          )}
        </div>
      </section>

      <div className="mt-8 mb-3 flex items-center justify-between gap-3">
        <h2 className="font-display text-lg font-semibold tracking-wide">
          {t({
            message: `Games (${count})`,
            comment: "The heading over a collection's games, with how many",
          })}
        </h2>
        {edits && total > 0 && (
          <Button
            data-part="select"
            aria-pressed={selection.active}
            onPress={() => (selection.active ? selection.stop() : selection.start())}
            className="flex items-center gap-1.5 rounded-control border border-border px-3 py-1.5 text-sm text-fg-muted outline-none data-focus-visible:focus-ring data-hovered:text-fg aria-pressed:border-accent-alt aria-pressed:text-fg"
          >
            <CheckSquare className="size-4" />
            <Trans comment="Starts choosing several games in the library">Select</Trans>
          </Button>
        )}
      </div>
      {games.error ? (
        <div role="alert" className="rounded-panel border border-border p-8">
          <p>
            <Trans>Could not load the games.</Trans>
          </p>
          <p className="mt-2 text-sm text-fg-muted">{games.error.message}</p>
        </div>
      ) : total > 0 ? (
        <SelectionScope selection={selection}>
          <VirtualLibrary
            game={games.game}
            total={total}
            layout="grid"
            loading={games.isFetching}
            onRange={games.setRange}
          />
          <SelectionBar
            selection={selection}
            total={total}
            loadedIds={() => loadedIds(games.game, total)}
            allIds={
              smart
                ? edits && (() => edits.gameIds(smart))
                : api && (() => api.memberIds(collection.id))
            }
          />
        </SelectionScope>
      ) : (
        !games.isPending && (
          <div
            data-part="empty"
            className="flex flex-col items-center gap-2 rounded-panel border border-dashed border-border-strong p-12 text-center text-fg-subtle"
          >
            <SearchX className="size-8" />
            {smart
              ? t`No games match these rules yet. They'll show up here as your library changes.`
              : t`This collection is empty. Add games here, or from a game's menu.`}
          </div>
        )
      )}
      {commands.dialogs}
    </div>
  );
}

/** A random game from the collection, an installed one where it has any. */
async function pickRandom(
  api: CollectionsApi,
  collection: CollectionSummary,
): Promise<GameSummary | undefined> {
  const { total } = await api.games(collection.id, 0, 0);
  if (total === 0) return undefined;
  const offset = Math.floor(Math.random() * Math.max(0, total - RANDOM_SAMPLE + 1));
  const { items } = await api.games(collection.id, offset, RANDOM_SAMPLE);
  const installed = items.filter((game) => game.status !== "not-installed");
  const pool = installed.length > 0 ? installed : items;
  return pool[Math.floor(Math.random() * pool.length)];
}
