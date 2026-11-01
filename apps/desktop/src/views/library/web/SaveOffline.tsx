import { useLingui } from "@lingui/react/macro";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Dialog, Modal, ModalOverlay } from "react-aria-components";
import { libraryKeys, useLibraryApi } from "../../../api/provider";
import { useStorageApi } from "../../../api/StorageProvider";
import { InstallRootPicker } from "../../../components/storage/InstallRootPicker";

/** A web game's Play offline… (WEB-GAMES-PLAN §6), from its menu: where to
 * keep it, then the guided capture opens. */
export function SaveOfflineDialog({
  gameId,
  title,
  close,
}: {
  gameId: string;
  title: string;
  close: () => void;
}) {
  const { t } = useLingui();
  const api = useLibraryApi();
  const storage = useStorageApi();
  const client = useQueryClient();
  const save = useMutation({
    mutationFn: (rootId: string) => {
      if (!api.saveWebGameOffline) throw new Error(t`Offline web games aren't available here`);
      return api.saveWebGameOffline(gameId, rootId);
    },
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: libraryKeys.all });
      close();
    },
  });
  if (!storage) return null;
  return (
    <ModalOverlay
      isOpen
      isDismissable={!save.isPending}
      onOpenChange={(open) => {
        if (!open) close();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="max-h-[85vh] w-full max-w-xl overflow-auto rounded-panel border border-border-strong bg-surface-raised p-5">
        <Dialog aria-label={t`Where to keep ${title}`} className="outline-none">
          {save.error && (
            <p role="alert" className="mb-3 text-sm text-danger">
              {save.error.message}
            </p>
          )}
          <InstallRootPicker
            api={storage}
            onCancel={close}
            onSelect={(rootId) => save.mutate(rootId)}
          />
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}

/** Deletes a web game's offline copy; `null` where the library can't. */
export function useRemoveOffline() {
  const api = useLibraryApi();
  const client = useQueryClient();
  const mutation = useMutation({
    mutationFn: (gameId: string) => {
      if (!api.removeWebOffline) throw new Error("Offline web games aren't available here");
      return api.removeWebOffline(gameId);
    },
    onSuccess: () => client.invalidateQueries({ queryKey: libraryKeys.all }),
  });
  return api.removeWebOffline ? mutation : null;
}
