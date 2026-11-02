import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import type { HostBusy } from "../../api/types";

/** The device to stream from is playing another game (#264): stop it and
 * stream this one, or leave it be. A pad starts on Stop and stream. */
export function StopHostDialog({
  busy,
  title,
  launching,
  error,
  close,
  confirm,
}: {
  busy: HostBusy;
  /** The game to stream, when known. */
  title?: string;
  launching: boolean;
  error?: string;
  close: () => void;
  confirm: () => void;
}) {
  const { t } = useLingui();
  const device = busy.device;
  const playing = busy.playing;
  const question = title
    ? playing
      ? t({
          message: `${device} is playing ${playing}. Stop it and stream ${title}?`,
          comment: "{device} is another device's name; {playing} and {title} are games",
        })
      : t({
          message: `${device} is playing another game. Stop it and stream ${title}?`,
          comment: "{device} is another device's name; {title} is a game",
        })
    : playing
      ? t({
          message: `${device} is playing ${playing}. Stop it and stream this game?`,
          comment: "{device} is another device's name; {playing} is a game",
        })
      : t({
          message: `${device} is playing another game. Stop it and stream this game?`,
          comment: "{device} is another device's name",
        });
  return (
    <ModalOverlay
      isOpen
      isDismissable={!launching}
      onOpenChange={(open) => {
        if (!open && !launching) close();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="w-full max-w-md rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg">
        <Dialog data-cs="stop-host" aria-label={t`Stop the other game?`} className="outline-none">
          <Heading slot="title" className="font-display text-xl font-semibold">
            {t`Stop the other game?`}
          </Heading>
          <p data-part="question" className="mt-2 text-sm text-fg-muted">
            {question}
          </p>
          <p className="mt-2 text-sm text-fg-muted">
            <Trans>Progress it hasn't saved is lost.</Trans>
          </p>
          {error && (
            <p role="alert" className="mt-3 text-sm text-danger">
              {error}
            </p>
          )}
          <div data-part="actions" className="mt-6 flex justify-end gap-2">
            <Button isDisabled={launching} onPress={close}>
              <Trans>Cancel</Trans>
            </Button>
            <Button
              variant="primary"
              autoFocus
              data-focus-default
              isDisabled={launching}
              onPress={confirm}
            >
              {launching ? t`Starting…` : t`Stop and stream`}
            </Button>
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
