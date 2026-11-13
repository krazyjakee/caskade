/** A device asking to pair with this one to play its games (#265): a
 * prompt over any view while this device lets others play, for an admin
 * to enter the code that device shows. Settings → Streaming lists the
 * same requests, so the prompt stays away while it is open. A request
 * dismissed here waits there until it expires (five minutes).
 *
 * Mounted in the shell, inside the input provider, so a pad reaches it. */

import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useRouterState } from "@tanstack/react-router";
import { useState } from "react";
import { Dialog, Heading } from "react-aria-components";
import { useHostStatus, usePairRequests } from "../../api/streaming";
import { useDeviceAdmin } from "../../views/settings/devices/admin";
import { ModalFrame } from "../ModalFrame";
import { PairRequestForm } from "./PairRequestForm";

export function StreamPairingHost() {
  const admin = useDeviceAdmin();
  const hosting = useHostStatus({ enabled: admin }).data?.state === "ready";
  const requests = usePairRequests({ enabled: admin && hosting }).data ?? [];
  const inSettings = useRouterState({
    select: (state) =>
      state.location.pathname === "/settings" &&
      (state.location.search as { tab?: string }).tab === "streaming",
  });
  const [dismissed, setDismissed] = useState<readonly string[]>([]);
  const request = requests.find((each) => !dismissed.includes(each.id));
  if (!admin || !hosting || inSettings || !request) return null;
  const dismiss = () => setDismissed((ids) => [...ids, request.id]);
  return <PromptDialog key={request.id} request={request} dismiss={dismiss} />;
}

function PromptDialog({
  request,
  dismiss,
}: {
  request: { id: string; name: string; address: string };
  dismiss: () => void;
}) {
  const { t } = useLingui();
  const device = request.name;
  return (
    <ModalFrame
      close={dismiss}
      className="w-full max-w-lg rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg"
    >
      <Dialog
        data-cs="stream-pair-prompt"
        aria-label={t`${device} wants to play from this device`}
        className="outline-none"
      >
        <Heading slot="title" className="font-display text-xl font-semibold">
          {t`${device} wants to play from this device`}
        </Heading>
        <p className="mt-2 text-sm text-fg-muted">
          <Trans>Enter the code {device} shows to let it play this device's games.</Trans>
        </p>
        <div className="mt-4">
          <PairRequestForm request={request} done={dismiss} autoFocus />
        </div>
        <div className="mt-4 flex justify-end">
          <Button variant="ghost" onPress={dismiss}>
            <Trans>Not now</Trans>
          </Button>
        </div>
      </Dialog>
    </ModalFrame>
  );
}
