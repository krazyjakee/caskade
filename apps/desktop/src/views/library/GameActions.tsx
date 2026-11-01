import { useLingui } from "@lingui/react/macro";
import { type MouseEvent, useRef } from "react";
import type { GameSummary } from "../../api/types";
import { useGameMenu } from "../../components/gameMenu/GameMenuHost";
import { useGameSelected, useSelection } from "../../components/gameSelection/selection";
import { useInputAction } from "../../input";

/** A library game's actions, shared by the grid's cards and the list's rows:
 * a pad's X opens its quick look (X again plays it), and its menu (`GameMenu`) opens from a right
 * click on its link (`targetProps`), the context menu key, Shift+F10, a
 * pad's Start (#150) or the row's actions button (`open`). */
export function useGameActions<Owner extends HTMLElement = HTMLDivElement>(
  game: GameSummary,
  /** Its place in the view, for Shift's range selection. */
  index?: number,
) {
  const { t } = useLingui();
  const owner = useRef<Owner>(null);
  const menu = useGameMenu({ gameId: game.id, title: game.title });
  const selection = useSelection();
  const chosen = useGameSelected(game.id);
  useInputAction("details", () => menu.look(), {
    label: t({ message: "Details", comment: "Button hint: a pad's X opens the game's quick look" }),
    owner,
  });
  useInputAction(
    "menu",
    () => (chosen.mode ? selection?.edit() : owner.current && menu.open(owner.current)),
    {
      label: chosen.mode
        ? t({ message: "Edit selected", comment: "Button hint: a pad's Start in selection mode" })
        : t({
            message: "Options",
            comment: "Button hint: a pad's Start opens the focused item's menu",
          }),
      owner,
    },
  );
  // In selection mode a click (or Enter, or a pad's A) chooses the game,
  // Shift a range, instead of opening it.
  const onClick =
    chosen.mode && selection
      ? (event: MouseEvent<HTMLElement>) => {
          event.preventDefault();
          if (event.shiftKey && index !== undefined) selection.extend(index);
          else selection.toggle(game.id, index);
        }
      : menu.onClick;
  return { owner, ...menu, onClick, chosen };
}
