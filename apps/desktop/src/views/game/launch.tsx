import { useRef, useState } from "react";
import { useLaunchGame } from "../../api/hooks";
import { useLibraryApi } from "../../api/provider";
import type { AgeRestriction, LaunchChoice, LaunchPrompt } from "../../api/types";
import { LaunchChoiceDialog } from "../../components/LaunchChoiceDialog";
import { AgeGateDialog } from "./AgeGate";
import { usePlayWithUpdates } from "./UpdateBeforePlay";

/** Starting one of a game's entries, as the game page's Play and a game's
 * menu do it: an admin's PIN first when the account's age limit asks for
 * one (#209), then the update offer, then the launch, then the action,
 * profile or disc question when the launch asks one. A play action chosen
 * up front (#178), or in the question, goes with every later answer.
 * Render `dialogs`. */
export function useGameLaunch(gameId: string) {
  const api = useLibraryApi();
  const launch = useLaunchGame();
  const [prompt, setPrompt] = useState<LaunchPrompt | null>(null);
  // The PIN pad, and what to do once an admin's PIN approved the launch.
  const [gate, setGate] = useState<{ restriction: AgeRestriction; next: () => void } | null>(null);
  const [gating, setGating] = useState(false);
  // The play action this launch starts, once chosen.
  const action = useRef<string | null>(null);
  const start = (entryId: string, choice?: LaunchChoice): void => {
    if (choice?.action) action.current = choice.action;
    const answers = action.current ? { ...choice, action: action.current } : choice;
    launch.mutate(
      { entryId, choice: answers },
      {
        onSuccess: (result) => {
          setPrompt(result.type === "choose" ? result.prompt : null);
          // The core refused it (an approval lapsed, or Play skipped the
          // check): ask, then launch again.
          if (result.type === "age_restricted")
            setGate({ restriction: result.restriction, next: () => start(entryId, choice) });
        },
      },
    );
  };
  const updates = usePlayWithUpdates(gameId, (entryId) => start(entryId));
  const play = (entryId: string, playAction?: string) => {
    action.current = playAction ?? null;
    if (!api.ageRestriction) {
      updates.play(entryId);
      return;
    }
    setGating(true);
    api
      .ageRestriction(gameId)
      .then(
        (restriction) =>
          restriction
            ? setGate({ restriction, next: () => updates.play(entryId) })
            : updates.play(entryId),
        // The launch is checked anyway.
        () => updates.play(entryId),
      )
      .finally(() => setGating(false));
  };
  const dialogs = (
    <>
      {gate && (
        <AgeGateDialog
          restriction={gate.restriction}
          close={() => setGate(null)}
          approved={() => {
            setGate(null);
            gate.next();
          }}
        />
      )}
      {prompt && (
        <LaunchChoiceDialog
          key={`${prompt.entryId}:${prompt.actions?.length ?? 0}:${prompt.profiles.length}:${prompt.discs.length}`}
          prompt={prompt}
          busy={launch.isPending}
          error={launch.isError ? launch.error.message : undefined}
          close={() => {
            setPrompt(null);
            launch.reset();
          }}
          choose={(choice) => start(prompt.entryId, choice)}
        />
      )}
      {updates.dialog}
    </>
  );
  return {
    /** Plays `entryId` (with one of its play actions, else its default):
     * the PIN if needed, then a newer release's offer. */
    play,
    checking: updates.checking || gating,
    launch,
    /** A PIN, profile or disc question is open; its dialog shows the
     * launch's error. */
    asking: prompt !== null || gate !== null,
    dialogs,
  };
}
