import type { GameDetails } from "../../api/types";

/** What each of the game editor's tabs gets. A tab loads and saves on its
 * own, and reports whether it holds unsaved work so the editor can ask
 * before closing. */
export interface GameEditorTabProps {
  gameId: string;
  details: GameDetails;
  onDirtyChange: (dirty: boolean) => void;
}
