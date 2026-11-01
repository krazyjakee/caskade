import { Button, Checkbox, TextField } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { Search } from "lucide-react";
import { useState } from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import { useCollectionMembers, useCollectionMembership } from "../../api/collections";
import { useGames } from "../../api/hooks";
import { formatNumber } from "../../api/presentation";
import type { CollectionSummary } from "../../api/types";

const SHOWN = 50;

/** Picks a manual collection's games from the library: a search, and a
 * checkbox per game that adds it or takes it out at once. */
export function AddGamesDialog({
  collection,
  close,
}: {
  collection: CollectionSummary;
  close: () => void;
}) {
  const { t } = useLingui();
  const [search, setSearch] = useState("");
  const games = useGames({ filter: "all", sort: "title", query: search }, { pageSize: SHOWN });
  const members = useCollectionMembers(collection.id);
  const membership = useCollectionMembership();
  const page = games.data?.pages[0];
  const inside = new Set(members.data ?? []);
  return (
    <ModalOverlay
      isOpen
      isDismissable
      onOpenChange={(open) => {
        if (!open) close();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="flex max-h-[85vh] w-full max-w-lg flex-col rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg">
        <Dialog data-cs="add-games" className="flex min-h-0 flex-col gap-4 outline-none">
          <Heading slot="title" className="font-display text-xl font-semibold">
            {t`Add games to ${{ name: collection.name }}`}
          </Heading>
          <TextField
            aria-label={t`Search the library`}
            placeholder={t`Search the library`}
            icon={Search}
            data-focus-default
            autoFocus
            value={search}
            onChange={setSearch}
          />
          <div data-part="games" className="-mx-2 min-h-40 overflow-y-auto px-2">
            {games.isPending || members.isPending ? (
              <p className="py-6 text-center text-sm text-fg-muted">{t`Loading games…`}</p>
            ) : games.error || members.error ? (
              <p role="alert" className="text-sm text-danger">
                {(games.error ?? members.error)?.message}
              </p>
            ) : page && page.items.length > 0 ? (
              <ul className="space-y-1">
                {page.items.map((game) => (
                  <li key={game.id}>
                    <Checkbox
                      data-spatial="focusable"
                      className="rounded-control px-2 py-1.5 data-hovered:bg-surface-hover"
                      isSelected={inside.has(game.id)}
                      isDisabled={membership.isPending}
                      onChange={(member) =>
                        membership.mutate({ id: collection.id, gameIds: [game.id], member })
                      }
                    >
                      <span className="flex min-w-0 flex-col">
                        <span className="truncate">{game.title}</span>
                        {game.platform && (
                          <span className="truncate text-xs text-fg-subtle">{game.platform}</span>
                        )}
                      </span>
                    </Checkbox>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="py-6 text-center text-sm text-fg-muted">
                {search.trim() ? t`No games match your search.` : t`Your library has no games yet.`}
              </p>
            )}
          </div>
          {page && page.total > page.items.length && (
            <p className="text-xs text-fg-subtle">
              {t`Showing ${{ shown: page.items.length }} of ${{ total: formatNumber(page.total) }}. Search to find the rest.`}
            </p>
          )}
          {membership.error && (
            <p role="alert" className="text-sm text-danger">
              {membership.error.message}
            </p>
          )}
          <div className="flex justify-end">
            <Button variant="primary" onPress={close}>
              <Trans>Done</Trans>
            </Button>
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
