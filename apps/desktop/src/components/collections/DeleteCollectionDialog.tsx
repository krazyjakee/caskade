import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import { useDeleteCollection } from "../../api/collections";
import type { CollectionSummary } from "../../api/types";

/** Asks before deleting a collection; its games stay in the library. */
export function DeleteCollectionDialog({
  collection,
  close,
  onDeleted,
}: {
  collection: CollectionSummary;
  close: () => void;
  onDeleted?: () => void;
}) {
  const { t } = useLingui();
  const remove = useDeleteCollection();
  const name = collection.name;
  return (
    <ModalOverlay
      isOpen
      isDismissable={!remove.isPending}
      onOpenChange={(open) => {
        if (!open && !remove.isPending) close();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="w-full max-w-md rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg">
        <Dialog role="alertdialog" className="space-y-4 outline-none">
          <Heading slot="title" className="font-display text-xl font-semibold">
            <Trans>Delete {name}?</Trans>
          </Heading>
          <p className="text-sm text-fg-muted">
            <Trans>The collection goes. Its games stay in your library.</Trans>
          </p>
          {remove.error && (
            <p role="alert" className="text-sm text-danger">
              {remove.error.message}
            </p>
          )}
          <div className="flex justify-end gap-2">
            <Button
              data-spatial="focusable"
              data-focus-default
              isDisabled={remove.isPending}
              onPress={close}
            >
              <Trans>Cancel</Trans>
            </Button>
            <Button
              data-spatial="focusable"
              variant="danger"
              isDisabled={remove.isPending}
              onPress={() =>
                remove.mutate(collection.id, {
                  onSuccess: () => {
                    close();
                    onDeleted?.();
                  },
                })
              }
            >
              {remove.isPending ? t`Deleting…` : t`Delete`}
            </Button>
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
