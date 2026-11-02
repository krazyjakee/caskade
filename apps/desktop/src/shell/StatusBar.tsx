import { plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { LoaderCircle } from "lucide-react";
import { useJobs } from "../views/activity/ActivityProvider";
import { AppUpdateLink } from "./AppUpdateLink";
import { SyncStatusLink } from "./SyncStatusLink";
import { useBackgroundStatus } from "./useBackgroundStatus";

export function StatusBar() {
  const { t } = useLingui();
  const { version, background, connection } = useBackgroundStatus();
  const jobs = useJobs();
  const activeJobs = jobs.data?.filter(
    (job) => job.status === "running" || job.status === "queued",
  );
  const waiting = jobs.data?.filter((job) => job.status === "needs_user").length ?? 0;
  const running = background.tasks.filter((task) => !task.queued);
  const queued = background.tasks.filter((task) => task.queued).length;
  const busy =
    connection !== "error" && (background.tasks.length > 0 || (activeJobs?.length ?? 0) > 0);
  const job = activeJobs?.find((job) => job.status === "running") ?? activeJobs?.[0];
  const phase =
    job &&
    (job.status === "queued"
      ? t({ message: "Queued", comment: "A job waiting for its turn" })
      : job.phase || t({ message: "Working", comment: "A running job with no phase" }));
  const description =
    running[0]?.label ??
    (queued ? background.tasks[0]?.label : undefined) ??
    (job
      ? t({
          message: `${{ phase }}: ${{ title: job.title }}`,
          comment: "The status bar's running job: its phase, then the game",
        })
      : undefined);
  const label =
    connection === "error"
      ? t`Background status unavailable`
      : description
        ? queued
          ? t({
              message: `${{ description }} · ${{ queued }} queued`,
              comment: "The running task, then how many wait after it",
            })
          : description
        : connection === "checking"
          ? t`Checking background tasks…`
          : waiting
            ? t({
                message: plural(waiting, {
                  one: "# task needs attention",
                  other: "# tasks need attention",
                }),
              })
            : t({
                message: "Ready",
                comment: "The status bar when nothing runs in the background",
              });
  const details = background.tasks.map((task) =>
    task.queued
      ? t({
          message: `Queued: ${{ task: task.label }}`,
          comment: "A background task waiting for its turn",
        })
      : task.label,
  );

  return (
    // Moves the window like the title bar; the update link keeps its click.
    // IGDB and Hasheous are credited in Settings → About (#162).
    <footer
      data-cs="status-bar"
      data-window-drag
      className="relative z-10 flex min-h-9 shrink-0 items-center gap-4 border-t border-border bg-surface-raised px-4 py-2 text-xs text-fg-muted"
    >
      <output
        aria-live="polite"
        aria-atomic="true"
        className="flex min-w-0 flex-1 items-center gap-2"
        title={details.length ? details.join("\n") : label}
      >
        <LoaderCircle
          data-part="spinner"
          aria-hidden="true"
          style={{ animationPlayState: "var(--cs-motion-ambient, running)" }}
          className={`size-3.5 shrink-0 ${busy ? "animate-worker text-accent-alt motion-reduce:animate-none" : "hidden"}`}
        />
        <span data-part="activity" className="truncate">
          {label}
        </span>
      </output>
      <SyncStatusLink />
      <AppUpdateLink />
      <span data-part="version" className="shrink-0 font-mono text-fg-subtle">
        <Trans>Caskade v{version}</Trans>
      </span>
    </footer>
  );
}
