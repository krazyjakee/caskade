import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import { useStorageApi } from "../../api/StorageProvider";

/** Asks before a game's menu deletes an install's files. */
export function UninstallDialog({
  title,
  entryId,
  location,
  close,
}: {
  title: string;
  entryId: string;
  location: string | null;
  close: () => void;
}) {
  const { t } = useLingui();
  const api = useStorageApi();
  const client = useQueryClient();
  const uninstall = useMutation({
    mutationFn: async () => {
      if (!api) throw new Error(t`Storage service is unavailable`);
      await api.uninstall(entryId);
    },
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["storage"] });
      void client.invalidateQueries({ queryKey: ["library"] });
      close();
    },
  });
  return (
    <ModalOverlay
      isOpen
      isDismissable={!uninstall.isPending}
      onOpenChange={(open) => {
        if (!open && !uninstall.isPending) close();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="w-full max-w-md rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg">
        <Dialog role="alertdialog" className="space-y-4 outline-none">
          <Heading slot="title" className="font-display text-xl font-semibold">
            <Trans>Uninstall {title}?</Trans>
          </Heading>
          <p className="text-sm text-fg-muted">
            {location ? (
              <Trans>
                Its installed files are deleted from {location}, with the play sessions recorded for
                this install. The game stays in your library. The uninstall shows in Activity.
              </Trans>
            ) : (
              <Trans>
                Its installed files are deleted, with the play sessions recorded for this install.
                The game stays in your library. The uninstall shows in Activity.
              </Trans>
            )}
          </p>
          {uninstall.error && (
            <p role="alert" className="text-sm text-danger">
              {uninstall.error.message}
            </p>
          )}
          <div className="flex justify-end gap-2">
            <Button data-spatial="focusable" isDisabled={uninstall.isPending} onPress={close}>
              <Trans>Cancel</Trans>
            </Button>
            <Button
              data-spatial="focusable"
              variant="danger"
              isDisabled={uninstall.isPending}
              onPress={() => uninstall.mutate()}
            >
              {uninstall.isPending ? t`Uninstalling…` : t`Uninstall`}
            </Button>
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
