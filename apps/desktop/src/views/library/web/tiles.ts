import type { GameSummary } from "../../../api/types";

/** A tile's size in the Web tab's mosaic: 1×1, 2×2 or 3×3 cells. */
export type TileSize = "small" | "large" | "huge";

/** Played this recently, a game's tile is large. */
const RECENT_DAYS = 7;

/** Each game's tile (WEB-GAMES-PLAN §5). A tile is large when the game is a
 * favourite, was played in the last week, or has play time in the top
 * quarter of `games`; the first large one is huge. */
export function tileSizes(games: GameSummary[], now = Date.now()): Map<string, TileSize> {
  const played = games
    .map((game) => game.playtimeSeconds)
    .filter((seconds) => seconds > 0)
    .sort((a, b) => b - a);
  const top = Math.ceil(games.length / 4);
  // The play time a game needs to be in the top quarter.
  const threshold = played[Math.min(top, played.length) - 1] ?? Number.POSITIVE_INFINITY;
  const since = now - RECENT_DAYS * 86_400_000;
  const sizes = new Map<string, TileSize>();
  let huge = false;
  for (const game of games) {
    const large =
      game.favorite ||
      (game.lastPlayedAt !== null && Date.parse(game.lastPlayedAt) >= since) ||
      (game.playtimeSeconds > 0 && game.playtimeSeconds >= threshold);
    if (large && !huge) {
      huge = true;
      sizes.set(game.id, "huge");
    } else sizes.set(game.id, large ? "large" : "small");
  }
  return sizes;
}
