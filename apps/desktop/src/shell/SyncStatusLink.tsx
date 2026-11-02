import { Plural, useLingui } from "@lingui/react/macro";
import { Link } from "@tanstack/react-router";
import { LoaderCircle } from "lucide-react";
import { reachable, useProvidedSyncApi, useSyncStatus } from "../api/sync";
import { LinkBadge } from "../components/sync/common";

/** The status bar's sync entry (#231): how many paired devices are in reach
 * and the best way one is reached, or that they're out of reach; to
 * Settings → Devices. Nothing without paired devices. */
export function SyncStatusLink() {
  return useProvidedSyncApi() ? <SyncStatus /> : null;
}

function SyncStatus() {
  const { t } = useLingui();
  const { data } = useSyncStatus();
  const devices = data?.devices ?? [];
  if (devices.length === 0) return null;
  const inReach = reachable(devices);
  const syncing = devices.some((device) => device.syncing);
  const best = inReach[0]?.link ?? "offline";
  const count = inReach.length;
  return (
    <Link
      to="/settings"
      search={{ tab: "devices" }}
      data-part="sync"
      data-status={syncing ? "syncing" : best}
      aria-label={t`Devices: open Settings → Devices`}
      className="flex shrink-0 items-center gap-1.5 rounded-sm hover:text-fg focus-visible:focus-ring"
    >
      {syncing && (
        <LoaderCircle
          aria-hidden="true"
          style={{ animationPlayState: "var(--cs-motion-ambient, running)" }}
          className="size-3.5 animate-worker text-accent-alt motion-reduce:animate-none"
        />
      )}
      <LinkBadge link={best} />
      <span>
        {count === 0 ? (
          t`Devices out of reach`
        ) : (
          <Plural value={count} one="# device in reach" other="# devices in reach" />
        )}
      </span>
    </Link>
  );
}
