import { ExternalLink } from "lucide-react";
import { type Game, stores, storeUrl } from "../mocks/games";

/** Links out to every store that sells the game. */
export function StoreLinks({ game }: { game: Game }) {
  return (
    <ul data-cs="store-links" className="flex flex-wrap items-center gap-2">
      {game.stores?.map((store) => (
        <li key={store}>
          <a
            href={storeUrl(game, store)}
            target="_blank"
            rel="noreferrer"
            data-part="store-link"
            data-store={store}
            className="flex items-center gap-2.5 rounded-control border border-border-strong bg-surface-overlay/70 px-4 py-2.5 text-sm backdrop-blur transition-colors duration-(--cs-motion-duration-fast) hover:border-accent-alt/50 hover:bg-surface-hover"
          >
            <span className="font-display font-semibold tracking-wide">{stores[store].label}</span>
            <ExternalLink className="size-3.5 text-fg-subtle" />
          </a>
        </li>
      ))}
    </ul>
  );
}
