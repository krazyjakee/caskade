import { msg } from "@lingui/core/macro";
import { useLingui } from "@lingui/react/macro";
import type { MouseEvent, ReactNode } from "react";
import { useCollection } from "../../api/collections";
import { useGame } from "../../api/hooks";
import { useCatalogGames } from "../../api/igdb";
import { backTarget, LIBRARY_GAMES, type ReturnPlace, returnPlace, useGoBack } from "./origin";

const back = msg({
  message: "Back",
  comment: "The game page's link back to where it was opened from",
});

/** "Back to …", named after the page, collection or game. */
function BackTo({ name }: { name: string | undefined }) {
  const { t } = useLingui();
  return name
    ? t({
        message: `Back to ${name}`,
        comment: "{name} is the page, collection or game the game page was opened from",
      })
    : t(back);
}

function CollectionName({ id }: { id: string }) {
  return <BackTo name={useCollection(id).data?.name} />;
}

function GameName({ id }: { id: string }) {
  return <BackTo name={useGame(id).data?.title} />;
}

function StoreGameName({ id }: { id: string }) {
  return <BackTo name={useCatalogGames([id]).data?.games[0]?.title} />;
}

function Label({ place }: { place: ReturnPlace }): ReactNode {
  switch (place.kind) {
    case "page":
      return <BackTo name={place.name} />;
    case "collection":
      return <CollectionName id={place.id} />;
    case "game":
      return <GameName id={place.id} />;
    case "store-game":
      return <StoreGameName id={place.id} />;
    default:
      return <BackTo name={undefined} />;
  }
}

/** The game page's way back (#195): to the page the game was opened from,
 * named after it, or to the Library. A pad's B steps back the same way,
 * through history. */
export function GameBackLink({
  gameId,
  from,
  className,
  base = LIBRARY_GAMES,
  fallback = "/library",
}: {
  gameId: string;
  /** The game route's `from`. */
  from?: string;
  className?: string;
  /** Where the page lives: library games, or the Store's (`STORE_GAMES`). */
  base?: string;
  /** Where Back goes without a `from`. */
  fallback?: string;
}) {
  const target = backTarget(gameId, from, base);
  const goBack = useGoBack();
  const onClick = (event: MouseEvent<HTMLAnchorElement>) => {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey)
      return;
    event.preventDefault();
    goBack(target, fallback);
  };
  return (
    <a href={target ?? fallback} onClick={onClick} className={className}>
      <span aria-hidden="true">← </span>
      <Label place={returnPlace(target ?? fallback)} />
    </a>
  );
}
