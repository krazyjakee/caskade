/** Share with someone… (#247): makes a link to one installed game. Whoever
 * opens it in their Caskade gets the game straight from this computer (and
 * from others it reached first), never through a relay. Stopping the share
 * makes the link fail at once; copies already received stay theirs.
 * Console mode shows the link as a QR code, for a phone to pass on. */
import { Button, SegmentedControl } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { Copy, Link2 } from "lucide-react";
import { useRef, useState } from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import { formatSize } from "../../api/presentation";
import {
  type ShareExpiry,
  type ShareTarget,
  useShareAction,
  useShareList,
  useShareQr,
} from "../../api/shares";
import { useDefaultFocus } from "../../input";
import { writeClipboard } from "../../input/clipboard";
import { useMode } from "../../shell/console/mode";
import { FailureNotice } from "../FailureNotice";
import { ShareRecipients, ShareStateLine } from "./common";

export function ShareGameDialog({ target, close }: { target: ShareTarget; close: () => void }) {
  const { t } = useLingui();
  const title = target.title;
  const consoleMode = useMode().mode === "console";
  const [expiry, setExpiry] = useState<ShareExpiry>("week");
  const [copied, setCopied] = useState(false);
  const create = useShareAction((api, _: undefined) => api.share(target, expiry));
  const stop = useShareAction((api, id: string) => api.stop(id));
  const link = create.data?.link;
  // Make a link goes once there's a link: a pad lands on Copy.
  const copyRef = useRef<HTMLButtonElement>(null);
  useDefaultFocus(copyRef, Boolean(link), link);
  // The share as it is now: its files read, who received it.
  const share =
    useShareList().data?.find((s) => s.id === create.data?.share.id) ?? create.data?.share;
  const qr = useShareQr(consoleMode ? share?.id : undefined).data;
  const size = share?.size ? formatSize(share.size) : null;
  return (
    <ModalOverlay
      isOpen
      isDismissable
      onOpenChange={(open) => {
        if (!open) close();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="max-h-[90vh] w-full max-w-lg overflow-auto rounded-panel border border-border-strong bg-surface-raised p-6">
        <Dialog data-cs="share-game" className="space-y-4 outline-none">
          <Heading slot="title" className="font-display text-xl font-semibold">
            <Trans>Share {title}</Trans>
          </Heading>
          {!share || !link ? (
            <>
              <p className="text-sm text-fg-muted">
                {target.kind === "game" ? (
                  <Trans>
                    Caskade makes a link. Whoever opens it in their Caskade gets the game straight
                    from this computer, on this network or over the internet.
                  </Trans>
                ) : target.item.kind === "collection" ? (
                  <Trans>
                    Caskade makes a link. Whoever opens it in their Caskade sees the collection's
                    games and which of them they have. Nothing is downloaded.
                  </Trans>
                ) : (
                  <Trans>
                    Caskade makes a link. Whoever opens it in their Caskade gets it straight from
                    this computer, on this network or over the internet.
                  </Trans>
                )}
              </p>
              <div className="flex flex-col gap-2">
                <span id="share-expiry" className="text-sm font-medium">
                  <Trans>The link works</Trans>
                </span>
                <SegmentedControl<ShareExpiry>
                  aria-labelledby="share-expiry"
                  options={[
                    { id: "day", label: t`For a day` },
                    { id: "week", label: t`For a week` },
                    { id: "never", label: t`Until I stop sharing` },
                  ]}
                  value={expiry}
                  onChange={setExpiry}
                />
              </div>
              {create.error && <FailureNotice variant="inline" error={create.error} />}
              <div className="flex justify-end gap-2">
                <Button data-spatial="focusable" onPress={close}>
                  <Trans>Cancel</Trans>
                </Button>
                <Button
                  variant="primary"
                  data-spatial="focusable"
                  data-focus-default
                  icon={Link2}
                  isDisabled={create.isPending}
                  onPress={() => create.mutate(undefined)}
                >
                  <Trans>Make a link</Trans>
                </Button>
              </div>
            </>
          ) : (
            <>
              <p className="text-sm text-fg-muted">
                {size ? (
                  <Trans>
                    Send this link to whoever you're sharing {title} ({size}) with. They open it, or
                    paste it in Add game → Receive a shared game. Keep Caskade open until it
                    arrives.
                  </Trans>
                ) : (
                  <Trans>
                    Send this link to whoever you're sharing {title} with. They open it, or paste it
                    in Add game → Receive a shared game. Keep Caskade open until it arrives.
                  </Trans>
                )}
              </p>
              {qr && (
                <img
                  data-part="qr"
                  src={qr}
                  alt={t`The link as a QR code`}
                  className="mx-auto size-56 rounded-control bg-white p-2"
                />
              )}
              <div className="flex gap-2">
                <p
                  data-part="link"
                  className="min-w-0 flex-1 truncate rounded-control border border-border p-2 font-mono text-xs select-all"
                  title={link}
                >
                  {link}
                </p>
                <Button
                  ref={copyRef}
                  icon={Copy}
                  data-spatial="focusable"
                  data-focus-default
                  onPress={async () => {
                    const wrote = await writeClipboard(link);
                    setCopied(wrote === true);
                  }}
                >
                  {copied ? t`Copied` : t({ message: "Copy", context: "button" })}
                </Button>
              </div>
              <ShareStateLine share={share} />
              <ShareRecipients share={share} />
              {stop.error && <FailureNotice variant="inline" error={stop.error} />}
              <div className="flex justify-between gap-2">
                <Button
                  variant="danger"
                  data-spatial="focusable"
                  isDisabled={stop.isPending}
                  onPress={() => stop.mutate(share.id, { onSuccess: close })}
                >
                  <Trans>Stop sharing</Trans>
                </Button>
                <Button variant="primary" data-spatial="focusable" onPress={close}>
                  <Trans>Done</Trans>
                </Button>
              </div>
            </>
          )}
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}
