import type { ReactNode } from "react";
import { ProgressBar } from "react-aria-components";

/** A job's progress bar, with what it is doing on the left and how far it
 * is on the right: a percentage of a known total, else `pending` while the
 * bar stays a quarter full. */
export function ProgressLine({
  label,
  status,
  done,
  total,
  pending,
  part,
  className = "mt-3",
}: {
  label: string;
  status: ReactNode;
  done: number;
  total: number | undefined;
  pending?: ReactNode;
  /** The bar's `data-part`. */
  part?: string;
  className?: string;
}) {
  const determinate = total !== undefined && Number.isFinite(total) && total > 0;
  const ratio = determinate ? Math.min(1, done / total) : 0;
  return (
    <ProgressBar
      aria-label={label}
      value={determinate ? ratio * 100 : undefined}
      isIndeterminate={!determinate}
      className={className}
    >
      {({ valueText }) => (
        <>
          <div className="mb-2 flex justify-between text-xs text-fg-muted">
            <span>{status}</span>
            <span>{determinate ? valueText : pending}</span>
          </div>
          <div className="h-1.5 overflow-hidden rounded-full bg-surface-overlay">
            <div
              data-part={part}
              className="h-full rounded-full bg-accent-alt"
              style={{ width: determinate ? `${ratio * 100}%` : "25%" }}
            />
          </div>
        </>
      )}
    </ProgressBar>
  );
}
