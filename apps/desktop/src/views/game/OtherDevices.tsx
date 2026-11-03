/** A game's copies on the user's other devices (#232): which device, how
 * it's reached, the size, why a copy couldn't come here when it can't, and
 * Copy to this computer (#233), an install job in Activity. Playing there
 * (#234) comes later. */
import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { Download } from "lucide-react";
import { formatSize } from "../../api/presentation";
import { type PeerCopy, usePeerCopies, useSyncAction } from "../../api/sync";
import { FailureNotice } from "../../components/FailureNotice";
import { DeviceIcon, LinkBadge, osNames } from "../../components/sync/common";

export function OtherDevices({
  gameId,
  primary = false,
}: {
  gameId: string;
  /** Nothing is installed here: copying is the page's main action. */
  primary?: boolean;
}) {
  const { data } = usePeerCopies(gameId);
  const copy = useSyncAction((api, deviceId: string) => api.copyHere(gameId, deviceId));
  if (!data || data.length === 0) return null;
  // The first copy that can come here is the one the page offers first.
  const best = data.find((peer) => peer.blocked === null && peer.link !== "offline");
  return (
    <section
      data-part="other-devices"
      aria-labelledby="other-devices-heading"
      className="flex flex-col gap-2"
    >
      <h3 id="other-devices-heading" className="text-sm font-semibold">
        <Trans>On your other devices</Trans>
      </h3>
      {data.map((peer) => (
        <PeerRow
          key={peer.deviceId}
          peer={peer}
          primary={primary && peer === best}
          copying={copy.isPending || (copy.isSuccess && copy.variables === peer.deviceId)}
          onCopy={() => copy.mutate(peer.deviceId)}
        />
      ))}
      {copy.isSuccess && (
        <p role="status" className="text-xs text-status-installed">
          <Trans>Copying to this computer. Follow it in Activity.</Trans>
        </p>
      )}
      {copy.error && <FailureNotice variant="inline" error={copy.error} />}
    </section>
  );
}

/** What to say about a copy, beside its size. */
export function usePeerNote() {
  const { t } = useLingui();
  return (peer: PeerCopy) => {
    const device = peer.deviceName;
    const system = osNames[peer.os] ?? peer.os;
    if (peer.playing) return t`Playing on ${device} now.`;
    if (peer.blocked === "launcherOnly")
      return t`It starts through its store's launcher there. Install it from the store here.`;
    if (peer.blocked === "otherSystem")
      return t`${device}'s copy is for ${system} and can't run here.`;
    if (peer.link === "offline") return t`${device} is out of reach.`;
    if (peer.throughProton) return t`A Windows game: it runs here through Proton.`;
    return null;
  };
}

function PeerRow({
  peer,
  primary,
  copying,
  onCopy,
}: {
  peer: PeerCopy;
  primary: boolean;
  copying: boolean;
  onCopy: () => void;
}) {
  const { t } = useLingui();
  const note = usePeerNote()(peer);
  // Game files go only over a direct connection: through a relay the copy
  // waits, and says so in Activity.
  const canCopy = peer.blocked === null && peer.link !== "offline";
  const device = peer.deviceName;
  const size = peer.sizeBytes === null ? null : formatSize(peer.sizeBytes);
  const details = [size, note].filter(Boolean).join(" · ");
  return (
    <div
      data-part="peer-copy"
      data-status={peer.link}
      className="flex flex-col gap-1 rounded-control border border-border p-3"
    >
      <span className="flex items-center gap-2 text-sm">
        <DeviceIcon kind={peer.kind} className="size-4 text-fg-muted" />
        <span className="flex-1">
          <Trans>On {device}</Trans>
        </span>
        <LinkBadge link={peer.link} />
      </span>
      {details && <span className="text-xs text-fg-muted">{details}</span>}
      {canCopy && (
        <div>
          <Button
            size="sm"
            variant={primary ? "primary" : "secondary"}
            data-focus-default={primary || undefined}
            icon={Download}
            isDisabled={copying}
            onPress={onCopy}
          >
            {t`Copy to this computer`}
          </Button>
        </div>
      )}
    </div>
  );
}
