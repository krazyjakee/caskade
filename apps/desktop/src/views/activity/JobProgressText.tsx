import type { ReactNode } from "react";
import type { JobSnapshot } from "../../api/install";
import { useJobs } from "./ActivityProvider";

/** One job as a line of text: starting until Activity lists it, then how
 * far it got (a percentage of a byte total, else null) and how it ended. */
export function JobProgressText({
  jobId,
  starting,
  completed,
  ended,
  needsUser,
  running,
}: {
  jobId: string;
  starting: ReactNode;
  completed: ReactNode;
  /** Cancelled, or failed with the job's error. */
  ended: (job: JobSnapshot) => ReactNode;
  needsUser: ReactNode;
  running: (job: JobSnapshot, percent: number | null) => ReactNode;
}) {
  const job = useJobs().data?.find((each) => each.id === jobId);
  if (!job)
    return (
      <p role="status" className="text-fg-muted">
        {starting}
      </p>
    );
  if (job.status === "completed") return <p className="text-accent-alt">{completed}</p>;
  if (job.status === "failed" || job.status === "cancelled")
    return (
      <p role="alert" className="text-danger">
        {ended(job)}
      </p>
    );
  if (job.status === "needs_user") return <p className="text-fg-muted">{needsUser}</p>;
  const percent =
    job.progress?.total && job.progress.unit === "bytes"
      ? Math.floor((job.progress.completed / job.progress.total) * 100)
      : null;
  return (
    <p role="status" className="text-fg-muted">
      {running(job, percent)}
    </p>
  );
}
