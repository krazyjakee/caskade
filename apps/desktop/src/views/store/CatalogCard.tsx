import { GameArt } from "@caskade/ui";
import { useLingui } from "@lingui/react/macro";
import { CircleCheck, Library } from "lucide-react";
import type { CSSProperties, MouseEvent } from "react";
import type { IgdbGame, IgdbOwned, IgdbReleaseDate } from "../../api/igdb";
import { platformsLine, releaseKind, releaseWhen, releaseWhere } from "./releases";

/** A catalog game in the Store (#163): its cover, title and platforms, the
 * release that listed it (recent and upcoming) and, apart from those, its
 * place in the library when it has one. A plain link, so it works without
 * the router (Storybook); `onOpen` opens its page. Its `data-status` is the
 * library's: absent for a game the library doesn't have, because a store
 * link says nothing of what it costs or whether it is on sale. */
export function CatalogCard({
  game,
  href,
  onOpen,
  release,
  owned,
  layout = "grid",
  index = 0,
}: {
  game: IgdbGame;
  href: string;
  onOpen?: (igdbId: string) => void;
  /** The release that listed the game, on the release shelves. */
  release?: IgdbReleaseDate | null;
  /** Its library game, when the library has it. */
  owned?: IgdbOwned;
  /** A grid sizes the art by its aspect; a shelf row fills its fixed height. */
  layout?: "grid" | "shelf";
  index?: number;
}) {
  const { t } = useLingui();
  const onClick = (event: MouseEvent<HTMLAnchorElement>) => {
    if (!onOpen) return;
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey)
      return;
    event.preventDefault();
    onOpen(game.id);
  };
  const status = owned ? (owned.installed ? "installed" : "not-installed") : undefined;
  const kind = release ? releaseKind(release) : null;
  const where = release ? releaseWhere(release) : platformsLine(game);
  const year = game.releaseDate?.slice(0, 4);
  return (
    <div
      data-cs="game-card"
      data-variant="store"
      data-status={status}
      data-focus-key={`catalog-card:${game.id}`}
      style={{ "--i": index } as CSSProperties}
      className="relative h-full min-w-0 animate-enter rounded-card"
    >
      <a
        href={href}
        onClick={onClick}
        className="group relative flex h-full flex-col overflow-hidden rounded-card border border-border bg-surface-raised outline-none transition-colors duration-(--cs-motion-duration-fast) hover:bg-surface-hover focus-visible:focus-ring-over"
      >
        <GameArt
          part="art"
          game={game}
          image={game.images.cover?.url}
          className={
            layout === "grid" ? "aspect-(--cs-card-aspect) w-full" : "min-h-0 w-full flex-1"
          }
        />
        <div className="min-w-0 shrink-0 px-3 py-2.5">
          <p data-part="title" className="truncate text-sm font-semibold">
            {game.title}
          </p>
          {where && (
            <p data-part="subtitle" className="truncate text-xs text-fg-subtle">
              {where}
            </p>
          )}
          <p data-part="meta" className="truncate font-mono text-[11px] text-fg-muted">
            {release
              ? releaseWhen(release)
              : (year ??
                t({
                  message: "TBD",
                  comment: "A release date that isn't known yet (to be determined)",
                }))}
            {kind === "cancelled" &&
              ` · ${t({ message: "Cancelled", comment: "A game release's status" })}`}
            {kind === "early-access" &&
              ` · ${t({ message: "Early access", comment: "A game release's status" })}`}
          </p>
        </div>
        {owned && (
          <span
            data-part="owned"
            className="absolute top-2 left-2 inline-flex items-center gap-1 rounded-full bg-surface-base/85 px-2 py-0.5 text-[11px] font-medium text-status-installed backdrop-blur"
          >
            {owned.installed ? (
              <CircleCheck className="size-3" aria-hidden />
            ) : (
              <Library className="size-3" aria-hidden />
            )}
            {owned.installed
              ? t({ message: "Installed", context: "game status" })
              : t({
                  message: "In library",
                  comment: "A store game the library has but isn't installed",
                })}
          </span>
        )}
      </a>
    </div>
  );
}
