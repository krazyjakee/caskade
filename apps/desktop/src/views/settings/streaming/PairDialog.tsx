import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import { errorMessage } from "../../../api/reply";
import {
  type PairingFinished,
  type PairingStarted,
  type StreamHost,
  streamingKey,
  useStreamingApi,
} from "../../../api/streaming";

type Step =
  | { type: "asking" }
  | { type: "code"; started: PairingStarted }
  | { type: "finished"; finished: PairingFinished; code?: string }
  | { type: "failed"; message: string };

/** Pairing this device with one to stream from (#265): its code, to enter
 * in Caskade on the other device, until that device answers. A wrong code
 * or a request that expired can be tried again. */
export function PairDialog({ host, close }: { host: StreamHost; close: () => void }) {
  const { t } = useLingui();
  const api = useStreamingApi();
  const client = useQueryClient();
  const [step, setStep] = useState<Step>({ type: "asking" });
  const [attempt, setAttempt] = useState(0);
  const device = host.name;
  // biome-ignore lint/correctness/useExhaustiveDependencies: `attempt` asks again.
  useEffect(() => {
    let current = true;
    let code: string | undefined;
    const stop = api.onPairingFinished((finished) => {
      if (!current || finished.hostId !== host.id) return;
      setStep({ type: "finished", finished, code });
      void client.invalidateQueries({ queryKey: streamingKey });
    });
    setStep({ type: "asking" });
    api.pairWith(host.id).then(
      (started) => {
        code = started.code;
        if (current) setStep((now) => (now.type === "asking" ? { type: "code", started } : now));
      },
      (error: unknown) => current && setStep({ type: "failed", message: errorMessage(error) }),
    );
    return () => {
      current = false;
      stop();
    };
  }, [api, client, host.id, attempt]);
  const again = () => setAttempt((n) => n + 1);
  const outcome = step.type === "finished" ? step.finished.outcome : null;
  return (
    <ModalOverlay
      isOpen
      isDismissable
      onOpenChange={(open) => {
        if (!open) close();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="w-full max-w-md rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg">
        <Dialog
          data-cs="pair-dialog"
          data-status={outcome ?? step.type}
          aria-label={t`Pair with ${device}`}
          className="outline-none"
        >
          <Heading slot="title" className="font-display text-xl font-semibold">
            {t`Pair with ${device}`}
          </Heading>
          {step.type === "asking" && (
            <p role="status" className="mt-3 text-sm text-fg-muted">
              <Trans>Asking {device}…</Trans>
            </p>
          )}
          {step.type === "code" && step.started.automatic && (
            <p role="status" className="mt-3 text-sm text-fg-muted">
              <Trans>
                {device} is one of your devices, so it's pairing without a code. This takes a
                moment.
              </Trans>
            </p>
          )}
          {step.type === "code" && !step.started.automatic && (
            <>
              <p className="mt-3 text-sm text-fg-muted">
                {/* Only an admin there sees where it goes (#52). */}
                <Trans>
                  On {device}, someone signed in to Caskade as an admin enters this code in Settings
                  → Streaming:
                </Trans>
              </p>
              <p
                data-part="code"
                className="mt-4 text-center font-mono text-5xl font-semibold tracking-[0.4em] text-accent-alt"
              >
                {step.started.code}
              </p>
              <p role="status" className="mt-4 text-sm text-fg-muted">
                <Trans>Waiting for {device}…</Trans>
              </p>
            </>
          )}
          {step.type === "finished" && (
            <p
              role={outcome === "paired" ? "status" : "alert"}
              className={`mt-3 text-sm ${outcome === "paired" ? "" : "text-danger"}`}
            >
              {outcome === "paired" ? (
                <Trans>
                  Paired. {device}'s games show in your library as Stream from {device}.
                </Trans>
              ) : outcome === "wrongCode" ? (
                <Trans>The code entered on {device} didn't match. Try again with a new code.</Trans>
              ) : outcome === "declined" ? (
                <Trans>{device} declined.</Trans>
              ) : outcome === "expired" ? (
                <Trans>
                  Nobody entered the code on {device} in time. It takes an admin signed in there.
                </Trans>
              ) : (
                (step.finished.message ?? <Trans>Pairing with {device} didn't work.</Trans>)
              )}
            </p>
          )}
          {step.type === "failed" && (
            <p role="alert" className="mt-3 text-sm text-danger">
              {step.message}
            </p>
          )}
          <div data-part="actions" className="mt-6 flex justify-end gap-2">
            {outcome === "paired" ? (
              <Button variant="primary" autoFocus onPress={close}>
                <Trans>Done</Trans>
              </Button>
            ) : (
              <>
                {(step.type === "failed" ||
                  outcome === "wrongCode" ||
                  outcome === "expired" ||
                  outcome === "failed") && (
                  <Button variant="primary" autoFocus onPress={again}>
                    <Trans>Try again</Trans>
                  </Button>
                )}
                <Button autoFocus={step.type === "code" || step.type === "asking"} onPress={close}>
                  {outcome ? <Trans>Close</Trans> : <Trans>Cancel</Trans>}
                </Button>
              </>
            )}
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
