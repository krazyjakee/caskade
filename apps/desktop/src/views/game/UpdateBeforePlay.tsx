import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation } from "@tanstack/react-query";
import { ExternalLink } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import {
  formatVersion,
  type GameUpdate,
  useChangeGameUpdate,
  useGameUpdates,
  useGameUpdatesApi,
} from "../../api/updates";
import { useJobs } from "../activity/ActivityProvider";
import { UpdateProgress, useInstallUpdate } from "./EntryUpdate";

/** Play, with the update offer first: when a followed install has a newer
 * release, Play asks whether to update before starting. Installs no
 * provider follows start straight away, and so does any install whose
 * check can't be made. */
export function usePlayWithUpdates(gameId: string, start: (entryId: string) => void) {
  const api = useGameUpdatesApi();
  const followed = useGameUpdates(gameId).data;
  const [offer, setOffer] = useState<GameUpdate | null>(null);
  const check = useMutation({ mutationFn: (entryId: string) => api.beforePlay(entryId) });
  const play = (entryId: string) => {
    // Until the followed installs are known (a game menu's Play comes first),
    // the core decides: it answers at once for an install nobody follows.
    if (followed && !followed.some((each) => each.entryId === entryId)) {
      start(entryId);
      return;
    }
    check.mutate(entryId, {
      onSuccess: (update) => (update ? setOffer(update) : start(entryId)),
      onError: () => start(entryId),
    });
  };
  const dialog = offer && (
    <UpdateBeforePlayDialog
      key={offer.entryId}
      gameId={gameId}
      update={offer}
      close={() => setOffer(null)}
      play={() => {
        setOffer(null);
        start(offer.entryId);
      }}
    />
  );
  return { play, checking: check.isPending, asking: offer !== null, dialog };
}

/** Starts the game once its update job has completed. */
function PlayWhenUpdated({ jobId, play }: { jobId: string; play: () => void }) {
  const status = useJobs().data?.find((job) => job.id === jobId)?.status;
  const started = useRef(false);
  useEffect(() => {
    if (status === "completed" && !started.current) {
      started.current = true;
      play();
    }
  }, [status, play]);
  return null;
}

function UpdateBeforePlayDialog({
  gameId,
  update,
  close,
  play,
}: {
  gameId: string;
  update: GameUpdate;
  close: () => void;
  play: () => void;
}) {
  const { t } = useLingui();
  const install = useInstallUpdate(gameId);
  const change = useChangeGameUpdate(gameId, update.entryId);
  const [jobId, setJobId] = useState<string | null>(null);
  const latest = update.latest ?? "";
  const busy = install.isPending || change.isPending;
  const latestVersion = formatVersion(latest);
  const installedVersion = update.installed ? formatVersion(update.installed) : "";
  return (
    <ModalOverlay
      isOpen
      onOpenChange={(open) => {
        if (!open && !busy) close();
      }}
      isDismissable={!busy}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="max-h-[85vh] w-full max-w-lg overflow-auto rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg">
        <Dialog data-cs="update-offer" className="outline-none">
          <Heading slot="title" className="font-display text-xl font-semibold">
            <Trans>Update before playing?</Trans>
          </Heading>
          <p className="mt-3 text-sm text-fg-muted">
            {update.installed ? (
              <Trans comment="Versions, e.g. v1.2">
                {latestVersion} is out, and you have {installedVersion}. Updating keeps your saves
                and settings.
              </Trans>
            ) : (
              <Trans comment="A version, e.g. v1.2">
                {latestVersion} is out. Updating keeps your saves and settings.
              </Trans>
            )}
          </p>
          {update.latestUrl && (
            <a
              href={update.latestUrl}
              target="_blank"
              rel="noreferrer"
              className="mt-2 inline-flex items-center gap-1 text-sm text-accent-alt hover:underline"
            >
              <Trans>What's new</Trans>
              <ExternalLink aria-hidden="true" className="size-3.5" />
            </a>
          )}
          {jobId && (
            <div className="mt-4 text-sm">
              <UpdateProgress jobId={jobId} />
              <PlayWhenUpdated jobId={jobId} play={play} />
            </div>
          )}
          {(install.isError || change.isError) && (
            <p role="alert" className="mt-3 text-sm text-danger">
              {(install.error ?? change.error)?.message}
            </p>
          )}
          <div className="mt-6 flex flex-wrap justify-end gap-2">
            {jobId ? (
              <Button data-spatial="focusable" onPress={close}>
                <Trans>Close</Trans>
              </Button>
            ) : (
              <>
                <Button
                  data-spatial="focusable"
                  variant="ghost"
                  isDisabled={busy}
                  onPress={() =>
                    change.mutate({ type: "skip", version: latest }, { onSuccess: play })
                  }
                >
                  <Trans>Skip this version</Trans>
                </Button>
                <Button data-spatial="focusable" isDisabled={busy} onPress={play}>
                  <Trans>Play now</Trans>
                </Button>
                <Button
                  data-spatial="focusable"
                  variant="primary"
                  autoFocus
                  isDisabled={busy}
                  onPress={() => install.mutate(update.entryId, { onSuccess: setJobId })}
                >
                  {install.isPending ? t`Starting…` : t`Update and play`}
                </Button>
              </>
            )}
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
