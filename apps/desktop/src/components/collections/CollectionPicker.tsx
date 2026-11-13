import { Button, Checkbox, TextField } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { Plus } from "lucide-react";
import { useState } from "react";
import { Dialog, Form, Heading } from "react-aria-components";
import {
  useCollectionMembership,
  useCollections,
  useGameCollections,
  useSaveCollection,
} from "../../api/collections";
import { ModalFrame } from "../ModalFrame";

/** The manual collections a game is in, from its menu: a checkbox per
 * collection, and a new one with the game in it. */
export function CollectionPicker({
  gameId,
  title,
  close,
}: {
  gameId: string;
  title: string;
  close: () => void;
}) {
  const { t } = useLingui();
  const collections = useCollections();
  const of = useGameCollections(gameId);
  const membership = useCollectionMembership();
  const save = useSaveCollection();
  const [name, setName] = useState("");
  const manual = (collections.data ?? []).filter((c) => c.kind.type === "manual");
  const inside = new Set(of.data ?? []);
  const busy = membership.isPending || save.isPending;
  const error = collections.error ?? of.error ?? membership.error ?? save.error;
  return (
    <ModalFrame
      close={close}
      className="flex max-h-[85vh] w-full max-w-md flex-col rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg"
    >
      <Dialog data-cs="collection-picker" className="flex min-h-0 flex-col gap-4 outline-none">
        <Heading slot="title" className="font-display text-xl font-semibold">
          <Trans>Collections for {title}</Trans>
        </Heading>
        {collections.isPending || of.isPending ? (
          <p className="text-sm text-fg-muted">{t`Loading collections…`}</p>
        ) : manual.length > 0 ? (
          <ul data-part="collections" className="-mx-2 space-y-1 overflow-y-auto px-2">
            {manual.map((collection, i) => (
              <li key={collection.id}>
                <Checkbox
                  data-spatial="focusable"
                  data-focus-default={i === 0 || undefined}
                  className="rounded-control px-2 py-1.5 data-hovered:bg-surface-hover"
                  isSelected={inside.has(collection.id)}
                  isDisabled={busy}
                  onChange={(member) =>
                    membership.mutate({ id: collection.id, gameIds: [gameId], member })
                  }
                >
                  {collection.name}
                </Checkbox>
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-sm text-fg-muted">{t`You have no collections to add games to yet.`}</p>
        )}
        <Form
          className="flex items-end gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            save.mutate(
              { name: name.trim(), description: "", kind: { type: "manual" } },
              {
                onSuccess: (id) => {
                  setName("");
                  membership.mutate({ id, gameIds: [gameId], member: true });
                },
              },
            );
          }}
        >
          <TextField
            className="flex-1"
            label={t`New collection`}
            placeholder={t({ message: "Name", context: "collection" })}
            data-focus-default={manual.length === 0 || undefined}
            value={name}
            onChange={setName}
            maxLength={200}
            isDisabled={busy}
          />
          <Button type="submit" icon={Plus} isDisabled={!name.trim() || busy}>
            {t({ message: "Create", context: "collection" })}
          </Button>
        </Form>
        <p className="text-xs text-fg-subtle">{t`Smart collections pick their own games.`}</p>
        {error && (
          <p role="alert" className="text-sm text-danger">
            {error.message}
          </p>
        )}
        <div className="flex justify-end">
          <Button variant="primary" onPress={close}>
            <Trans>Done</Trans>
          </Button>
        </div>
      </Dialog>
    </ModalFrame>
  );
}
