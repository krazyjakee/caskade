import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useRouter } from "@tanstack/react-router";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import { libraryKeys, useLibraryApi } from "../../../api/provider";
import { backTarget, useGoBack } from "../../game/origin";

/** Asks before a web game's menu removes it: scans never bring a web game
 * back, so it goes for good, with its offline copy and saves. On its own
 * page, the page goes back to where it was opened from. */
export function RemoveWebGameDialog({
  gameId,
  title,
  keepsGame,
  close,
}: {
  gameId: string;
  title: string;
  /** The game has other entries, which stay. */
  keepsGame: boolean;
  close: () => void;
}) {
  const { t } = useLingui();
  const api = useLibraryApi();
  const client = useQueryClient();
  const router = useRouter();
  const goBack = useGoBack();
  const remove = useMutation({
    mutationFn: () => {
      if (!api.removeWebGame) throw new Error(t`Web games aren't available here`);
      return api.removeWebGame(gameId);
    },
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: libraryKeys.all });
      close();
      const { pathname, search } = router.state.location;
      if (!keepsGame && pathname === `/games/${encodeURIComponent(gameId)}`)
        goBack(backTarget(gameId, (search as { from?: unknown }).from), "/library?filter=web");
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
            <Trans>Remove {title}?</Trans>
          </Heading>
          <p className="text-sm text-fg-muted">
            {keepsGame ? (
              <Trans>
                Its web version is removed: the offline copy and the progress saved in the game
                window are deleted, with its play sessions. The game's other versions stay in your
                library.
              </Trans>
            ) : (
              <Trans>
                It leaves your library: the offline copy and the progress saved in the game window
                are deleted, with its play sessions. Add its page again to play it again.
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
              {remove.isPending ? t`Removing…` : t({ message: "Remove", context: "web game" })}
            </Button>
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
