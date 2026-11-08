import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import { useOptionalLibraryApi } from "../../api/provider";
import type { BeforePlay, SaveVersion } from "../../api/saves";
import { useSaveDeviceName } from "../../components/saves/devices";
import { SaveConflictDialog } from "../../components/saves/SaveConflictDialog";

/** How long the note after a save check stays up. */
const NOTE_MS = 10_000;
/** While another device's save is on its way: how often to look, and for
 * how long before the game starts with the save here. */
const ARRIVING_POLL_MS = 2_000;
const ARRIVING_MS = 60_000;

/** Play, with the saves first (#239): a newer save from another device is
 * put in place before the game starts, and when both devices played since
 * they last agreed, Play asks which save to use. The check waits for other
 * devices a few seconds at most; when it can't be made, the game starts
 * anyway and a note says so. A newer save whose files are still on their
 * way (the other device's session just ended, #240) is waited for, with
 * Skip. */
export function usePlayWithSaves(start: (entryId: string) => void) {
  const api = useOptionalLibraryApi()?.saves;
  const client = useQueryClient();
  const [conflict, setConflict] = useState<{ entryId: string; versions: SaveVersion[] } | null>(
    null,
  );
  const [note, setNote] = useState<BeforePlay | null>(null);
  const [arriving, setArriving] = useState<{ entryId: string; version: SaveVersion } | null>(null);
  const settled = () => void client.invalidateQueries({ queryKey: ["save-versions"] });
  const check = useMutation({
    mutationFn: (entryId: string) => {
      if (!api) throw new Error("Saves are unavailable");
      return api.beforePlay(entryId);
    },
  });
  const choose = useMutation({
    mutationFn: ({ entryId, versionId }: { entryId: string; versionId: string }) => {
      if (!api) throw new Error("Saves are unavailable");
      return api.choose(entryId, versionId);
    },
    onSettled: settled,
  });
  const settle = (entryId: string, result: BeforePlay, waited = false) => {
    settled();
    if (result.check.type === "conflict") {
      setConflict({ entryId, versions: result.check.versions });
      return;
    }
    if (result.check.type === "notArrived" && !waited) {
      setArriving({ entryId, version: result.check.version });
      return;
    }
    setNote(worthSaying(result) ? result : null);
    start(entryId);
  };
  const play = (entryId: string) => {
    if (!api) {
      start(entryId);
      return;
    }
    check.mutate(entryId, {
      onSuccess: (result) => settle(entryId, result),
      // Saves never stop a launch.
      onError: () => start(entryId),
    });
  };
  const dialog = (
    <>
      {conflict && (
        <SaveConflictDialog
          key={conflict.entryId}
          versions={conflict.versions}
          busy={choose.isPending}
          error={choose.isError ? choose.error.message : undefined}
          close={() => {
            setConflict(null);
            choose.reset();
          }}
          choose={(versionId) =>
            choose.mutate(
              { entryId: conflict.entryId, versionId },
              {
                onSuccess: () => {
                  setConflict(null);
                  start(conflict.entryId);
                },
              },
            )
          }
        />
      )}
      {arriving && api && (
        <WaitForSave
          key={arriving.entryId}
          version={arriving.version}
          check={() => api.beforePlay(arriving.entryId)}
          done={(result) => {
            setArriving(null);
            settle(arriving.entryId, result, true);
          }}
          cancel={() => setArriving(null)}
        />
      )}
      {note && <SaveNote result={note} dismiss={() => setNote(null)} />}
    </>
  );
  return {
    play,
    checking: check.isPending,
    /** A conflict's question, or the wait for a save on its way. */
    asking: conflict !== null || arriving !== null,
    dialog,
  };
}

/** Another device's newer save is on its way: looks again every couple of
 * seconds, and Skip (or a minute) starts the game with the save here. */
function WaitForSave({
  version,
  check,
  done,
  cancel,
}: {
  version: SaveVersion;
  check: () => Promise<BeforePlay>;
  done: (result: BeforePlay) => void;
  cancel: () => void;
}) {
  const { t } = useLingui();
  const deviceName = useSaveDeviceName();
  const device = deviceName(version);
  const latest = useRef<BeforePlay>({
    check: { type: "notArrived", version },
    checked: true,
    warnings: [],
  });
  const finish = useRef(done);
  finish.current = done;
  const ask = useRef(check);
  ask.current = check;
  useEffect(() => {
    let stopped = false;
    const until = Date.now() + ARRIVING_MS;
    const look = async () => {
      while (!stopped && Date.now() < until) {
        await new Promise((resolve) => setTimeout(resolve, ARRIVING_POLL_MS));
        if (stopped) return;
        try {
          latest.current = await ask.current();
        } catch {
          continue;
        }
        if (stopped) return;
        if (latest.current.check.type !== "notArrived") {
          finish.current(latest.current);
          return;
        }
      }
      if (!stopped) finish.current(latest.current);
    };
    void look();
    return () => {
      stopped = true;
    };
  }, []);
  return (
    <ModalOverlay
      isOpen
      isDismissable
      onOpenChange={(open) => {
        if (!open) cancel();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="w-full max-w-md rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg">
        <Dialog data-cs="save-arriving" className="outline-none">
          <Heading slot="title" className="font-display text-xl font-semibold">
            {t`Waiting for the save from ${device}`}
          </Heading>
          <p role="status" className="mt-2 text-sm text-fg-muted">
            <Trans>
              It was just played there, and its save is on its way. Skip to play with the save on
              this device.
            </Trans>
          </p>
          <div className="mt-6 flex justify-end gap-2">
            <Button data-spatial="focusable" onPress={cancel}>
              <Trans>Cancel</Trans>
            </Button>
            <Button
              data-spatial="focusable"
              data-focus-default=""
              variant="primary"
              onPress={() => finish.current(latest.current)}
            >
              <Trans>Skip</Trans>
            </Button>
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}

function worthSaying(result: BeforePlay): boolean {
  return (
    !result.checked ||
    result.warnings.length > 0 ||
    result.check.type === "restored" ||
    result.check.type === "notArrived"
  );
}

/** What the check before the launch did, when it's worth knowing. */
function SaveNote({ result, dismiss }: { result: BeforePlay; dismiss: () => void }) {
  const { t } = useLingui();
  const deviceName = useSaveDeviceName();
  useEffect(() => {
    const timer = setTimeout(dismiss, NOTE_MS);
    return () => clearTimeout(timer);
  }, [dismiss]);
  const lines: string[] = [];
  const check = result.check;
  if (check.type === "restored") {
    const device = deviceName(check.version);
    lines.push(t`Started with the save from ${device}.`);
  }
  if (check.type === "notArrived") {
    const device = deviceName(check.version);
    lines.push(
      t`A newer save from ${device} hasn't arrived yet, so the game started with the save on this device.`,
    );
  }
  if (!result.checked)
    lines.push(t`Not every paired device could be reached, so one of them may have a newer save.`);
  lines.push(...result.warnings);
  return (
    <div
      role="status"
      data-cs="save-note"
      className="fixed right-4 bottom-12 z-40 flex max-w-sm items-start gap-3 rounded-panel border border-border-strong bg-surface-raised p-4 text-sm shadow-lg"
    >
      <div className="space-y-1">
        {lines.map((line) => (
          <p key={line}>{line}</p>
        ))}
      </div>
      <Button data-spatial="focusable" variant="ghost" onPress={dismiss}>
        <Trans>Dismiss</Trans>
      </Button>
    </div>
  );
}
