import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useStorageApi } from "../../api/StorageProvider";
import { ConfirmDialog } from "../ConfirmDialog";

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
    <ConfirmDialog
      title={t`Uninstall ${title}?`}
      confirmLabel={t`Uninstall`}
      pendingLabel={t`Uninstalling…`}
      isPending={uninstall.isPending}
      error={uninstall.error}
      onConfirm={() => uninstall.mutate()}
      close={close}
    >
      <p>
        {location ? (
          <Trans>
            Its installed files are deleted from {location}, with the play sessions recorded for
            this install. The game stays in your library. The uninstall shows in Activity.
          </Trans>
        ) : (
          <Trans>
            Its installed files are deleted, with the play sessions recorded for this install. The
            game stays in your library. The uninstall shows in Activity.
          </Trans>
        )}
      </p>
    </ConfirmDialog>
  );
}
