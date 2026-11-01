import { useLingui } from "@lingui/react/macro";
import { Pencil, Pin, PinOff, Plus, Trash2 } from "lucide-react";
import { useState } from "react";
import { useSetCollectionPinned } from "../../api/collections";
import type { CollectionSummary } from "../../api/types";
import type { MenuAction } from "../ActionMenu";
import { AddGamesDialog } from "./AddGamesDialog";
import { CollectionDialog } from "./CollectionDialog";
import { DeleteCollectionDialog } from "./DeleteCollectionDialog";

type Open = { type: "edit" | "add" | "delete"; collection: CollectionSummary };

/** A collection's commands (edit, add games, pin, delete) as menu actions,
 * and the dialogs they open. Render `dialogs` once. */
export function useCollectionActions({ onDeleted }: { onDeleted?: () => void } = {}) {
  const { t } = useLingui();
  const [open, setOpen] = useState<Open | null>(null);
  const pin = useSetCollectionPinned();
  const close = () => setOpen(null);
  const actions = (collection: CollectionSummary): MenuAction[] => [
    {
      id: "edit",
      label: collection.kind.type === "smart" ? t`Edit rules…` : t`Edit…`,
      icon: Pencil,
      onAction: () => setOpen({ type: "edit", collection }),
    },
    ...(collection.kind.type === "manual"
      ? [
          {
            id: "add",
            label: t`Add games…`,
            icon: Plus,
            onAction: () => setOpen({ type: "add", collection }),
          },
        ]
      : []),
    {
      id: "pin",
      label: collection.pinned ? t`Unpin` : t`Pin to the top`,
      icon: collection.pinned ? PinOff : Pin,
      isDisabled: pin.isPending,
      onAction: () => pin.mutate({ id: collection.id, pinned: !collection.pinned }),
    },
    {
      id: "delete",
      label: t`Delete…`,
      icon: Trash2,
      tone: "danger",
      onAction: () => setOpen({ type: "delete", collection }),
    },
  ];
  const dialogs =
    open?.type === "edit" ? (
      <CollectionDialog collection={open.collection} close={close} />
    ) : open?.type === "add" ? (
      <AddGamesDialog collection={open.collection} close={close} />
    ) : open?.type === "delete" ? (
      <DeleteCollectionDialog collection={open.collection} close={close} onDeleted={onDeleted} />
    ) : null;
  return {
    actions,
    open: (type: Open["type"], collection: CollectionSummary) => setOpen({ type, collection }),
    dialogs,
    error: pin.error,
  };
}
