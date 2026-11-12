import { Button } from "@caskade/ui";
import { plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { Link, useNavigate } from "@tanstack/react-router";
import { useRef } from "react";
import { isActiveJob } from "../../api/install";
import { useUnreadNotifications } from "../../api/notifications";
import { RefreshCard } from "../../components/gameSelection/RefreshCard";
import { useScreenTabs } from "../../input";
import { useFollowJob, useJobActions, useJobs } from "./ActivityProvider";
import { JobCard } from "./JobCard";
import { NotificationFeed } from "./NotificationFeed";

/** Activity's tabs: running jobs, and the notification feed (#198). */
export const activityTabs = ["jobs", "notifications"] as const;
export type ActivityTab = (typeof activityTabs)[number];

export function ActivityView({
  tab = "jobs",
  unreadOnly = false,
  open,
}: {
  tab?: ActivityTab;
  unreadOnly?: boolean;
  open?: string;
}) {
  const { t } = useLingui();
  const jobs = useJobs();
  const unread = useUnreadNotifications().data ?? 0;
  const navigate = useNavigate();
  const tablist = useRef<HTMLDivElement>(null);
  useScreenTabs(
    activityTabs,
    tab,
    (next) => void navigate({ to: "/activity", search: { tab: next }, replace: true }),
    { tabs: tablist },
  );
  const active = jobs.data?.filter(isActiveJob).length ?? 0;
  const waiting = jobs.data?.filter((job) => job.status === "needs_user").length ?? 0;
  const labels: Record<ActivityTab, { label: string; count: number; detail: string }> = {
    jobs: {
      label: t({ message: "Jobs", comment: "An Activity tab: installs and other background jobs" }),
      count: active,
      detail: waiting
        ? plural(waiting, {
            one: `${active} active, # needs you`,
            other: `${active} active, # need you`,
          })
        : plural(active, { one: "# active", other: "# active" }),
    },
    notifications: {
      label: t({ message: "Notifications", comment: "An Activity tab" }),
      count: unread,
      detail: plural(unread, { one: "# unread", other: "# unread" }),
    },
  };
  return (
    <div data-view="activity" data-focus-scope="activity" className="mt-4">
      <header className="mb-6 flex flex-wrap items-center justify-between gap-4">
        <div>
          <p className="font-mono text-xs tracking-widest text-fg-subtle uppercase">
            <Trans>Jobs and notifications</Trans>
          </p>
          <h1 className="mt-1 font-display text-3xl font-semibold">
            <Trans>Activity</Trans>
          </h1>
          <p className="mt-2 text-sm text-fg-muted">
            {plural(active, { one: "# active job", other: "# active jobs" })} ·{" "}
            {plural(unread, { one: "# unread notification", other: "# unread notifications" })}
          </p>
        </div>
      </header>
      <div
        ref={tablist}
        role="tablist"
        aria-label={t`Activity`}
        className="mb-6 flex flex-wrap gap-2"
      >
        {activityTabs.map((id) => {
          const selected = id === tab;
          const { label, detail } = labels[id];
          return (
            <Link
              key={id}
              to="/activity"
              search={{ tab: id }}
              replace
              role="tab"
              aria-selected={selected}
              aria-label={t({
                message: `${label}, ${detail}`,
                comment: "An Activity tab's name and what it holds: Jobs, 2 active",
              })}
              data-selected={selected || undefined}
              data-status={id === "jobs" && waiting ? "needs-user" : undefined}
              className="rounded-full border border-border bg-surface-raised px-4 py-1.5 text-sm text-fg-muted transition-colors duration-(--cs-motion-duration-fast) hover:text-fg focus-visible:focus-ring data-selected:border-accent-alt/50 data-selected:bg-accent/20 data-selected:text-fg"
            >
              {labels[id].label}{" "}
              <span className="font-mono text-xs text-fg-subtle">{labels[id].count}</span>
            </Link>
          );
        })}
      </div>
      <div role="tabpanel" aria-label={labels[tab].label}>
        {tab === "jobs" ? (
          <JobsPanel />
        ) : (
          // A new filter starts at the first page.
          <NotificationFeed
            key={unreadOnly ? "unread" : "all"}
            unreadOnly={unreadOnly}
            open={open}
          />
        )}
      </div>
    </div>
  );
}

function JobsPanel() {
  const jobs = useJobs();
  const action = useJobActions();
  const follow = useFollowJob();
  return (
    <>
      <RefreshCard />
      {jobs.isPending && (
        <p role="status">
          <Trans>Loading activity…</Trans>
        </p>
      )}
      {jobs.isError && (
        <div role="alert" className="rounded-panel border border-border p-4">
          <p>{jobs.error.message}</p>
          <Button
            className="mt-3"
            onPress={() => {
              void jobs.refetch();
            }}
          >
            <Trans>Try again</Trans>
          </Button>
        </div>
      )}
      {action.isError && (
        <p role="alert" className="mb-4 text-danger">
          {action.error.message}
        </p>
      )}
      {jobs.data?.length === 0 && (
        <p
          data-part="empty"
          className="rounded-panel border border-dashed border-border p-12 text-center text-fg-muted"
        >
          <Trans>No jobs yet. Install a game from a file, folder or URL.</Trans>
        </p>
      )}
      <div className="space-y-4">
        {jobs.data?.map((job) => (
          <div key={job.id}>
            <JobCard
              job={job}
              busy={action.isPending}
              cancel={() => action.mutate({ type: "cancel", jobId: job.id })}
              remove={() => action.mutate({ type: "remove", jobId: job.id })}
              resume={() =>
                action.mutate(
                  { type: "resume", jobId: job.id },
                  { onSuccess: () => follow(job.id) },
                )
              }
              retry={(strategyId) =>
                action.mutate(
                  { type: "retry", jobId: job.id, strategyId },
                  { onSuccess: () => follow(job.id) },
                )
              }
              openQuestion={() => follow(job.id)}
            />
            {job.status === "completed" && job.gameId && (
              <Link
                to="/games/$gameId"
                params={{ gameId: job.gameId }}
                // Its Back link comes back here (#195).
                search={{ from: "/activity" }}
                className="mt-2 inline-block text-sm text-accent-alt"
              >
                <Trans>Open game</Trans>
              </Link>
            )}
          </div>
        ))}
      </div>
    </>
  );
}
