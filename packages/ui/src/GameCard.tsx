import { plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { CircleCheck, Clock, HardDrive, Store } from "lucide-react";
import type { CSSProperties } from "react";
import { GameArt } from "./GameArt";
import { type GameStatus, StatusBadge } from "./StatusBadge";

/** What a card shows; the app's game types fit it. */
export interface CardGame {
  title: string;
  genre: string;
  status: GameStatus;
  /** Hue pair for generated placeholder art. */
  art?: [number, number];
  playtimeHours?: number;
  lastPlayed?: string;
  sizeGb?: number;
  /** Stores that sell a game that is available to buy. */
  stores?: readonly string[];
}

export function GameCard({
  game,
  variant = "library",
  index = 0,
}: {
  game: CardGame;
  variant?: "library" | "recent" | "store";
  /** Position in its grid, used to stagger the enter animation. */
  index?: number;
}) {
  const { t } = useLingui();
  const { sizeGb, playtimeHours } = game;
  const stores = game.stores?.length ?? 0;
  const size =
    sizeGb !== undefined
      ? t({ message: `${sizeGb} GB`, comment: "A game's size on disk, in gigabytes" })
      : "—";
  return (
    <button
      type="button"
      data-cs="game-card"
      data-status={game.status}
      data-variant={variant}
      style={{ "--i": index } as CSSProperties}
      className="relative flex w-full animate-enter flex-col overflow-hidden rounded-card border border-border bg-surface-raised text-left hover:bg-surface-hover focus-visible:focus-ring-over"
    >
      <div className="relative">
        <GameArt part="art" game={game} className="aspect-(--cs-card-aspect) w-full" />
        <div className="absolute top-2 left-2">
          <StatusBadge part="badge" status={game.status} />
        </div>
      </div>
      <div className="flex flex-col gap-1 px-3 py-2.5">
        <span data-part="title" className="truncate text-sm font-semibold">
          {game.title}
        </span>
        {variant !== "recent" && (
          <span data-part="subtitle" className="truncate text-xs text-fg-subtle">
            {game.genre}
          </span>
        )}
        <div
          data-part="meta"
          className="flex items-center justify-between font-mono text-[11px] text-fg-muted"
        >
          {variant === "store" ? (
            <>
              <span data-part="size" className="inline-flex items-center gap-1">
                <HardDrive className="size-3.5" />
                {size}
              </span>
              {stores ? (
                <span data-part="stores" className="inline-flex items-center gap-1">
                  <Store className="size-3.5" />
                  {t({
                    message: plural(stores, { one: "# store", other: "# stores" }),
                    comment: "How many stores sell a game",
                  })}
                </span>
              ) : (
                <span
                  data-part="owned"
                  className="inline-flex items-center gap-1 text-status-installed"
                >
                  <CircleCheck className="size-3.5" />
                  <Trans comment="A store game the user already owns">Owned</Trans>
                </span>
              )}
            </>
          ) : (
            <>
              <span data-part="playtime" className="inline-flex items-center gap-1">
                <Clock className="size-3.5" />
                {playtimeHours
                  ? t({ message: `${playtimeHours}h`, comment: "Hours played, abbreviated" })
                  : "—"}
              </span>
              {variant === "library" ? (
                <span data-part="size" className="inline-flex items-center gap-1">
                  <HardDrive className="size-3.5" />
                  {size}
                </span>
              ) : (
                <span data-part="last-played">{game.lastPlayed}</span>
              )}
            </>
          )}
        </div>
      </div>
    </button>
  );
}
