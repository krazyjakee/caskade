import { Button } from "@caskade/ui";
import { Trans } from "@lingui/react/macro";
import type { ReactNode } from "react";
import { Dialog, Heading } from "react-aria-components";
import { FailureNotice } from "./FailureNotice";
import { ModalFrame } from "./ModalFrame";

/** Asks before something that can't be undone (#49): a removal, a reset.
 * The body says what goes with it. Cancel is the first stop, for a pad and
 * for Enter alike; the dialog stays open while the action runs and shows
 * its failure, and the caller closes it when the action is done. */
export function ConfirmDialog({
  title,
  children,
  confirmLabel,
  pendingLabel,
  isPending = false,
  confirmDisabled = false,
  error,
  onConfirm,
  close,
}: {
  title: string;
  /** What goes, and what stays. */
  children: ReactNode;
  confirmLabel: string;
  /** Shown on the button while the action runs, e.g. "Removing…". */
  pendingLabel?: string;
  isPending?: boolean;
  /** The action waits, e.g. for what it would change. */
  confirmDisabled?: boolean;
  error?: unknown;
  onConfirm: () => void;
  close: () => void;
}) {
  return (
    <ModalFrame
      busy={isPending}
      close={close}
      className="w-full max-w-md rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg"
    >
      <Dialog role="alertdialog" className="space-y-4 outline-none">
        <Heading slot="title" className="font-display text-xl font-semibold">
          {title}
        </Heading>
        <div className="space-y-2 text-sm text-fg-muted">{children}</div>
        {error ? <FailureNotice variant="inline" error={error} /> : null}
        <div className="flex justify-end gap-2">
          <Button
            data-spatial="focusable"
            data-focus-default
            autoFocus
            isDisabled={isPending}
            onPress={close}
          >
            <Trans>Cancel</Trans>
          </Button>
          <Button
            data-spatial="focusable"
            variant="danger"
            isDisabled={isPending || confirmDisabled}
            onPress={onConfirm}
          >
            {isPending && pendingLabel ? pendingLabel : confirmLabel}
          </Button>
        </div>
      </Dialog>
    </ModalFrame>
  );
}
