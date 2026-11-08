import { GameArt } from "@caskade/ui";
import { useLingui } from "@lingui/react/macro";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { Heart } from "lucide-react";
import { type CSSProperties, memo, useMemo } from "react";
import { normalizeLibraryQuery } from "../../../api/hooks";
import { libraryKeys, useLibraryApi } from "../../../api/provider";
import type { GameSummary, LibraryQuery } from "../../../api/types";
import { AgeLockLabel } from "../../game/AgeGate";
import { useGameActions } from "../GameActions";
import { type TileSize, tileSizes } from "./tiles";

/** The most web games the mosaic shows: the core's page limit. */
const MOSAIC_LIMIT = 1000;

const spans: Record<TileSize, string> = {
  small: "",
  large: "col-span-2 row-span-2",
  huge: "col-span-3 row-span-3",
};

/** Every game on the Web tab, for the mosaic. */
export function useWebGames(input: LibraryQuery, { enabled = true } = {}) {
  const api = useLibraryApi();
  const query = normalizeLibraryQuery(input);
  return useQuery({
    queryKey: [...libraryKeys.games, query, "mosaic"],
    queryFn: () => api.listGames({ ...query, offset: 0, limit: MOSAIC_LIMIT }),
    enabled,
  });
}

/** The Web tab's games as a mosaic of square tiles,
 * packed densely; favourites and games played lately or often are large. */
export function WebMosaic({ games }: { games: GameSummary[] }) {
  const { t } = useLingui();
  const sizes = useMemo(() => tileSizes(games), [games]);
  return (
    <ul
      aria-label={t({ message: "Web games", comment: "The Web tab's mosaic of games" })}
      data-part="mosaic"
      className="grid grid-flow-dense auto-rows-(--tile) grid-cols-[repeat(auto-fill,var(--tile))] justify-center gap-3 pb-8 [--tile:calc(var(--cs-card-width)*0.6)]"
    >
      {games.map((game, index) => (
        <WebTile key={game.id} game={game} size={sizes.get(game.id) ?? "small"} index={index} />
      ))}
    </ul>
  );
}

const WebTile = memo(function WebTile({
  game,
  size,
  index,
}: {
  game: GameSummary;
  size: TileSize;
  index: number;
}) {
  const { t } = useLingui();
  const actions = useGameActions<HTMLLIElement>(game);
  return (
    <li
      ref={actions.owner}
      data-focus-key={`game-card:${game.id}`}
      data-cs="game-card"
      data-variant="web"
      data-tile={size}
      data-status={game.status}
      className={`relative min-w-0 animate-enter rounded-[calc(var(--cs-radius-card)*2)] ${spans[size]}`}
      style={{ "--i": Math.min(index, 20) } as CSSProperties}
    >
      <Link
        {...actions.targetProps}
        to="/games/$gameId"
        params={{ gameId: game.id }}
        onClick={actions.onClick}
        onDoubleClick={actions.onDoubleClick}
        onMouseDown={actions.onMouseDown}
        aria-label={game.title}
        className="group relative block size-full overflow-hidden rounded-[calc(var(--cs-radius-card)*2)] border border-border bg-surface-raised outline-none focus-visible:focus-ring-over"
      >
        <GameArt part="art" game={game} image={game.coverGrid} className="size-full" />
        {game.adminPinNeeded && (
          <span className="absolute top-2 left-2 rounded-control bg-surface-base/90 px-2 py-1">
            <AgeLockLabel />
          </span>
        )}
        <p
          data-part="title"
          className="absolute inset-x-0 bottom-0 truncate bg-linear-to-t from-surface-base/90 to-transparent px-2.5 pt-6 pb-2 text-sm font-semibold opacity-0 transition-opacity duration-(--cs-motion-duration-fast) group-hover:opacity-100 group-focus-visible:opacity-100"
        >
          {game.title}
        </p>
        {game.favorite && (
          <Heart
            data-part="favorite"
            aria-label={t({ message: "Favorite", comment: "A heart on a favourite game's card" })}
            className="absolute top-2.5 right-2.5 size-3.5 fill-accent-alt text-accent-alt"
          />
        )}
      </Link>
    </li>
  );
});
