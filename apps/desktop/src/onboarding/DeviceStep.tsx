/** Set up from another device (#242): pair with a Caskade on this network
 * (or by a pairing link), then bring its library over in the first sync:
 * games, accounts with their PINs, collections, play history and the
 * settings that sync. This device's own folders, emulators and hardware
 * settings are asked for next, as on any first start. Nothing here needs
 * the internet. */
import { Button, TextField } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { CheckCircle2, Link2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { type PairedDevice, useSyncAction, useSyncStatus } from "../api/sync";
import { FailureNotice } from "../components/FailureNotice";
import { DeviceIcon } from "../components/sync/common";
import { StepHeading } from "./Onboarding";

export function DeviceStep({ onNext }: { onNext(): void }) {
  const { t } = useLingui();
  const status = useSyncStatus().data;
  const pair = useSyncAction((api, id: string) => api.pair(id));
  const accept = useSyncAction((api, link: string) => api.acceptPairingLink(link));
  const syncNow = useSyncAction((api, id: string) => api.syncNow(id));
  const [link, setLink] = useState("");
  // The first device paired here: a fresh install has no other.
  const source: PairedDevice | undefined = status?.devices[0];
  const asked = useRef<string | null>(null);
  useEffect(() => {
    if (source && asked.current !== source.id) {
      asked.current = source.id;
      syncNow.mutate(source.id);
    }
  }, [source, syncNow]);

  if (!status) return null;
  if (status.thisDevice.identity !== "ready")
    return (
      <section data-part="from-device">
        <StepHeading
          title={t`Set up from another device`}
          description={t`Getting this device ready to pair…`}
        />
      </section>
    );

  if (source) {
    const name = source.name;
    const done = source.lastSyncMs !== null && !source.syncing;
    return (
      <section data-part="from-device" data-status={done ? "synced" : "syncing"}>
        <StepHeading
          title={done ? t`Your library is here` : t`Bringing your library from ${name}`}
          description={
            done
              ? t`Games, accounts, collections and play history came from ${name}. Next, set up what belongs to this device: its folders, emulators and how it starts.`
              : t`This takes a moment on the same network, with or without the internet.`
          }
        />
        <div className="flex flex-col items-center gap-4">
          {done ? (
            <CheckCircle2 aria-hidden className="size-10 text-success" />
          ) : (
            <p role="status" className="text-sm text-fg-muted">
              <Trans>Syncing with {name}…</Trans>
            </p>
          )}
          {source.syncError && !source.syncing && (
            <p role="alert" className="text-sm text-danger">
              <Trans>The last try didn't reach {name}. It tries again in a moment.</Trans>
            </p>
          )}
          <Button
            variant="primary"
            data-focus-default
            data-spatial="focusable"
            isDisabled={!done}
            onPress={onNext}
          >
            <Trans>Continue</Trans>
          </Button>
        </div>
      </section>
    );
  }

  return (
    <section data-part="from-device" data-status="pairing">
      <StepHeading
        title={t`Set up from another device`}
        description={t`Open Caskade on the device you already use, on the same network. Pair with it here and compare the codes both show.`}
      />
      <div className="flex flex-col gap-6">
        <div>
          <h2 className="mb-2 text-sm font-semibold">
            <Trans>Nearby</Trans>
          </h2>
          {status.nearby.length === 0 ? (
            <p role="status" className="text-sm text-fg-muted">
              <Trans>Looking for Caskade on this network…</Trans>
            </p>
          ) : (
            <ul aria-label={t`Nearby devices`} className="flex flex-col gap-2">
              {status.nearby.map((device, index) => (
                <li
                  key={device.id}
                  className="flex items-center justify-between gap-3 rounded-control border border-border p-3"
                >
                  <span className="flex items-center gap-3">
                    <DeviceIcon kind={device.kind} className="size-5 text-fg-muted" />
                    <span className="font-medium">{device.name}</span>
                  </span>
                  <Button
                    data-spatial="focusable"
                    data-focus-default={index === 0 ? "" : undefined}
                    variant="primary"
                    isDisabled={pair.isPending}
                    onPress={() => pair.mutate(device.id)}
                  >
                    <Trans>Pair</Trans>
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </div>
        <div>
          <h2 className="mb-2 text-sm font-semibold">
            <Trans>Or use a pairing link</Trans>
          </h2>
          <p className="mb-2 text-sm text-fg-muted">
            <Trans>
              On the other device, open Settings → Devices and copy a pairing link, then paste it
              here.
            </Trans>
          </p>
          <div className="flex items-end gap-2">
            <TextField
              aria-label={t`Pairing link`}
              placeholder="caskade://pair?…"
              icon={Link2}
              value={link}
              onChange={setLink}
              className="flex-1"
            />
            <Button
              data-spatial="focusable"
              isDisabled={!link.trim() || accept.isPending}
              onPress={() => accept.mutate(link.trim())}
            >
              <Trans>Pair</Trans>
            </Button>
          </div>
        </div>
        {(pair.error ?? accept.error) && (
          <FailureNotice variant="inline" error={pair.error ?? accept.error} />
        )}
      </div>
    </section>
  );
}
