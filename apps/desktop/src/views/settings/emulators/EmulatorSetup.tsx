import { Button, Pill, SettingsSection, StatusDot } from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg, plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { CloudDownload, Download, RefreshCw, RotateCcw, TriangleAlert, X } from "lucide-react";
import type { EmulationSnapshot } from "../../../api/emulation";
import {
  type Installable,
  type SetupJob,
  setupProgress,
  setupRunning,
} from "../../../api/emulatorSetup";
import { formatDateTime } from "../../../api/presentation";
import {
  emulationKey,
  useEmulationApi,
  useInstallable,
  useManagedAction,
  useMissingEmulators,
  useSetupInstall,
  useSetupJobs,
} from "./EmulationProvider";

/** The newest install of an emulator, or for a platform. */
function latestJob(jobs: SetupJob[] | undefined, match: (job: SetupJob) => boolean) {
  return jobs?.filter(match).at(-1);
}

/** A job's progress, its warnings (a BIOS to add) and its error. */
function JobStatus({ job }: { job: SetupJob }) {
  const { t } = useLingui();
  const { setup } = useEmulationApi();
  const files = job.fetchedBios.join(", ");
  return (
    <div data-part="setup-status" data-status={job.status} className="space-y-1 text-xs">
      <div className="flex items-center gap-2">
        {setupRunning(job) ? (
          <span role="status" className="text-fg-muted">
            {setupProgress(job)}
          </span>
        ) : job.status === "failed" ? (
          <span role="alert" className="text-danger">
            {setupProgress(job)}
          </span>
        ) : (
          <StatusDot tone={job.status === "done" ? "ok" : "off"}>{setupProgress(job)}</StatusDot>
        )}
        {setupRunning(job) && setup && (
          <Button
            size="sm"
            variant="ghost"
            icon={X}
            aria-label={t`Cancel installing ${{ name: job.name }}`}
            onPress={() =>
              setup.cancel(job.id).catch((error: unknown) => console.error("Cancel failed", error))
            }
          >
            <Trans context="emulator install">Cancel</Trans>
          </Button>
        )}
      </div>
      {job.status === "done" && job.fetchedBios.length > 0 && (
        <p className="text-fg-muted">
          <Trans comment="BIOS files downloaded from the user's RomM server">
            Got {files} from RomM.
          </Trans>
        </p>
      )}
      {job.status === "done" &&
        job.warnings.map((warning) => (
          <p key={warning} className="text-warning">
            {warning}
          </p>
        ))}
    </div>
  );
}

/** Platforms the user has games for that no emulator runs (EMULATOR-SETUP-PLAN §7). */
export function MissingEmulators() {
  const { t } = useLingui();
  const missing = useMissingEmulators();
  const jobs = useSetupJobs();
  const install = useSetupInstall();
  if (missing.isError)
    return (
      <p data-part="missing-emulators" role="alert" className="text-sm text-danger">
        {t`Couldn't check which platforms need an emulator: ${{ error: missing.error.message }}`}
      </p>
    );
  if (!missing.data?.length) return null;
  return (
    <section
      data-part="missing-emulators"
      aria-labelledby="missing-emulators-title"
      className="space-y-4 rounded-panel border border-warning/40 bg-warning/5 p-5"
    >
      <div className="flex items-start gap-3">
        <TriangleAlert className="mt-0.5 size-5 shrink-0 text-warning" />
        <div>
          <h3 id="missing-emulators-title" className="font-display font-semibold">
            {plural(
              { platforms: missing.data.length },
              { one: "# platform has no emulator", other: "# platforms have no emulator" },
            )}
          </h3>
          <p className="mt-1 text-sm text-fg-muted">
            <Trans>
              You have games for these, but nothing to play them with. Caskade can download, install
              and set up an emulator for each in one click.
            </Trans>
          </p>
        </div>
      </div>
      <ul className="divide-y divide-border rounded-control border border-border bg-surface-raised">
        {missing.data.map((platform) => {
          const job = latestJob(jobs.data, (row) => row.platformId === platform.platformId);
          const running = job && setupRunning(job);
          return (
            <li
              key={platform.platformId}
              data-part="missing-platform"
              className="flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-3"
            >
              <div className="min-w-48 flex-1">
                <p className="font-medium">{platform.name}</p>
                <p className="text-xs text-fg-muted">
                  {plural({ total: platform.games }, { one: "# game", other: "# games" })}
                  {platform.recommended &&
                    ` · ${t({
                      message: `Recommended: ${{ name: platform.recommended.name }}`,
                      comment: "The emulator Caskade recommends for a platform",
                    })}`}
                </p>
                {job && <JobStatus job={job} />}
              </div>
              {platform.recommended ? (
                <Button
                  variant="primary"
                  icon={job?.status === "failed" ? RotateCcw : Download}
                  isDisabled={running || install.isPending}
                  onPress={() => install.mutate({ platformId: platform.platformId })}
                >
                  {job?.status === "failed"
                    ? t`Try again`
                    : t`Install ${{ name: platform.recommended.name }}`}
                </Button>
              ) : (
                <p className="max-w-80 text-xs text-fg-muted">
                  {t`Caskade can't install an emulator for ${{ platform: platform.short }} on this system. Add one you have installed with Add emulator.`}
                </p>
              )}
            </li>
          );
        })}
      </ul>
      {install.error && (
        <p role="alert" className="text-sm text-danger">
          {install.error.message}
        </p>
      )}
    </section>
  );
}

type Source = { label: MessageDescriptor; tone: "accent" | "warning" };
const downloadOnly: Source = {
  label: msg({ message: "Download only", comment: "How an emulator Caskade installs is updated" }),
  tone: "warning",
};
const automatic: Source = {
  label: msg({
    message: "Updates automatically",
    comment: "How an emulator Caskade installs is updated",
  }),
  tone: "accent",
};
const sources: Record<string, Source> = {
  github: automatic,
  dolphin: automatic,
  flatpak: {
    label: msg({
      message: "Updated by Flatpak",
      comment: "How an emulator Caskade installs is updated",
    }),
    tone: "accent",
  },
  url: downloadOnly,
};

function EmulatorCard({ item, snapshot }: { item: Installable; snapshot: EmulationSnapshot }) {
  const { t } = useLingui();
  const jobs = useSetupJobs();
  const install = useSetupInstall();
  const job = latestJob(jobs.data, (row) => row.definitionId === item.definitionId);
  const running = job && setupRunning(job);
  const source = sources[item.source] ?? downloadOnly;
  const platforms = item.platforms.map(
    (id) => snapshot.platforms.find((platform) => platform.id === id)?.short ?? id,
  );
  return (
    <li
      data-part="emulator-card"
      data-installed={item.managed ? "" : undefined}
      className="flex flex-col gap-3 rounded-control border border-border bg-surface-overlay p-4"
    >
      <div className="flex items-start justify-between gap-3">
        <div>
          <p className="font-display font-semibold">{item.name}</p>
          <p className="mt-1 text-xs text-fg-muted">{platforms.join(", ")}</p>
        </div>
        <Pill tone={source.tone} icon={item.updates ? RefreshCw : CloudDownload}>
          {t(source.label)}
        </Pill>
      </div>
      {job && (running || !item.managed || job.status !== "done" || job.warnings.length) ? (
        <JobStatus job={job} />
      ) : (
        item.managed && (
          <StatusDot tone="ok">
            {item.installedLabel
              ? t({
                  message: `Installed ${{ version: item.installedLabel }}`,
                  comment: "An emulator's installed version",
                })
              : t({ message: "Installed", context: "emulator" })}
          </StatusDot>
        )
      )}
      <div className="mt-auto flex flex-wrap gap-2">
        {item.managed && item.updateAvailable && !running ? (
          <ManagedUpdate item={item} />
        ) : (
          <Button
            size="sm"
            variant={item.managed ? "secondary" : "primary"}
            icon={item.managed ? RotateCcw : Download}
            isDisabled={running || install.isPending}
            aria-label={
              item.managed ? t`Reinstall ${{ name: item.name }}` : t`Install ${{ name: item.name }}`
            }
            onPress={() => install.mutate({ definitionId: item.definitionId })}
          >
            {item.managed
              ? t({ message: "Reinstall", context: "emulator" })
              : t({ message: "Install", context: "emulator" })}
          </Button>
        )}
      </div>
    </li>
  );
}

/** Update and Skip for a newer version of a managed emulator. */
function ManagedUpdate({ item }: { item: Installable }) {
  const { t } = useLingui();
  const action = useManagedAction();
  const emulatorId = item.managed?.emulatorId;
  const version = item.latestLabel;
  if (!emulatorId) return null;
  return (
    <>
      <Button
        size="sm"
        variant="primary"
        icon={Download}
        isDisabled={action.isPending}
        aria-label={t`Update ${{ name: item.name }} to ${{ version: item.latestLabel }}`}
        onPress={() => action.mutate({ action: "update", emulatorId })}
      >
        <Trans comment="Updates an emulator to the named version">Update to {version}</Trans>
      </Button>
      <Button
        size="sm"
        variant="ghost"
        isDisabled={action.isPending}
        aria-label={t({
          message: `Skip ${{ name: item.name }} ${{ version: item.latestLabel }}`,
          comment: "Skips this update of an emulator",
        })}
        onPress={() => action.mutate({ action: "skip", emulatorId })}
      >
        <Trans comment="Skips an emulator update">Skip</Trans>
      </Button>
      {action.error && (
        <p role="alert" className="basis-full text-xs text-danger">
          {action.error.message}
        </p>
      )}
    </>
  );
}

/** In the emulator editor: what Caskade installed, and its updates. */
export function ManagedEmulatorInfo({ emulatorId }: { emulatorId: string }) {
  const { t } = useLingui();
  const installable = useInstallable();
  const jobs = useSetupJobs();
  const action = useManagedAction();
  const item = installable.data?.find((row) => row.managed?.emulatorId === emulatorId);
  if (!item?.managed) return null;
  const job = latestJob(jobs.data, (row) => row.definitionId === item.definitionId);
  const running = job && setupRunning(job);
  const checked = item.managed.checkedAtMs ? formatDateTime(item.managed.checkedAtMs) : null;
  return (
    <section
      data-part="managed-emulator"
      aria-label={t`Installed by Caskade`}
      className="flex flex-wrap items-center gap-x-4 gap-y-2 rounded-panel border border-border bg-surface-raised px-5 py-4"
    >
      <CloudDownload className="size-5 text-accent-alt" />
      <div className="min-w-48 flex-1 text-sm">
        <p className="font-medium">
          {t`Installed by Caskade`}
          {item.installedLabel && ` · ${item.installedLabel}`}
        </p>
        <p className="text-xs text-fg-muted">
          {!item.updates
            ? item.source === "flatpak"
              ? t`Flatpak keeps it up to date.`
              : t`Download only: Reinstall gets the newest build.`
            : item.updateAvailable
              ? t({
                  message: `${{ version: item.latestLabel }} is available.`,
                  comment: "A newer version of an emulator",
                })
              : item.managed.error
                ? t`The last update check failed: ${{ error: item.managed.error }}`
                : checked
                  ? t`Up to date (checked ${{ checked }}).`
                  : t`Not checked for updates yet.`}
        </p>
        {job && (running || job.status === "failed") && <JobStatus job={job} />}
      </div>
      {item.updateAvailable && !running ? (
        <div className="flex gap-2">
          <ManagedUpdate item={item} />
        </div>
      ) : (
        item.updates && (
          <Button
            size="sm"
            variant="ghost"
            icon={RefreshCw}
            isDisabled={action.isPending || running}
            onPress={() => action.mutate({ action: "check", emulatorId })}
          >
            {action.isPending ? t`Checking…` : t`Check for updates`}
          </Button>
        )
      )}
    </section>
  );
}

/** "Get from RomM" for a platform's missing BIOS files (EMULATOR-SETUP-PLAN §5). */
export function FetchBios({ platformId }: { platformId: string }) {
  const { t } = useLingui();
  const { setup } = useEmulationApi();
  const client = useQueryClient();
  const fetch = useMutation({
    mutationFn: () => {
      if (!setup) return Promise.reject(new Error(t`Caskade can't fetch BIOS files here.`));
      return setup.fetchBios(platformId);
    },
    onSuccess: (result) => {
      if (result.fetched.length) void client.invalidateQueries({ queryKey: emulationKey });
    },
  });
  if (!setup) return null;
  const result = fetch.data;
  return (
    <div className="space-y-1">
      <Button
        size="sm"
        icon={CloudDownload}
        isDisabled={fetch.isPending}
        onPress={() => fetch.mutate()}
      >
        {fetch.isPending ? t`Looking on RomM…` : t`Get from RomM`}
      </Button>
      {fetch.error && (
        <p role="alert" className="text-xs text-danger">
          {fetch.error.message}
        </p>
      )}
      {result && (
        <p role="status" className="text-xs text-fg-muted">
          {result.fetched.length
            ? t({
                message: `Got ${{ files: result.fetched.join(", ") }}.`,
                comment: "BIOS files downloaded from RomM",
              })
            : t`No RomM server has them.`}
          {result.missing.length > 0 &&
            ` ${
              result.dir
                ? t({
                    message: `Still missing: ${{ files: result.missing.join(", ") }}. Put them in ${{ dir: result.dir }}.`,
                    comment: "BIOS files; the folder to put them in",
                  })
                : t({
                    message: `Still missing: ${{ files: result.missing.join(", ") }}.`,
                    comment: "BIOS files",
                  })
            }`}
        </p>
      )}
    </div>
  );
}

/** Every emulator Caskade can install here (EMULATOR-SETUP-PLAN §7). */
export function BrowseEmulators({ snapshot }: { snapshot: EmulationSnapshot }) {
  const { t } = useLingui();
  const installable = useInstallable();
  const install = useSetupInstall();
  return (
    <SettingsSection
      icon={CloudDownload}
      title={t`Get emulators`}
      description={t`Caskade downloads these for you, sets them up and makes them the default for platforms that have none. Emulators from GitHub are kept up to date.`}
    >
      {installable.error && (
        <p role="alert" className="mt-3 text-sm text-danger">
          {installable.error.message}
        </p>
      )}
      {install.error && (
        <p role="alert" className="mt-3 text-sm text-danger">
          {install.error.message}
        </p>
      )}
      {installable.isPending ? (
        <p role="status" className="mt-4 text-sm text-fg-muted">
          <Trans>Loading emulators…</Trans>
        </p>
      ) : !installable.data?.length ? (
        <p className="mt-4 text-sm text-fg-muted">
          <Trans>Caskade can't install emulators on this system yet.</Trans>
        </p>
      ) : (
        <ul
          aria-label={t`Emulators to install`}
          className="mt-4 grid gap-3 sm:grid-cols-2 xl:grid-cols-3"
        >
          {installable.data.map((item) => (
            <EmulatorCard key={item.definitionId} item={item} snapshot={snapshot} />
          ))}
        </ul>
      )}
    </SettingsSection>
  );
}
