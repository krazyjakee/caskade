/** Games moving between this user's devices (#233), and games shared with
 * other people and received from them (#246, #247). With nothing under way
 * or shared, the section stays out of sight. */
import { SettingsSection } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { ArrowDownToLine, ArrowUpFromLine, Share2 } from "lucide-react";
import { useTransfers } from "../../../api/sync";
import { SharesList, useHasShares } from "../../../components/shares/SharesList";
import { LinkBadge, TransferProgress } from "../../../components/sync/common";

export function CopiesAndShares() {
  const { t } = useLingui();
  const transfers = useTransfers().data ?? [];
  const shares = useHasShares();
  if (transfers.length === 0 && !shares) return null;
  return (
    <SettingsSection
      icon={Share2}
      title={t`Copies and shares`}
      description={t`Games copied between your devices, and games you shared. Both computers need Caskade open until a game arrives.`}
    >
      <ul className="flex flex-col gap-2 empty:hidden">
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
      </ul>
      <SharesList />
    </SettingsSection>
  );
}
