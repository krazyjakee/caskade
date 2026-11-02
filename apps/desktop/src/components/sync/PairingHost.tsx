/** Pairing over any view (#226, #231): the code of a pairing this device
 * started or another device started, for both users to compare and
 * confirm; and a `caskade://pair` link opened from outside, which pairs only
 * when an admin says so here (a web page can open such a link).
 *
 * Mounted in the shell, inside the input provider, so a pad reaches every
 * control; in console mode the code is large enough to read from a sofa. */
import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { type ReactNode, useEffect, useState } from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import { formatDateTime } from "../../api/presentation";
import {
  type Pairing,
  type PairingLinkTarget,
  useOpenedLink,
  useSyncAction,
  useSyncStatus,
} from "../../api/sync";
import { useMode } from "../../shell/console/mode";
import { useDeviceAdmin } from "../../views/settings/devices/admin";
import { FailureNotice } from "../FailureNotice";

export function PairingHost() {
  const { data } = useSyncStatus();
  const opened = useOpenedLink().data ?? null;
  // A pairing dismissed here stays out of sight until the core forgets it.
  const [dismissed, setDismissed] = useState<string | null>(null);
  const pairing = data?.pairing ?? null;
  useEffect(() => {
    if (!pairing) setDismissed(null);
  }, [pairing]);
  if (pairing && pairing.deviceId !== dismissed) {
    // Keyed by state: a pad keeps a focused button when the dialog changes.
    return (
      <PairDialog
        key={`${pairing.deviceId}:${pairing.state}`}
        pairing={pairing}
        onDismiss={() => setDismissed(pairing.deviceId)}
      />
    );
  }
  if (opened) return <OpenedLinkDialog target={opened} />;
  return null;
}

function Frame({
  children,
  onClose,
  label,
}: {
  children: ReactNode;
  onClose: () => void;
  label: string;
}) {
  return (
    <ModalOverlay
      isOpen
      isDismissable
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="w-full max-w-md rounded-panel border border-border-strong bg-surface-raised p-6">
        <Dialog
          data-cs="pair-device"
          data-focus-scope="pair-device"
          aria-label={label}
          className="space-y-4 outline-none"
        >
          {children}
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}

function PairDialog({ pairing, onDismiss }: { pairing: Pairing; onDismiss: () => void }) {
  const { t } = useLingui();
  const admin = useDeviceAdmin();
  const { mode } = useMode();
  const confirm = useSyncAction((api, id: string) => api.confirmPairing(id));
  const cancel = useSyncAction((api, id: string) => api.cancelPairing(id));
  // Cancels it, then dismisses it: whoever cancelled needs no word that it
  // was cancelled. One action, so it finishes after the dialog has gone.
  const refuse = useSyncAction(async (api, id: string) => {
    await api.cancelPairing(id);
    await api.cancelPairing(id);
  });
  const name = pairing.name;
  const waiting = pairing.state === "waiting";
  // A finished pairing is dismissed here and in the core.
  const close = () => {
    if (admin) cancel.mutate(pairing.deviceId);
    onDismiss();
  };
  const code = `${pairing.code.slice(0, 3)} ${pairing.code.slice(3)}`;
  return (
    <Frame label={t`Pair with ${name}`} onClose={close}>
      <div data-part="state" data-status={pairing.state} className="space-y-4">
        <Heading slot="title" className="font-display text-xl font-semibold">
          {pairing.state === "paired" ? (
            <Trans>Paired with {name}</Trans>
          ) : pairing.state === "cancelled" ? (
            <Trans>Pairing with {name} was cancelled</Trans>
          ) : pairing.state === "failed" ? (
            <Trans>Pairing with {name} didn't finish</Trans>
          ) : pairing.incoming ? (
            <Trans>{name} wants to pair</Trans>
          ) : (
            <Trans>Pair with {name}?</Trans>
          )}
        </Heading>
        {waiting && (
          <>
            <p className="text-sm text-fg-muted">
              <Trans>{name} shows a code too. Pair only if it is the same as this one.</Trans>
            </p>
            <p
              data-part="code"
              className={`text-center font-mono font-semibold tracking-[0.3em] ${
                mode === "console" ? "text-7xl" : "text-4xl"
              }`}
            >
              {code}
            </p>
            {pairing.confirmed && (
              <p role="status" className="text-center text-sm text-fg-muted">
                <Trans>Waiting for {name} to confirm…</Trans>
              </p>
            )}
            {!admin && (
              <p className="text-sm text-fg-muted">
                <Trans>Only an admin can pair devices. Ask one to confirm here.</Trans>
              </p>
            )}
          </>
        )}
        {pairing.state === "paired" && (
          <p className="text-sm text-fg-muted">
            <Trans>
              The first sync starts now. Your library, play history and collections show on both
              devices; each keeps its own games and settings.
            </Trans>
          </p>
        )}
        {pairing.state === "cancelled" && (
          <p className="text-sm text-fg-muted">
            <Trans>Nothing was paired. Start again from either device.</Trans>
          </p>
        )}
        {pairing.state === "failed" && (
          <p className="text-sm text-fg-muted">
            <Trans>
              The devices lost each other, or nobody confirmed in time. Nothing was paired.
            </Trans>
          </p>
        )}
        {(confirm.error ?? cancel.error) && (
          <FailureNotice variant="inline" error={confirm.error ?? cancel.error} />
        )}
        <div className="flex justify-end gap-2">
          {waiting && admin ? (
            <>
              <Button
                onPress={() => {
                  refuse.mutate(pairing.deviceId);
                  onDismiss();
                }}
              >
                <Trans>They don't match</Trans>
              </Button>
              <Button
                variant="primary"
                data-focus-default
                isDisabled={confirm.isPending || pairing.confirmed}
                onPress={() => confirm.mutate(pairing.deviceId)}
              >
                <Trans>They match: pair</Trans>
              </Button>
            </>
          ) : (
            <Button variant="primary" data-focus-default onPress={close}>
              {waiting ? <Trans>Close</Trans> : <Trans>Done</Trans>}
            </Button>
          )}
        </div>
      </div>
    </Frame>
  );
}

function OpenedLinkDialog({ target }: { target: PairingLinkTarget }) {
  const { t } = useLingui();
  const admin = useDeviceAdmin();
  const accept = useSyncAction((api, _: undefined) => api.acceptOpenedLink());
  const dismiss = useSyncAction((api, _: undefined) => api.dismissOpenedLink());
  const device = target.deviceId.slice(0, 8);
  const until = formatDateTime(target.expiresAtMs);
  const close = () => dismiss.mutate(undefined);
  return (
    <Frame label={t`Pair with the device from this link`} onClose={close}>
      <div
        data-part="link"
        data-status={target.expired ? "expired" : "valid"}
        className="space-y-4"
      >
        <Heading slot="title" className="font-display text-xl font-semibold">
          {accept.isSuccess ? (
            <Trans>Paired with {accept.data.name}</Trans>
          ) : (
            <Trans>Pair with the device from this link?</Trans>
          )}
        </Heading>
        {!accept.isSuccess && (
          <p className="text-sm text-fg-muted">
            {target.expired ? (
              <Trans>
                This pairing link has expired. Make a new one on the other device (it starts with{" "}
                {device}).
              </Trans>
            ) : (
              <Trans>
                A pairing link was opened. Pairing hands the device that made it (it starts with{" "}
                {device}) your library and accounts. The link works until {until}.
              </Trans>
            )}
          </p>
        )}
        {!admin && !accept.isSuccess && (
          <p className="text-sm text-fg-muted">
            <Trans>Only an admin can pair devices.</Trans>
          </p>
        )}
        {accept.error && <FailureNotice variant="inline" error={accept.error} />}
        <div className="flex justify-end gap-2">
          {accept.isSuccess ? (
            <Button variant="primary" data-focus-default onPress={close}>
              <Trans>Done</Trans>
            </Button>
          ) : (
            <>
              <Button data-focus-default={!admin || target.expired || undefined} onPress={close}>
                <Trans>Not now</Trans>
              </Button>
              {admin && !target.expired && (
                <Button
                  variant="primary"
                  data-focus-default
                  isDisabled={accept.isPending}
                  onPress={() => accept.mutate(undefined)}
                >
                  <Trans>Pair</Trans>
                </Button>
              )}
            </>
          )}
        </div>
      </div>
    </Frame>
  );
}
