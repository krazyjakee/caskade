/** Pieces the share views share: who received a share and how far each got,
 * and what a share is doing. */
import { Trans, useLingui } from "@lingui/react/macro";
import { ProgressBar } from "react-aria-components";
import { formatDateTime, formatSize } from "../../api/presentation";
import type { ShareRecipient, ShareRecord } from "../../api/shares";

/** Who opened a share's link, and how far each has got. */
export function ShareRecipients({ share }: { share: ShareRecord }) {
  if (share.recipients.length === 0)
    return (
      <p role="status" className="text-xs text-fg-subtle">
        <Trans>Nobody has opened the link yet.</Trans>
      </p>
    );
  return (
    <ul data-part="recipients" className="flex flex-col gap-2">
      {share.recipients.map((recipient) => (
        <Recipient key={recipient.name} recipient={recipient} size={share.size} />
      ))}
    </ul>
  );
}

function Recipient({ recipient, size }: { recipient: ShareRecipient; size: number | null }) {
  const { t } = useLingui();
  const name = recipient.name;
  const total = size ?? 0;
  const ratio = recipient.finished ? 1 : total > 0 ? Math.min(recipient.done / total, 1) : 0;
  const done = formatSize(recipient.done);
  const all = formatSize(total);
  const note = recipient.finished
    ? t`Received`
    : recipient.connected
      ? total > 0
        ? t`${done} of ${all}`
        : done
      : t`Waiting for them to come back`;
  return (
    <li
      data-cs="share-recipient"
      data-status={recipient.finished ? "done" : recipient.connected ? "receiving" : "away"}
      className="flex flex-col gap-1.5"
    >
      <span className="flex items-center gap-3 text-xs">
        <span className="flex-1 font-medium">{name}</span>
        {recipient.serving && (
          <span className="text-fg-subtle">
            <Trans>Helping others receive it</Trans>
          </span>
        )}
      </span>
      <ProgressBar
        aria-label={t`How far ${name} got`}
        value={ratio * 100}
        className="flex flex-col gap-1"
      >
        <div className="h-1.5 overflow-hidden rounded-full bg-surface-overlay">
          <div
            className={`h-full rounded-full ${recipient.finished ? "bg-status-installed" : "bg-accent-alt"}`}
            style={{ width: `${ratio * 100}%` }}
          />
        </div>
        <span className="text-xs text-fg-muted">{note}</span>
      </ProgressBar>
    </li>
  );
}

/** What a share does now: reading its files, its link working until when,
 * or stopped. */
export function ShareStateLine({ share }: { share: ShareRecord }) {
  const { t } = useLingui();
  const state = share.state;
  const until = share.expiresAtMs === null ? null : formatDateTime(share.expiresAtMs);
  let text: string;
  switch (state.state) {
    case "preparing": {
      const done = formatSize(state.done);
      const total = formatSize(state.total);
      text =
        state.total > 0
          ? t`Getting the files ready: ${done} of ${total}. The link works already.`
          : t`Getting the files ready. The link works already.`;
      break;
    }
    case "ready":
      text = until ? t`The link works until ${until}.` : t`The link works until you stop sharing.`;
      break;
    case "failed": {
      const reason = state.reason;
      text = t`The game's files couldn't be read: ${reason}`;
      break;
    }
    case "stopped":
      text = t`Stopped: the link doesn't work any more.`;
      break;
    case "expired":
      text = t`Expired: the link doesn't work any more.`;
      break;
  }
  return (
    <p
      data-part="state"
      data-status={state.state}
      role="status"
      className={`text-xs ${state.state === "failed" ? "text-danger" : "text-fg-muted"}`}
    >
      {text}
    </p>
  );
}
