/** Pieces every sync view shares: how a device is reached, its icon, and
 * a transfer's progress. */
import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";
import { useLingui } from "@lingui/react/macro";
import {
  Gamepad2,
  Globe,
  Laptop,
  type LucideIcon,
  Monitor,
  Moon,
  Network,
  RadioTower,
  Tv,
} from "lucide-react";
import { ProgressBar } from "react-aria-components";
import { formatSize } from "../../api/presentation";
import type { DeviceKind, DeviceOs, PeerLink, Transfer } from "../../api/sync";

const links: Record<PeerLink, { label: MessageDescriptor; icon: LucideIcon; tone: string }> = {
  lan: { label: msg`Local network`, icon: Network, tone: "text-status-installed" },
  direct: { label: msg`Internet, direct`, icon: Globe, tone: "text-accent-alt" },
  relay: { label: msg`Through a relay`, icon: RadioTower, tone: "text-warning" },
  offline: { label: msg`Offline`, icon: Moon, tone: "text-fg-subtle" },
};

/** How a device is reached: the local network, the internet, a relay, or
 * not at all. */
export function LinkBadge({ link }: { link: PeerLink }) {
  const { t } = useLingui();
  const { label, icon: Icon, tone } = links[link];
  return (
    <span
      data-cs="peer-link"
      data-status={link}
      className={`inline-flex items-center gap-1.5 text-xs whitespace-nowrap ${tone}`}
    >
      <Icon aria-hidden className="size-3.5" />
      {t(label)}
    </span>
  );
}

const kinds: Record<DeviceKind, LucideIcon> = {
  desktop: Monitor,
  laptop: Laptop,
  handheld: Gamepad2,
  tv: Tv,
};

export function DeviceIcon({
  kind,
  className = "size-5",
}: {
  kind: DeviceKind;
  className?: string;
}) {
  const Icon = kinds[kind];
  return <Icon aria-hidden className={className} />;
}

export const osNames: Record<DeviceOs, string> = {
  linux: "Linux",
  windows: "Windows",
  mac: "macOS",
};

/** A transfer's bar and what it is doing. */
export function TransferProgress({ transfer }: { transfer: Transfer }) {
  const { t } = useLingui();
  const ratio = transfer.totalBytes > 0 ? transfer.doneBytes / transfer.totalBytes : 0;
  const done = formatSize(transfer.doneBytes);
  const total = formatSize(transfer.totalBytes);
  const speed = formatSize(transfer.bytesPerSecond);
  const note =
    transfer.state === "waitingDirect"
      ? t`Waiting for a direct connection. Game files don't go through relays.`
      : transfer.state === "done"
        ? t`Done: ${total}`
        : transfer.state === "failed"
          ? t`Stopped`
          : t`${done} of ${total} · ${speed}/s`;
  return (
    <ProgressBar
      aria-label={t`Transfer progress`}
      value={ratio * 100}
      isIndeterminate={transfer.state === "waitingDirect"}
      className="flex flex-col gap-1.5"
    >
      <div className="h-1.5 overflow-hidden rounded-full bg-surface-overlay">
        <div
          className={`h-full rounded-full ${transfer.state === "done" ? "bg-status-installed" : "bg-accent-alt"}`}
          style={{ width: transfer.state === "waitingDirect" ? "0%" : `${ratio * 100}%` }}
        />
      </div>
      <span className="text-xs text-fg-muted">{note}</span>
    </ProgressBar>
  );
}
