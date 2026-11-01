/** The editor's Scripts tab (#180): the game's own lifecycle scripts, with
 * the order a launch runs them in among the global ones. */
import { ScriptsPanel } from "../scripts/ScriptsPanel";
import type { GameEditorTabProps } from "./types";

export function ScriptsTab({ gameId, onDirtyChange }: GameEditorTabProps) {
  return (
    <ScriptsPanel scope={{ type: "game", gameId }} gameId={gameId} onDirtyChange={onDirtyChange} />
  );
}
