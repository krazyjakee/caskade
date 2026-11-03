/** Emulators the user's other devices use for a platform that this device
 * doesn't have (#275): a platform's preferred emulator syncs, and waits
 * here until its emulator is installed. Installing is one click, and only
 * when asked; Keep mine sets the other device's choice aside. */
import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { Download, MonitorSmartphone, RotateCcw } from "lucide-react";
import { setupProgress, setupRunning } from "../../../api/emulatorSetup";
import {
  useEmulatorOffers,
  useKeepEmulator,
  useSetupInstall,
  useSetupJobs,
} from "./EmulationProvider";

export function EmulatorsFromDevices() {
  const { t } = useLingui();
  const offers = useEmulatorOffers();
  const jobs = useSetupJobs();
  const install = useSetupInstall();
  const keep = useKeepEmulator();
  if (!offers.data?.length) return null;
  return (
    <section
      data-part="emulators-from-devices"
      aria-labelledby="emulators-from-devices-title"
      className="space-y-4 rounded-panel border border-border bg-surface-raised p-5"
    >
      <div className="flex items-start gap-3">
        <MonitorSmartphone className="mt-0.5 size-5 shrink-0 text-accent" />
        <div>
          <h3 id="emulators-from-devices-title" className="font-display font-semibold">
            <Trans>From your other devices</Trans>
          </h3>
          <p className="mt-1 text-sm text-fg-muted">
            <Trans>
              Your other devices play these platforms with an emulator this device doesn't have.
              Install it to use it here too, or keep this device's.
            </Trans>
          </p>
        </div>
      </div>
      <ul className="divide-y divide-border rounded-control border border-border">
        {offers.data.map((offer) => {
          const job = jobs.data?.filter((row) => row.definitionId === offer.definitionId).at(-1);
          const running = job && setupRunning(job);
          const emulator = offer.emulatorName;
          const platform = offer.platformName;
          const device = offer.deviceName;
          return (
            <li
              key={offer.platformId}
              data-part="emulator-offer"
              className="flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-3"
            >
              <div className="min-w-48 flex-1">
                <p className="font-medium">{platform}</p>
                <p className="text-xs text-fg-subtle">
                  {device
                    ? t`${device} uses ${emulator}`
                    : t`Another of your devices uses ${emulator}`}
                </p>
                {job && (
                  <p
                    role={job.status === "failed" ? "alert" : "status"}
                    className={
                      job.status === "failed" ? "text-xs text-danger" : "text-xs text-fg-muted"
                    }
                  >
                    {setupProgress(job)}
                  </p>
                )}
              </div>
              {offer.installable ? (
                <Button
                  variant="primary"
                  icon={job?.status === "failed" ? RotateCcw : Download}
                  isDisabled={running || install.isPending}
                  onPress={() => install.mutate({ definitionId: offer.definitionId })}
                >
                  {job?.status === "failed" ? t`Try again` : t`Install ${emulator}`}
                </Button>
              ) : (
                <p className="max-w-64 text-xs text-fg-muted">
                  <Trans>Caskade can't install {emulator} on this system.</Trans>
                </p>
              )}
              <Button
                variant="ghost"
                isDisabled={running || keep.isPending}
                onPress={() => keep.mutate(offer.platformId)}
                aria-label={t`Keep this device's emulator for ${platform}`}
              >
                <Trans>Keep mine</Trans>
              </Button>
            </li>
          );
        })}
      </ul>
      {(install.error ?? keep.error) && (
        <p role="alert" className="text-sm text-danger">
          {(install.error ?? keep.error)?.message}
        </p>
      )}
    </section>
  );
}
