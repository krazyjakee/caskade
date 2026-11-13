import { Trans, useLingui } from "@lingui/react/macro";
import { useDeleteCollection } from "../../api/collections";
import type { CollectionSummary } from "../../api/types";
import { ConfirmDialog } from "../ConfirmDialog";

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
    <ConfirmDialog
      title={t`Delete ${name}?`}
      confirmLabel={t`Delete`}
      pendingLabel={t`Deleting…`}
      isPending={remove.isPending}
      error={remove.error}
      onConfirm={() =>
        remove.mutate(collection.id, {
          onSuccess: () => {
            close();
            onDeleted?.();
          },
        })
      }
      close={close}
    >
      <p>
        <Trans>The collection goes. Its games stay in your library.</Trans>
      </p>
    </ConfirmDialog>
  );
}
