import { useLingui } from "@lingui/react/macro";
import { formatPercent } from "../../../api/presentation";

const RADIUS = 48;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;
/** The arc covers three quarters of the circle, open at the bottom. */
const ARC = CIRCUMFERENCE * 0.75;
/** At or above this a gauge says "High" and turns to the warning colour. */
export const HIGH = 90;

/** A utilization gauge, 0–100 %, with its value written inside. `null`
 * draws an empty track and says why in `missing`. */
export function Gauge({
  label,
  value,
  missing,
}: {
  /** What it measures, for assistive technology ("CPU usage"). */
  label: string;
  value: number | null;
  missing?: string;
}) {
  const { t } = useLingui();
  const fraction = value === null ? 0 : Math.min(1, Math.max(0, value / 100));
  const high = value !== null && value >= HIGH;
  return (
    // biome-ignore lint/a11y/useSemanticElements: a native <meter> can't hold the drawn arc and its label.
    <div
      data-part="gauge"
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={value === null ? undefined : Math.round(value)}
      aria-valuetext={
        value === null
          ? (missing ?? t`No reading`)
          : high
            ? t({
                message: `${{ percent: formatPercent(value) }}, high`,
                comment: "A gauge's reading at or above 90 %",
              })
            : formatPercent(value)
      }
      className="relative size-28 shrink-0 console:size-36"
    >
      <svg viewBox="0 0 120 120" aria-hidden className="size-full">
        <circle
          cx="60"
          cy="60"
          r={RADIUS}
          fill="none"
          stroke="currentColor"
          strokeWidth="10"
          strokeLinecap="round"
          strokeDasharray={`${ARC} ${CIRCUMFERENCE}`}
          transform="rotate(135 60 60)"
          className="text-border"
        />
        {value !== null && (
          <circle
            cx="60"
            cy="60"
            r={RADIUS}
            fill="none"
            stroke="currentColor"
            strokeWidth="10"
            strokeLinecap="round"
            strokeDasharray={`${ARC * fraction} ${CIRCUMFERENCE}`}
            transform="rotate(135 60 60)"
            className={`transition-[stroke-dasharray] duration-(--cs-motion-duration-slow) ease-(--cs-motion-ease-out) ${high ? "text-warning" : "text-accent-alt"}`}
          />
        )}
      </svg>
      <div className="absolute inset-0 flex flex-col items-center justify-center pt-1 text-center">
        <span
          className={`font-display text-2xl font-semibold tabular-nums ${value === null ? "text-fg-subtle" : "text-fg"}`}
        >
          {formatPercent(value)}
        </span>
        <span className="font-mono text-[0.65rem] tracking-wider text-fg-muted uppercase">
          {value === null
            ? t({ message: "n/a", comment: "A gauge without a reading" })
            : high
              ? t({ message: "High", context: "gauge" })
              : t({ message: "Used", context: "gauge" })}
        </span>
      </div>
    </div>
  );
}
