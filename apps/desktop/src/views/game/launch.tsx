import { useRef, useState } from "react";
import { useGame, useLaunchGame } from "../../api/hooks";
import { useLibraryApi } from "../../api/provider";
import type { AgeRestriction, HostBusy, LaunchChoice, LaunchPrompt } from "../../api/types";
import { LaunchChoiceDialog } from "../../components/LaunchChoiceDialog";
import { StopHostDialog } from "../../components/streaming/StopHostDialog";
import { AgeGateDialog } from "./AgeGate";
import { usePlayWhilePlayingElsewhere } from "./PlayingElsewhere";
import { usePlayWithSaves } from "./SavesBeforePlay";
import { usePlayWithUpdates } from "./UpdateBeforePlay";

/** Starting one of a game's entries, as the game page's Play and a game's
 * menu do it: an admin's PIN first when the account's age limit asks for
 * one (#209), then the update offer, then a warning when it's running on
 * another device (#240), then the saves (a newer one from another device,
 * or which to use when both played, #239), then the launch, then the action,
 * profile or disc question when the launch asks one. A play action chosen
 * up front (#178), or in the question, goes with every later answer. A
 * release streamed from another device (#264) skips the update, the
 * warning and the saves, which are that device's; it may ask to stop the
 * game that device plays. Render `dialogs`. */
export function useGameLaunch(gameId: string) {
  const api = useLibraryApi();
  const launch = useLaunchGame();
  const [prompt, setPrompt] = useState<LaunchPrompt | null>(null);
  // The PIN pad, and what to do once an admin's PIN approved the launch.
  const [gate, setGate] = useState<{ restriction: AgeRestriction; next: () => void } | null>(null);
  const [gating, setGating] = useState(false);
  // The streaming device plays another game, and what to do once the
  // user says to stop it.
  const [hostBusy, setHostBusy] = useState<{ busy: HostBusy; next: () => void } | null>(null);
  const game = useGame(gameId);
  const streamed = (entryId: string) =>
    game.data?.entries.some((entry) => entry.id === entryId && entry.source.startsWith("stream:"));
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
          setHostBusy(
            result.type === "host_busy"
              ? { busy: result.busy, next: () => start(entryId, { ...choice, stopHost: true }) }
              : null,
          );
        },
      },
    );
  };
  const saves = usePlayWithSaves((entryId) => start(entryId));
  const elsewhere = usePlayWhilePlayingElsewhere(gameId, (entryId) => saves.play(entryId));
  const updates = usePlayWithUpdates(gameId, (entryId) => elsewhere.play(entryId));
  const proceed = (entryId: string) => (streamed(entryId) ? start(entryId) : updates.play(entryId));
  const play = (entryId: string, playAction?: string) => {
    action.current = playAction ?? null;
    if (!api.ageRestriction) {
      proceed(entryId);
      return;
    }
    setGating(true);
    api
      .ageRestriction(gameId)
      .then(
        (restriction) =>
          restriction ? setGate({ restriction, next: () => proceed(entryId) }) : proceed(entryId),
        // The launch is checked anyway.
        () => proceed(entryId),
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
      {hostBusy && (
        <StopHostDialog
          busy={hostBusy.busy}
          title={game.data?.title}
          launching={launch.isPending}
          error={launch.isError ? launch.error.message : undefined}
          close={() => {
            setHostBusy(null);
            launch.reset();
          }}
          confirm={hostBusy.next}
        />
      )}
      {updates.dialog}
      {elsewhere.dialog}
      {saves.dialog}
    </>
  );
  return {
    /** Plays `entryId` (with one of its play actions, else its default):
     * the PIN if needed, then a newer release's offer. */
    play,
    checking: updates.checking || saves.checking || gating,
    launch,
    /** A PIN, profile or disc question is open; its dialog shows the
     * launch's error. */
    asking: prompt !== null || gate !== null || hostBusy !== null,
    /** Any question before or at the launch is open: the PIN, a newer
     * release's offer, playing elsewhere, the saves, a profile or disc.
     * Its answer goes on with the launch, or ends it. */
    questioning:
      prompt !== null ||
      gate !== null ||
      hostBusy !== null ||
      updates.asking ||
      elsewhere.asking ||
      saves.asking,
    dialogs,
  };
}
