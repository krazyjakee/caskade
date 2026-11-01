import { GameArt } from "@caskade/ui";
import { useLingui } from "@lingui/react/macro";
import { Link } from "@tanstack/react-router";
import { Check, Heart } from "lucide-react";
import { memo } from "react";
import { platformLine } from "../../api/presentation";
import { useStorageAvailability } from "../../api/StorageProvider";
import type { GameSummary } from "../../api/types";
import { PluginCardExtras } from "../../plugins/GameSlots";
import { AgeLockLabel } from "../game/AgeGate";
import { useGameActions } from "./GameActions";

/** A game in the library grid: its cover, its title and, unless it is a PC
 * game, its platform (or its first and how many more). The list layout is
 * `LibraryRow`. Its menu opens at a right click, or under the card from the
 * context menu key or a pad's Start. */
export const LibraryCard = memo(function LibraryCard({
  game,
  index,
}: {
  game: GameSummary;
  /** Its place in the view, for range selection. */
  index?: number;
}) {
  const { t } = useLingui();
  const unavailableCopy = useStorageAvailability(game.id);
  const actions = useGameActions(game, index);
  const platform = platformLine(game.platform);
  return (
    <div
      ref={actions.owner}
      // Focus comes back to this game's card after its details (#150).
      data-focus-key={`game-card:${game.id}`}
      data-cs="game-card"
      data-status={game.status}
      data-variant="library"
      data-storage-status={unavailableCopy ? "unavailable" : undefined}
      data-part="grid-item"
      data-selected={actions.chosen.selected || undefined}
      className="relative h-full min-w-0 rounded-card [contain:layout_style]"
    >
      <Link
        {...actions.targetProps}
        to="/games/$gameId"
        params={{ gameId: game.id }}
        // Desktop: its summary panel; console: its page (`useSelectGame`).
        // In selection mode: chooses it.
        onClick={actions.onClick}
        aria-pressed={actions.chosen.mode ? actions.chosen.selected : undefined}
        className="group relative flex h-full flex-col overflow-hidden rounded-card border border-border bg-surface-raised outline-none transition-colors duration-(--cs-motion-duration-fast) hover:bg-surface-hover focus-visible:focus-ring-over"
      >
        <GameArt part="art" game={game} image={game.coverGrid} className="min-h-0 flex-1 w-full" />
        {game.adminPinNeeded && (
          <span className="absolute top-2 left-2 rounded-control bg-surface-base/90 px-2 py-1">
            <AgeLockLabel />
          </span>
        )}
        <div className="h-14 min-w-0 shrink-0 px-3 py-2.5">
          <p data-part="title" className="truncate text-sm font-semibold">
            {game.title}
          </p>
          {platform && (
            <p data-part="subtitle" className="truncate text-xs text-fg-subtle">
              {platform}
            </p>
          )}
        </div>
        {unavailableCopy && (
          <span
            data-part="storage-availability"
            className="absolute top-2 left-2 rounded-control border border-warning/40 bg-surface-base/90 px-2 py-1 text-[10px] text-warning"
          >
            {t`Managed copy unavailable`}
          </span>
        )}
        {actions.chosen.mode && <SelectMark selected={actions.chosen.selected} />}
        {game.favorite && (
          <Heart
            data-part="favorite"
            aria-label={t({ message: "Favorite", comment: "A heart on a favourite game's card" })}
            className="absolute top-3 right-3 size-3.5 fill-accent-alt text-accent-alt"
          />
        )}
        <div data-part="plugins" className="contents">
          <PluginCardExtras gameId={game.id} />
        </div>
      </Link>
    </div>
  );
});

/** A chosen game's check, in selection mode (#173). */
function SelectMark({ selected }: { selected: boolean }) {
  const { t } = useLingui();
  return (
    <span
      data-part="select"
      aria-hidden="true"
      title={selected ? t`Selected` : undefined}
      className={`absolute top-2 left-2 grid size-6 place-items-center rounded-full border-2 ${selected ? "border-accent-alt bg-accent text-accent-fg" : "border-fg-subtle bg-surface-base/80"}`}
    >
      {selected && <Check className="size-4" strokeWidth={3} />}
    </span>
  );
}
