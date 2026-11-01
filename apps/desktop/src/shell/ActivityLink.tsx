import { plural } from "@lingui/core/macro";
import { useLingui } from "@lingui/react/macro";
import { Link } from "@tanstack/react-router";
import { Bell } from "lucide-react";
import { isActiveJob } from "../api/install";
import { useUnreadNotifications } from "../api/notifications";
import { useJobs } from "../views/activity/ActivityProvider";

/** The bell: Activity, with the number of running jobs and, apart from
 * them, the unread notifications. It opens the jobs while one waits for an
 * answer, else the notifications while some are unread. */
export function ActivityLink({ className = "size-5" }: { className?: string }) {
  const { t } = useLingui();
  const jobs = useJobs();
  const active = jobs.data?.filter(isActiveJob).length ?? 0;
  const waiting = jobs.data?.filter((job) => job.status === "needs_user").length ?? 0;
  const unread = useUnreadNotifications().data ?? 0;
  const label = [
    t({ message: "Activity", context: "navigation" }),
    active && t({ message: plural(active, { one: "# active job", other: "# active jobs" }) }),
    waiting &&
      t({
        message: plural(waiting, { one: "# needs your attention", other: "# need your attention" }),
        comment: "Jobs waiting for an answer, after the Activity button's name",
      }),
    unread &&
      t({
        message: plural(unread, {
          one: "# unread notification",
          other: "# unread notifications",
        }),
      }),
  ]
    .filter(Boolean)
    .join(", ");
  return (
    <Link
      to="/activity"
      search={{ tab: !waiting && unread ? "notifications" : "jobs" }}
      aria-label={label}
      data-spatial="focusable"
      data-part="activity"
      className="relative rounded-control p-2 text-fg-muted focus-visible:focus-ring"
    >
      <Bell className={className} />
      {active > 0 && (
        <span
          data-part="job-count"
          data-status={waiting ? "needs-user" : "active"}
          className="absolute -top-1 -right-1 min-w-4 rounded-full bg-accent-alt px-1 text-center font-mono text-[10px] text-accent-fg"
        >
          {active}
        </span>
      )}
      {unread > 0 && (
        <span
          data-part="notification-count"
          className="absolute -bottom-1 -right-1 min-w-4 rounded-full border border-surface bg-accent px-1 text-center font-mono text-[10px] text-accent-fg"
        >
          {unread > 99 ? "99+" : unread}
        </span>
      )}
    </Link>
  );
}
