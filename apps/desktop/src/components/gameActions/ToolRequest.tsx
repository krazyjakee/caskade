import { Button } from "@caskade/ui";
import { Trans } from "@lingui/react/macro";
import { useEffect, useRef, useState } from "react";
import { Dialog, Heading } from "react-aria-components";
import { useRunTool } from "../../api/actions";
import type { AgeRestriction } from "../../api/types";
import { AgeGateDialog } from "../../views/game/AgeGate";
import { ModalFrame } from "../ModalFrame";

/** Runs one of a release's tools once (#178), as its menu item asked: an
 * admin's PIN first when the age limit asks for one, and why it failed
 * when it couldn't start. A tool has no play session, so nothing follows
 * once it started. */
export function ToolRequest({
  entryId,
  actionId,
  name,
  close,
}: {
  entryId: string;
  actionId: string;
  name: string;
  close: () => void;
}) {
  const run = useRunTool();
  const [gate, setGate] = useState<AgeRestriction | null>(null);
  const start = () =>
    run.mutate(
      { entryId, actionId },
      {
        onSuccess: (outcome) => {
          if (outcome.type === "age_restricted") setGate(outcome.restriction);
          else close();
        },
      },
    );
  const started = useRef(false);
  useEffect(() => {
    if (started.current) return;
    started.current = true;
    start();
  });
  if (gate)
    return (
      <AgeGateDialog
        restriction={gate}
        close={close}
        approved={() => {
          setGate(null);
          start();
        }}
      />
    );
  if (!run.isError) return null;
  return (
    <ModalFrame
      close={close}
      className="w-full max-w-md rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg"
    >
      <Dialog role="alertdialog" data-cs="tool-failed" className="space-y-4 outline-none">
        <Heading slot="title" className="font-display text-xl font-semibold">
          <Trans>{name} didn't start</Trans>
        </Heading>
        <p className="text-sm text-fg-muted">{run.error.message}</p>
        <div className="flex justify-end">
          <Button data-spatial="focusable" onPress={close}>
            <Trans>OK</Trans>
          </Button>
        </div>
      </Dialog>
    </ModalFrame>
  );
}
