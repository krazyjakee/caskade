import { Button, Select } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import { useStorageApi } from "../../api/StorageProvider";
import type { StorageApi } from "../../api/storage";
import { formatBytes, rootBlockReason } from "../../api/storage";

interface MoveGameProps {
  gameId: string;
  entryId?: string;
  isRunning?: boolean;
  api?: StorageApi;
}

/** The game page's button for `MoveGameDialog`. */
export function MoveGameAction(props: MoveGameProps) {
  const api = useStorageApi();
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button isDisabled={!(props.api ?? api) || props.isRunning} onPress={() => setOpen(true)}>
        <Trans>Move installed files…</Trans>
      </Button>
      <MoveGameDialog {...props} isOpen={open} onOpenChange={setOpen} />
    </>
  );
}

/** Loads storage roots only while it is open; a library grid creates no
 * per-card root queries. External/delegated/running games receive a
 * concrete reason. */
export function MoveGameDialog({
  gameId,
  entryId,
  isRunning = false,
  api: provided,
  isOpen: open,
  onOpenChange: setOpen,
}: MoveGameProps & { isOpen: boolean; onOpenChange: (open: boolean) => void }) {
  const { t } = useLingui();
  const context = useStorageApi();
  const api = provided ?? context;
  const [entry, setEntry] = useState("");
  const [target, setTarget] = useState("");
  const client = useQueryClient();
  const roots = useQuery({
    queryKey: ["storage", "roots"],
    queryFn: () => {
      if (!api) throw new Error(t`Storage service is unavailable`);
      return api.list();
    },
    enabled: open && Boolean(api),
    refetchOnWindowFocus: true,
  });
  const candidates = (roots.data ?? []).flatMap((root) =>
    root.games
      .filter((game) => game.gameId === gameId && (!entryId || game.entryId === entryId))
      .map((game) => ({ root, game })),
  );
  const selected =
    candidates.find((candidate) => candidate.game.entryId === (entryId ?? entry)) ?? candidates[0];
  const needs = { estimatedBytes: selected?.game.sizeBytes, executable: true };
  const targets = (roots.data ?? []).filter((root) => root.root.id !== selected?.root.root.id);
  const destination =
    targets.find((root) => root.root.id === target) ??
    targets.find((root) => !rootBlockReason(root, needs));
  const reason = isRunning
    ? t`Quit this game before moving its files.`
    : (selected?.game.moveBlocked ??
      (!selected
        ? t`Only Caskade-managed installations can be moved.`
        : destination
          ? rootBlockReason(destination, needs)
          : t`Add another eligible storage location in Settings → Storage.`));
  const move = useMutation({
    mutationFn: async () => {
      if (!api || !selected || !destination || reason)
        throw new Error(reason ?? t`Storage selection is unavailable`);
      await api.move(selected.game.entryId, destination.root.id);
    },
    onSuccess: () => {
      setOpen(false);
      void client.invalidateQueries({ queryKey: ["storage"] });
      void client.invalidateQueries({ queryKey: ["library"] });
    },
  });
  return (
    <ModalOverlay
      isOpen={open}
      onOpenChange={(value) => {
        if (!move.isPending) setOpen(value);
      }}
      isDismissable={!move.isPending}
      className="fixed inset-0 z-50 flex items-center justify-center bg-surface-base/80 p-4"
    >
      <Modal className="w-full max-w-lg rounded-panel border border-border bg-surface-overlay p-5 shadow-xl">
        <Dialog className="flex flex-col gap-4 outline-none">
          <Heading slot="title" className="font-display text-xl">
            <Trans>Move installed files</Trans>
          </Heading>
          {roots.isPending ? (
            <p role="status">{t`Checking locations…`}</p>
          ) : roots.error ? (
            <p role="alert">{t`Locations could not be loaded.`}</p>
          ) : (
            <>
              {candidates.length > 1 && (
                <Select
                  label={t`Installation`}
                  options={candidates.map(({ root, game }) => ({
                    id: game.entryId,
                    label: `${root.root.label ?? root.root.path} · ${game.relativeDir}`,
                  }))}
                  value={selected?.game.entryId ?? ""}
                  onChange={setEntry}
                />
              )}
              <p className="text-sm text-fg-muted">
                {selected
                  ? t`${{ title: selected.game.title }} · ${{ size: formatBytes(selected.game.sizeBytes) }}. Playtime and library identity are kept.`
                  : t`Games owned by other launchers stay in their launcher locations.`}
              </p>
              <Select
                label={t`Destination`}
                options={targets.map((root) => ({
                  id: root.root.id,
                  label: t`${{ location: root.root.label ?? root.root.path }} · ${{ free: formatBytes(root.capabilities.freeBytes) }} free`,
                }))}
                value={destination?.root.id ?? ""}
                onChange={setTarget}
                isDisabled={!targets.length || move.isPending}
              />
              {reason && (
                <p role="alert" className="text-sm text-warning">
                  {reason}
                </p>
              )}
              <p className="text-xs text-fg-muted">
                <Trans>The move appears in Activity, where it can be cancelled.</Trans>
              </p>
            </>
          )}
          {move.error && (
            <p role="alert" className="text-sm text-danger">
              {move.error.message}
            </p>
          )}
          <div className="flex justify-end gap-2">
            <Button isDisabled={move.isPending} onPress={() => setOpen(false)}>
              <Trans>Cancel</Trans>
            </Button>
            <Button
              isDisabled={
                roots.isPending || Boolean(roots.error) || Boolean(reason) || move.isPending
              }
              onPress={() => move.mutate()}
            >
              {move.isPending ? t`Moving…` : t`Move game`}
            </Button>
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
