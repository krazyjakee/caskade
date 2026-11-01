import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useRouter } from "@tanstack/react-router";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import { libraryKeys, useLibraryApi } from "../../api/provider";
import { backTarget, useGoBack } from "../../views/game/origin";

/** Asks before a game's menu removes a game Caskade installed or handed
 * off: no scan brings it back, so it goes for good. On its own page, the
 * page goes back to where it was opened from. */
export function RemoveGameDialog({
  gameId,
  title,
  uninstalls,
  close,
}: {
  gameId: string;
  title: string;
  /** It has files Caskade installed, which are deleted first. */
  uninstalls: boolean;
  close: () => void;
}) {
  const { t } = useLingui();
  const api = useLibraryApi();
  const client = useQueryClient();
  const router = useRouter();
  const goBack = useGoBack();
  const remove = useMutation({
    mutationFn: () => {
      if (!api.removeGame) throw new Error(t`Removing games isn't available here`);
      return api.removeGame(gameId);
    },
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["storage"] });
      void client.invalidateQueries({ queryKey: libraryKeys.all });
      close();
      const { pathname, search } = router.state.location;
      if (pathname === `/games/${encodeURIComponent(gameId)}`)
        goBack(backTarget(gameId, (search as { from?: unknown }).from));
    },
  });
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
            <Trans>Remove {title} from your library?</Trans>
          </Heading>
          <p className="text-sm text-fg-muted">
            {uninstalls ? (
              <Trans>
                The files Caskade installed for it are deleted, then it leaves your library with its
                play sessions. Files it didn't install stay where they are. The uninstall shows in
                Activity.
              </Trans>
            ) : (
              <Trans>
                It leaves your library with its play sessions. Its files, if it has any, stay where
                they are.
              </Trans>
            )}
          </p>
          {remove.error && (
            <p role="alert" className="text-sm text-danger">
              {remove.error.message}
            </p>
          )}
          <div className="flex justify-end gap-2">
            <Button data-spatial="focusable" isDisabled={remove.isPending} onPress={close}>
              <Trans>Cancel</Trans>
            </Button>
            <Button
              data-spatial="focusable"
              variant="danger"
              isDisabled={remove.isPending}
              onPress={() => remove.mutate()}
            >
              {remove.isPending ? t`Removing…` : t({ message: "Remove", context: "library game" })}
            </Button>
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
