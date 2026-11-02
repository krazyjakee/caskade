/** Games moving between this user's devices (#233) and games shared with
 * other people (#246). Until those exist nothing is ever under way, and the
 * section stays out of sight rather than offering buttons that do nothing. */
import { SettingsSection } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { ArrowDownToLine, ArrowUpFromLine, Share2 } from "lucide-react";
import { formatDateTime, formatSize } from "../../../api/presentation";
import { useShares, useTransfers } from "../../../api/sync";
import { LinkBadge, TransferProgress } from "../../../components/sync/common";

export function CopiesAndShares() {
  const { t } = useLingui();
  const transfers = useTransfers().data ?? [];
  const shares = useShares().data ?? [];
  if (transfers.length === 0 && shares.length === 0) return null;
  return (
    <SettingsSection
      icon={Share2}
      title={t`Copies and shares`}
      description={t`Games copied between your devices, and games you shared. Both computers need Caskade open until a game arrives.`}
    >
      <ul className="flex flex-col gap-2">
        {transfers.map((transfer) => {
          const title = transfer.title;
          const device = transfer.deviceName;
          return (
            <li
              key={transfer.id}
              data-cs="transfer"
              data-status={transfer.state}
              className="flex flex-col gap-2 rounded-control border border-border p-3"
            >
              <span className="flex flex-wrap items-center gap-3 text-sm">
                {transfer.direction === "incoming" ? (
                  <ArrowDownToLine aria-hidden className="size-4 text-fg-muted" />
                ) : (
                  <ArrowUpFromLine aria-hidden className="size-4 text-fg-muted" />
                )}
                <span className="min-w-0 flex-1 font-medium">
                  {transfer.direction === "incoming" ? (
                    <Trans>
                      {title} from {device}
                    </Trans>
                  ) : (
                    <Trans>
                      {title} to {device}
                    </Trans>
                  )}
                </span>
                <LinkBadge link={transfer.link} />
              </span>
              <TransferProgress transfer={transfer} />
            </li>
          );
        })}
        {shares.map((share) => {
          const title = share.title;
          const size = formatSize(share.sizeBytes);
          const count = share.recipients;
          const expires = share.expiresAtMs === null ? null : formatDateTime(share.expiresAtMs);
          return (
            <li
              key={share.id}
              data-cs="share"
              data-status={share.expiresAtMs === null ? "open" : "expiring"}
              className="flex flex-wrap items-center gap-3 rounded-control border border-border p-3 text-sm"
            >
              <Share2 aria-hidden className="size-4 text-fg-muted" />
              <span className="min-w-0 flex-1 font-medium">{title}</span>
              <span className="text-xs text-fg-subtle">
                {expires ? (
                  <Trans>
                    {size} · received by {count} · until {expires}
                  </Trans>
                ) : (
                  <Trans>
                    {size} · received by {count}
                  </Trans>
                )}
              </span>
            </li>
          );
        })}
      </ul>
    </SettingsSection>
  );
}
