/** Settings → Devices → Copies and shares (#247): the games this device
 * shared, with who received them, Copy link, Stop sharing and Forget; and
 * the shares received here that arrive or help their other recipients,
 * with the switch to keep helping. */
import { Button, Switch } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { Copy, Share2, Trash2, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { formatSize } from "../../api/presentation";
import {
  type ReceivedShare,
  type ShareRecord,
  shareRunning,
  useReceivedShares,
  useShareAction,
  useShareList,
} from "../../api/shares";
import { writeClipboard } from "../../input/clipboard";
import { FailureNotice } from "../FailureNotice";
import { ShareRecipients, ShareStateLine } from "./common";

/** Whether there's anything to list. */
export function useHasShares(): boolean {
  const shares = useShareList().data ?? [];
  const received = useReceivedShares().data ?? [];
  return shares.length > 0 || received.length > 0;
}

export function SharesList() {
  const shares = useShareList().data ?? [];
  const received = useReceivedShares().data ?? [];
  if (shares.length === 0 && received.length === 0) return null;
  return (
    <ul data-part="shares" className="flex flex-col gap-2">
      {shares.map((share) => (
        <ShareRow key={share.id} share={share} />
      ))}
      {received.map((share) => (
        <ReceivedRow key={share.job} share={share} />
      ))}
    </ul>
  );
}

function ShareRow({ share }: { share: ShareRecord }) {
  const { t } = useLingui();
  const title = share.title;
  const size = share.size === null ? null : formatSize(share.size);
  const running = shareRunning(share);
  const [copied, setCopied] = useState(false);
  const copy = useShareAction(async (api, id: string) => {
    const link = await api.link(id);
    const wrote = await writeClipboard(link);
    setCopied(wrote === true);
  });
  const stop = useShareAction((api, id: string) => api.stop(id));
  // Stop sharing goes once it's stopped: focus moves to Forget in its place.
  const [stopping, setStopping] = useState(false);
  const forgetRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (stopping && !running) forgetRef.current?.focus();
  }, [stopping, running]);
  const forget = useShareAction((api, id: string) => api.forget(id));
  const rowRef = useRef<HTMLLIElement>(null);
  // Forgotten, the row goes: focus moves to the next row (or the one before).
  const forgetAndMove = () => {
    const row = rowRef.current;
    const neighbour = row?.nextElementSibling ?? row?.previousElementSibling;
    forget.mutate(share.id, {
      onSuccess: () =>
        requestAnimationFrame(() =>
          neighbour?.querySelector<HTMLElement>("button:not([disabled]), input")?.focus(),
        ),
    });
  };
  const error = copy.error ?? stop.error ?? forget.error;
  return (
    <li
      ref={rowRef}
      data-cs="share"
      data-status={share.state.state}
      className="flex flex-col gap-2 rounded-control border border-border p-3"
    >
      <span className="flex flex-wrap items-center gap-3 text-sm">
        <Share2 aria-hidden className="size-4 text-fg-muted" />
        <span className="min-w-0 flex-1 font-medium">
          {size ? (
            <Trans>
              {title} · {size}
            </Trans>
          ) : (
            title
          )}
        </span>
        {running ? (
          <>
            <Button
              size="sm"
              icon={Copy}
              data-spatial="focusable"
              isDisabled={copy.isPending}
              onPress={() => copy.mutate(share.id)}
            >
              {copied ? t`Link copied` : t`Copy link`}
            </Button>
            <Button
              size="sm"
              variant="danger"
              icon={X}
              data-spatial="focusable"
              isDisabled={stop.isPending}
              onPress={() => {
                setStopping(true);
                stop.mutate(share.id, { onError: () => setStopping(false) });
              }}
            >
              <Trans>Stop sharing</Trans>
            </Button>
          </>
        ) : (
          <Button
            ref={forgetRef}
            size="sm"
            icon={Trash2}
            data-spatial="focusable"
            isDisabled={forget.isPending}
            onPress={forgetAndMove}
          >
            <Trans>Forget</Trans>
          </Button>
        )}
      </span>
      <ShareStateLine share={share} />
      {share.recipients.length > 0 && <ShareRecipients share={share} />}
      {error && <FailureNotice variant="inline" error={error} />}
    </li>
  );
}

function ReceivedRow({ share }: { share: ReceivedShare }) {
  const { t } = useLingui();
  const title = share.title;
  const sharer = share.sharer;
  const helping = share.helping;
  const keep = useShareAction((api, on: boolean) => api.keepHelping(share.job, on));
  // It can help after it's here only if that was chosen while it arrived.
  const canChange = share.receiving || share.serving;
  return (
    <li
      data-cs="received-share"
      data-status={share.receiving ? "receiving" : share.serving ? "serving" : "done"}
      className="flex flex-col gap-2 rounded-control border border-border p-3"
    >
      <span className="flex flex-wrap items-center gap-3 text-sm">
        <Share2 aria-hidden className="size-4 text-fg-muted" />
        <span className="min-w-0 flex-1 font-medium">
          <Trans>
            {title} from {sharer}
          </Trans>
        </span>
        <Switch
          size="sm"
          isSelected={share.keepHelping}
          isDisabled={!canChange || keep.isPending}
          onChange={(on) => keep.mutate(on)}
          data-spatial="focusable"
        >
          {t`Help others receive it`}
        </Switch>
      </span>
      <p role="status" className="text-xs text-fg-muted">
        {share.serving ? (
          <Trans>Helping {helping} others receive it.</Trans>
        ) : share.receiving ? (
          <Trans>Arriving: follow it in Activity.</Trans>
        ) : (
          <Trans>Here.</Trans>
        )}
      </p>
      {keep.error && <FailureNotice variant="inline" error={keep.error} />}
    </li>
  );
}
