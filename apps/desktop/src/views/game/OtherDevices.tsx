/** A game's copies on the user's other devices (#232): which device, how
 * it's reached, the size, and why a copy couldn't come here when it can't.
 * Copying here (#233) and playing there (#234) come later; until then the
 * panel only tells. */
import { Trans, useLingui } from "@lingui/react/macro";
import { formatSize } from "../../api/presentation";
import { type PeerCopy, usePeerCopies } from "../../api/sync";
import { DeviceIcon, LinkBadge, osNames } from "../../components/sync/common";

export function OtherDevices({ gameId }: { gameId: string }) {
  const { data } = usePeerCopies(gameId);
  if (!data || data.length === 0) return null;
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
        <PeerRow key={peer.deviceId} peer={peer} />
      ))}
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

function PeerRow({ peer }: { peer: PeerCopy }) {
  const note = usePeerNote()(peer);
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
    </div>
  );
}
