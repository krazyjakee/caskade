import { Button } from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg, plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { ListChecks, Play, RotateCcw, Trash2, X } from "lucide-react";
import { useId } from "react";
import { Disclosure, DisclosurePanel, Heading } from "react-aria-components";
import { canRemove, canRetry, isActiveJob, type JobSnapshot } from "../../api/install";
import { formatSize } from "../../api/presentation";
import { useActionMenus } from "../../components/ActionMenu";
import { JobFailurePanel } from "./JobRecovery";
import { jobKind, jobPhase } from "./jobLabels";
import { ProgressLine } from "./ProgressLine";

const statusLabels: Record<JobSnapshot["status"], MessageDescriptor> = {
  queued: msg({ message: "Queued", comment: "A background job's status" }),
  running: msg({ message: "In progress", comment: "A background job's status" }),
  needs_user: msg({ message: "Needs your attention", comment: "A background job's status" }),
  paused: msg({
    message: "Paused",
    comment: "A download the user paused; it goes on from where it stopped when resumed",
  }),
  completed: msg({ message: "Completed", comment: "A background job's status" }),
  failed: msg({ message: "Failed", comment: "A background job's status" }),
  cancelled: msg({ message: "Cancelled", context: "job status" }),
};
export function JobCard({
  job,
  busy,
  cancel,
  remove,
  retry,
  resume,
  openQuestion,
}: {
  job: JobSnapshot;
  busy: boolean;
  cancel: () => void;
  /** Takes a failed or cancelled job out of Activity. */
  remove: () => void;
  /** Without a strategy, the job runs again as it ran before. */
  retry: (strategyId?: string) => void;
  /** Goes on with a paused download (#131). */
  resume?: () => void;
  openQuestion: () => void;
}) {
  const { t } = useLingui();
  const targetDir = job.targetDir;
  const heading = useId();
  // A right click offers the card's buttons.
  const menus = useActionMenus();
  const ended = job.status === "failed" || job.status === "cancelled";
  // A classified failure brings its own recovery buttons (#158).
  const failure = job.status === "failed" ? job.failure : undefined;
  const progress = job.progress;
  const completed = Math.max(0, progress?.completed ?? 0);
  const amount =
    progress?.unit === "bytes"
      ? formatSize(completed)
      : plural(completed, { one: "# file", other: "# files" });
  const title = job.title;
  const resumeLabel = t({
    message: "Resume",
    comment: "Goes on with a paused download from where it stopped",
  });
  const removeLabel = t({
    message: "Remove",
    comment: "Takes a failed or cancelled job out of Activity; its files stay",
  });
  return (
    <article
      {...menus.target(job.title, [
        job.status === "needs_user" &&
          job.question && {
            id: "review",
            label: t`Review choices`,
            icon: ListChecks,
            onAction: openQuestion,
          },
        job.status === "paused" &&
          resume && {
            id: "resume",
            label: resumeLabel,
            icon: Play,
            isDisabled: busy,
            onAction: resume,
          },
        isActiveJob(job) && {
          id: "cancel",
          label: t`Cancel job`,
          icon: X,
          tone: "danger" as const,
          isDisabled: busy,
          onAction: cancel,
        },
        canRetry(job) && {
          id: "retry",
          label: t({ message: "Retry", context: "job" }),
          icon: RotateCcw,
          isDisabled: busy,
          onAction: () => retry(),
        },
        ...(ended ? job.strategies : []).map(({ id, label: strategyLabel }) => ({
          id: `retry:${id}`,
          label: t({
            message: `Retry with ${strategyLabel}`,
            comment:
              "Runs a failed job again with another install strategy, named by the installer",
          }),
          icon: RotateCcw,
          isDisabled: busy,
          onAction: () => retry(id),
        })),
        canRemove(job) && {
          id: "remove",
          label: removeLabel,
          icon: Trash2,
          isDisabled: busy,
          onAction: remove,
        },
      ])}
      aria-labelledby={heading}
      data-cs="activity-job"
      data-status={job.status}
      className="rounded-panel border border-border bg-surface-raised p-5"
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <p className="font-mono text-xs text-fg-subtle uppercase">
            {t(jobKind(job.kind))}
            {job.phase && job.phase !== job.kind ? ` · ${t(jobPhase(job.phase))}` : ""}
          </p>
          <h2 id={heading} className="mt-1 break-words font-display text-lg font-semibold">
            {job.title}
          </h2>
          <p className="mt-1 text-sm text-fg-muted">{t(statusLabels[job.status])}</p>
        </div>
        <div className="flex gap-2">
          {job.status === "needs_user" && job.question && (
            <Button data-spatial="focusable" variant="primary" onPress={openQuestion}>
              <Trans>Review choices</Trans>
            </Button>
          )}
          {job.status === "paused" && resume && (
            <Button data-spatial="focusable" variant="primary" onPress={resume} isDisabled={busy}>
              <Play className="size-4" /> {resumeLabel}
            </Button>
          )}
          {isActiveJob(job) && (
            <Button data-spatial="focusable" variant="ghost" onPress={cancel} isDisabled={busy}>
              <Trans>Cancel job</Trans>
            </Button>
          )}
          {canRetry(job) && !failure && (
            <Button
              data-spatial="focusable"
              variant="primary"
              onPress={() => retry()}
              isDisabled={busy}
            >
              <RotateCcw className="size-4" /> {t({ message: "Retry", context: "job" })}
            </Button>
          )}
          {canRemove(job) && (
            <Button data-spatial="focusable" variant="ghost" onPress={remove} isDisabled={busy}>
              <Trash2 className="size-4" /> {removeLabel}
            </Button>
          )}
        </div>
      </div>
      {job.targetDir && (
        <p className="mt-3 break-all font-mono text-xs text-fg-subtle">
          <Trans>Target: {targetDir}</Trans>
        </p>
      )}
      {job.status === "running" && (
        <ProgressLine
          label={t`${title} progress`}
          status={job.message || t`Working…`}
          done={completed}
          total={progress?.total}
          pending={progress ? amount : t`Waiting for progress`}
          part="progress"
          className="mt-4"
        />
      )}
      {failure ? (
        <JobFailurePanel job={job} failure={failure} busy={busy} />
      ) : (
        job.error && (
          <p role="alert" className="mt-3 text-sm text-danger">
            {job.error}
          </p>
        )
      )}
      {job.verification && (
        <section
          aria-label={t`Verification results`}
          className="mt-4 rounded-control border border-border p-3"
        >
          <h3 className="text-sm font-semibold">
            {job.verification.valid ? t`Verification passed` : t`Verification found problems`}
          </h3>
          <p className="mt-1 text-xs text-fg-muted">
            {plural(job.verification.checkedFiles, {
              one: "# file checked",
              other: "# files checked",
            })}{" "}
            ·{" "}
            {plural(job.verification.missing.length, {
              one: "# missing",
              other: "# missing",
            })}{" "}
            ·{" "}
            {plural(job.verification.changed.length, {
              one: "# changed",
              other: "# changed",
            })}
          </p>
          <ul className="mt-2 text-xs text-fg-muted">
            {job.verification.missing.map((path) => (
              <li key={`missing:${path}`}>
                <Trans>Missing: {path}</Trans>
              </li>
            ))}
            {job.verification.changed.map((path) => (
              <li key={`changed:${path}`}>
                <Trans>Changed: {path}</Trans>
              </li>
            ))}
          </ul>
        </section>
      )}
      {job.warnings.length > 0 && (
        <section aria-label={t`Warnings`} className="mt-4 rounded-control border border-border p-3">
          <h3 className="text-sm font-semibold">
            <Trans>Warnings</Trans>
          </h3>
          <ul className="mt-2 space-y-1 text-sm text-fg-muted">
            {[
              ...new Map(
                job.warnings.map((warning) => [`${warning.code}:${warning.message}`, warning]),
              ).values(),
            ].map((warning) => (
              <li key={`${warning.code}:${warning.message}`}>{warning.message}</li>
            ))}
          </ul>
        </section>
      )}
      {job.dependencies.length > 0 && (
        <section aria-label={t`Dependencies`} className="mt-4">
          <h3 className="text-sm font-semibold">
            <Trans>Dependencies</Trans>
          </h3>
          <ul className="mt-1 text-sm text-fg-muted">
            {job.dependencies.map((dependency) => (
              <li key={dependency.id}>
                {dependency.name} —{" "}
                {dependency.installed
                  ? t({ message: "Installed", context: "dependency status" })
                  : t({ message: "Required", context: "dependency status" })}
              </li>
            ))}
          </ul>
        </section>
      )}
      <Disclosure className="mt-4 border-t border-border pt-3">
        <Heading>
          <Button slot="trigger" data-spatial="focusable" variant="ghost" size="sm">
            <Trans>Advanced strategies</Trans>
          </Button>
        </Heading>
        <DisclosurePanel className="mt-2 space-y-2">
          {job.strategies.length === 0 && (
            <p className="text-sm text-fg-muted">
              <Trans>No strategy details are available yet.</Trans>
            </p>
          )}
          {job.strategies.map((strategy) => {
            const strategyLabel = strategy.label;
            return (
              <div
                key={strategy.id}
                data-part="strategy"
                data-selected={strategy.selected || undefined}
                className="rounded-control border border-border p-3"
              >
                <p className="text-sm font-semibold">
                  {strategy.label}
                  {strategy.selected &&
                    ` · ${t({ message: "Selected", comment: "The install strategy the installer chose" })}`}
                </p>
                <p className="mt-1 text-xs text-fg-muted">{strategy.reason}</p>
                {(job.status === "failed" || job.status === "cancelled") && (
                  <Button
                    data-spatial="focusable"
                    size="sm"
                    className="mt-2"
                    isDisabled={busy}
                    onPress={() => retry(strategy.id)}
                  >
                    {t({
                      message: `Retry with ${strategyLabel}`,
                      comment:
                        "Runs a failed job again with another install strategy, named by the installer",
                    })}
                  </Button>
                )}
              </div>
            );
          })}
        </DisclosurePanel>
      </Disclosure>
      {menus.menu}
    </article>
  );
}
