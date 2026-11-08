import { useLingui } from "@lingui/react/macro";
import { Play } from "lucide-react";
import { useGameStarting } from "../../components/gameMenu/GameMenuHost";

/** "Starting…" on a game whose Play is on its way (a double click, its
 * menu's or its quick look's Play), so the click answers at once: the
 * checks before a launch can take a few seconds. Over a card's art, or
 * inline after a list row's title. */
export function StartingMark({
  gameId,
  inline = false,
  className = "bottom-0",
}: {
  gameId: string;
  inline?: boolean;
  /** Where it sits over the card: the art's bottom edge. */
  className?: string;
}) {
  const { t } = useLingui();
  const starting = useGameStarting(gameId);
  if (!starting) return null;
  const label = t({ message: "Starting…", comment: "On a game's card while the game starts" });
  if (inline)
    return (
      <span
        data-part="starting"
        role="status"
        className="inline-flex shrink-0 items-center gap-1 text-xs text-accent-alt"
      >
        <Play aria-hidden="true" className="size-3" />
        {label}
      </span>
    );
  return (
    <span
      data-part="starting"
      role="status"
      className={`absolute inset-x-0 flex items-center justify-center gap-1.5 overflow-hidden bg-surface-base/90 px-2 py-2 text-xs font-semibold text-fg ${className}`}
    >
      <Play aria-hidden="true" className="size-3.5 fill-current text-accent-alt" />
      {label}
      <span
        aria-hidden="true"
        className="absolute bottom-0 left-0 h-0.5 w-2/5 animate-progress bg-linear-to-r from-transparent via-accent-alt to-transparent [animation-play-state:var(--cs-motion-ambient)]"
      />
    </span>
  );
}
