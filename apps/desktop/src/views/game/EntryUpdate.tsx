import { Button, Switch } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Download, ExternalLink, RefreshCw } from "lucide-react";
import { useState } from "react";
import { formatDate } from "../../api/presentation";
import {
  formatVersion,
  type GameUpdate,
  gameUpdatesKey,
  useChangeGameUpdate,
  useGameUpdates,
  useGameUpdatesApi,
} from "../../api/updates";
import { activityKey, useFollowJob, useJobs } from "../activity/ActivityProvider";

/** Starts an install's update; resolves with its job's id. */
export function useInstallUpdate(gameId: string) {
  const api = useGameUpdatesApi();
  const client = useQueryClient();
  const follow = useFollowJob();
  return useMutation({
    mutationFn: (entryId: string) => api.install(entryId),
    onSuccess: (jobId) => {
      follow(jobId);
      void client.invalidateQueries({ queryKey: activityKey });
      void client.invalidateQueries({ queryKey: gameUpdatesKey(gameId) });
    },
  });
}

/** An update job as it runs: how far it got, and how it ended. */
export function UpdateProgress({ jobId }: { jobId: string }) {
  const { t } = useLingui();
  const job = useJobs().data?.find((each) => each.id === jobId);
  if (!job)
    return (
      <p className="text-fg-muted">
        <Trans>Starting the update…</Trans>
      </p>
    );
  if (job.status === "completed")
    return (
      <p className="text-accent-alt">
        <Trans>Updated.</Trans>
      </p>
    );
  if (job.status === "failed" || job.status === "cancelled") {
    const error = job.error;
    return (
      <p role="alert" className="text-danger">
        {job.status === "cancelled" ? t`The update was cancelled.` : t`Update failed: ${error}`}
      </p>
    );
  }
  if (job.status === "needs_user")
    return (
      <p className="text-fg-muted">
        <Trans>The update needs a choice in Activity.</Trans>
      </p>
    );
  const percent =
    job.progress?.total && job.progress.unit === "bytes"
      ? Math.floor((job.progress.completed / job.progress.total) * 100)
      : null;
  const downloading = job.phase === "download";
  return (
    <p role="status" className="text-fg-muted">
      {percent === null
        ? downloading
          ? t`Downloading the update…`
          : t`Installing the update…`
        : downloading
          ? t`Downloading the update… ${percent}%`
          : t`Installing the update… ${percent}%`}
    </p>
  );
}

function ReleaseLink({ update }: { update: GameUpdate }) {
  const href = update.available ? update.latestUrl : update.projectUrl;
  if (!href) return null;
  return (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      className="inline-flex items-center gap-1 text-accent-alt hover:underline"
    >
      {update.available ? <Trans>What's new</Trans> : <Trans>Releases</Trans>}
      <ExternalLink aria-hidden="true" className="size-3" />
    </a>
  );
}

/** An install's updates: the release installed, a newer one when there is,
 * and the controls to update, skip it or check now. Nothing for installs
 * no update provider follows. */
export function EntryUpdate({
  gameId,
  entryId,
  isRunning,
}: {
  gameId: string;
  entryId: string;
  isRunning: boolean;
}) {
  const { t } = useLingui();
  const update = useGameUpdates(gameId).data?.find((each) => each.entryId === entryId);
  const change = useChangeGameUpdate(gameId, entryId);
  const install = useInstallUpdate(gameId);
  const [jobId, setJobId] = useState<string | null>(null);
  if (!update) return null;
  const { installed, latest } = update;
  const latestVersion = latest ? formatVersion(latest) : "";
  const installedVersion = installed ? formatVersion(installed) : "";
  const checked = formatDate(new Date(update.checkedAtMs).toISOString());
  const state = [
    installed
      ? t({
          message: `${installedVersion} · up to date`,
          comment: "{installedVersion} is a version, e.g. v1.2",
        })
      : t`Not checked yet`,
    update.checkedAtMs > 0 &&
      t({
        message: `checked ${checked}`,
        comment: "When updates were last checked for; {checked} is a date",
      }),
  ]
    .filter(Boolean)
    .join(" · ");
  const lastError = update.error;
  const skipped = update.skipped ? formatVersion(update.skipped) : "";
  return (
    <div
      data-part="update"
      data-status={update.available ? "available" : "current"}
      className="mt-2 space-y-1.5 text-xs"
    >
      {update.available && latest ? (
        <p className="font-semibold text-accent-alt">
          {installed ? (
            <Trans comment="Versions, e.g. v1.2">
              {latestVersion} is out{" "}
              <span className="font-normal text-fg-muted">· you have {installedVersion}</span>
            </Trans>
          ) : (
            <Trans comment="A version, e.g. v1.2">{latestVersion} is out</Trans>
          )}
        </p>
      ) : (
        <p className="text-fg-muted">{state}</p>
      )}
      {lastError && (
        <p className="text-fg-muted">
          <Trans>The last check failed: {lastError}</Trans>
        </p>
      )}
      {update.skipped && update.skipped === latest && (
        <p className="text-fg-muted">
          <Trans>You skipped {skipped}.</Trans>
        </p>
      )}
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        {update.available && (
          <Button
            size="sm"
            variant="primary"
            icon={Download}
            isDisabled={isRunning || install.isPending || jobId !== null}
            onPress={() => install.mutate(entryId, { onSuccess: setJobId })}
          >
            <Trans comment="Button: installs the newer release">Update</Trans>
          </Button>
        )}
        {update.available && latest && (
          <Button
            size="sm"
            variant="ghost"
            isDisabled={change.isPending}
            onPress={() => change.mutate({ type: "skip", version: latest })}
          >
            <Trans>Skip this version</Trans>
          </Button>
        )}
        {update.skipped && update.skipped === latest && (
          <Button
            size="sm"
            variant="ghost"
            isDisabled={change.isPending}
            onPress={() => change.mutate({ type: "skip", version: null })}
          >
            <Trans>Offer it again</Trans>
          </Button>
        )}
        <Button
          size="sm"
          variant="ghost"
          icon={RefreshCw}
          isDisabled={change.isPending}
          onPress={() => change.mutate({ type: "check" })}
        >
          {change.isPending ? t`Checking…` : t`Check now`}
        </Button>
        <ReleaseLink update={update} />
        <Switch
          size="sm"
          isSelected={update.prereleases}
          isDisabled={change.isPending}
          onChange={(prereleases) => change.mutate({ type: "prereleases", prereleases })}
        >
          <Trans>Prereleases</Trans>
        </Switch>
      </div>
      {jobId && <UpdateProgress jobId={jobId} />}
      {(change.isError || install.isError) && (
        <p role="alert" className="text-danger">
          {(change.error ?? install.error)?.message}
        </p>
      )}
    </div>
  );
}
