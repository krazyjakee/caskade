import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { useOptionalLibraryApi } from "../../api/provider";
import type { BeforePlay, SaveVersion } from "../../api/saves";
import { useSaveDeviceName } from "../../components/saves/devices";
import { SaveConflictDialog } from "../../components/saves/SaveConflictDialog";

/** How long the note after a save check stays up. */
const NOTE_MS = 10_000;

/** Play, with the saves first (#239): a newer save from another device is
 * put in place before the game starts, and when both devices played since
 * they last agreed, Play asks which save to use. The check waits for other
 * devices a few seconds at most; when it can't be made, or a save hasn't
 * arrived, the game starts anyway and a note says so. */
export function usePlayWithSaves(start: (entryId: string) => void) {
  const api = useOptionalLibraryApi()?.saves;
  const client = useQueryClient();
  const [conflict, setConflict] = useState<{ entryId: string; versions: SaveVersion[] } | null>(
    null,
  );
  const [note, setNote] = useState<BeforePlay | null>(null);
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
  const play = (entryId: string) => {
    if (!api) {
      start(entryId);
      return;
    }
    check.mutate(entryId, {
      onSuccess: (result) => {
        settled();
        if (result.check.type === "conflict") {
          setConflict({ entryId, versions: result.check.versions });
          return;
        }
        setNote(worthSaying(result) ? result : null);
        start(entryId);
      },
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
      {note && <SaveNote result={note} dismiss={() => setNote(null)} />}
    </>
  );
  return { play, checking: check.isPending, dialog };
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
