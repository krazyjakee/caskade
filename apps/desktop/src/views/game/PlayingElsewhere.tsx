/** Starting a game that's running on another device (#240): Play warns that
 * each device will keep its own save. The check reads what the core already
 * knows, so it never waits on a device that can't be reached. */
import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useState } from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import { formatDateTime } from "../../api/presentation";
import { useOptionalLibraryApi } from "../../api/provider";
import { type PlayingElsewhere, usePlayingElsewhere } from "../../api/saves";

export function usePlayWhilePlayingElsewhere(gameId: string, next: (entryId: string) => void) {
  const api = useOptionalLibraryApi()?.saves;
  const [asking, setAsking] = useState<{ entryId: string; playing: PlayingElsewhere[] } | null>(
    null,
  );
  const play = (entryId: string) => {
    if (!api) {
      next(entryId);
      return;
    }
    api.playingElsewhere().then(
      (all) => {
        const playing = all.filter((each) => each.gameId === gameId);
        if (playing.length > 0) setAsking({ entryId, playing });
        else next(entryId);
      },
      // Presence never stops a launch.
      () => next(entryId),
    );
  };
  const dialog = asking && (
    <PlayingElsewhereDialog
      playing={asking.playing}
      close={() => setAsking(null)}
      playAnyway={() => {
        setAsking(null);
        next(asking.entryId);
      }}
    />
  );
  return { play, asking: asking !== null, dialog };
}

function PlayingElsewhereDialog({
  playing,
  close,
  playAnyway,
}: {
  playing: PlayingElsewhere[];
  close: () => void;
  playAnyway: () => void;
}) {
  const { t } = useLingui();
  const first = playing[0];
  const device = first?.deviceName || t({ message: "another device", context: "playing on" });
  const title = first?.title ?? "";
  return (
    <ModalOverlay
      isOpen
      isDismissable
      onOpenChange={(open) => {
        if (!open) close();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="w-full max-w-lg rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg">
        <Dialog data-cs="playing-elsewhere" className="outline-none">
          <Heading slot="title" className="font-display text-xl font-semibold">
            {t`${title} is running on ${device}`}
          </Heading>
          <p className="mt-2 text-sm text-fg-muted">
            <Trans>
              If you play here too, each device keeps its own save, and the next time you play you
              choose which one to carry on with.
            </Trans>
          </p>
          <ul className="mt-4 space-y-1 text-sm">
            {playing.map((each) => {
              const name =
                each.deviceName || t({ message: "another device", context: "playing on" });
              const since = formatDateTime(each.startedAtMs);
              return (
                <li key={`${each.deviceId}:${each.startedAtMs}`}>{t`On ${name} since ${since}`}</li>
              );
            })}
          </ul>
          <div className="mt-6 flex justify-end gap-2">
            <Button data-spatial="focusable" data-focus-default="" onPress={close}>
              <Trans>Cancel</Trans>
            </Button>
            <Button data-spatial="focusable" variant="primary" onPress={playAnyway}>
              <Trans>Play anyway</Trans>
            </Button>
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}

/** Where the game is running now, on the game page. */
export function PlayingElsewhereLine({ gameId }: { gameId: string }) {
  const { t } = useLingui();
  const playing = usePlayingElsewhere(gameId);
  if (playing.length === 0) return null;
  const devices = playing
    .map((each) => each.deviceName || t({ message: "another device", context: "playing on" }))
    .join(", ");
  return (
    <p data-part="playing-elsewhere" role="status" className="mt-2 text-sm text-accent-alt">
      {t`Playing on ${devices}`}
    </p>
  );
}

/** A game's card in console mode: the device it's running on. */
export function PlayingElsewhereMark({ gameId }: { gameId: string }) {
  const { t } = useLingui();
  const playing = usePlayingElsewhere(gameId)[0];
  if (!playing) return null;
  const device = playing.deviceName || t({ message: "another device", context: "playing on" });
  return (
    <span
      data-part="playing-elsewhere"
      className="absolute right-2 bottom-16 left-2 truncate rounded-control bg-surface-base/90 px-2 py-1 text-[11px] text-accent-alt"
    >
      {t`Playing on ${device}`}
    </span>
  );
}

/** What a paired device is playing, in Settings → Devices. */
export function DevicePlaying({ deviceId }: { deviceId: string }) {
  const { t } = useLingui();
  const titles = usePlayingElsewhere()
    .filter((each) => each.deviceId === deviceId)
    .map((each) => each.title);
  if (titles.length === 0) return null;
  const games = titles.join(", ");
  return (
    <p data-part="device-playing" className="text-xs text-accent-alt">
      {t`Playing ${games}`}
    </p>
  );
}
