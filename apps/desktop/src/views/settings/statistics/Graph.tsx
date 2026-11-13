import { useLingui } from "@lingui/react/macro";
import { useId } from "react";
import { formatWindow } from "./format";

export interface GraphPoint {
  /** Unix milliseconds. */
  at: number;
  value: number | null;
}

interface GraphSeries {
  /** Its name in the legend and the summary ("Receive"). */
  name: string;
  points: GraphPoint[];
  /** A dashed line, so two series differ by more than colour. */
  dashed?: boolean;
  /** A token colour utility for the line. */
  tone: string;
}

const WIDTH = 240;
const HEIGHT = 64;

type Run = [number, number][];

/** The line's runs of points: a missing value or a jump of more than
 * `gap` between samples breaks it, so a gap shows as a gap. */
function runs(
  points: GraphPoint[],
  x: (at: number) => number,
  y: (value: number) => number,
  gap: number,
): Run[] {
  const out: Run[] = [];
  let run: Run = [];
  let last: number | null = null;
  for (const point of points) {
    const broken = point.value === null || (last !== null && point.at - last > gap);
    if (broken && run.length > 0) {
      out.push(run);
      run = [];
    }
    if (point.value === null) {
      last = null;
    } else {
      run.push([x(point.at), y(point.value)]);
      last = point.at;
    }
  }
  if (run.length > 0) out.push(run);
  return out;
}

function line(run: Run) {
  const [first] = run;
  // A lone point still shows, as a dot from its round cap.
  const points: Run = run.length === 1 && first ? [first, [first[0] + 0.01, first[1]]] : run;
  return points
    .map(([px, py], i) => `${i === 0 ? "M" : "L"}${px.toFixed(2)},${py.toFixed(2)}`)
    .join(" ");
}

function area(run: Run) {
  const first = run[0]?.[0] ?? 0;
  const last = run.at(-1)?.[0] ?? 0;
  return `${line(run)} L${last.toFixed(2)},${HEIGHT} L${first.toFixed(2)},${HEIGHT} Z`;
}

/** A recent-history graph: time runs left to right over `windowMs`, ending
 * at `end`. The values themselves are written beside it by the card; this
 * names its range and summarises it for assistive technology. */
export function Graph({
  label,
  series,
  end,
  windowMs,
  intervalMs,
  max,
  format,
}: {
  label: string;
  series: GraphSeries[];
  end: number;
  windowMs: number;
  intervalMs: number;
  /** The top of the scale. */
  max: number;
  format: (value: number) => string;
}) {
  const { t } = useLingui();
  const clip = useId();
  const start = end - windowMs;
  const x = (at: number) => ((at - start) / windowMs) * WIDTH;
  const y = (value: number) =>
    HEIGHT - (Math.min(max, Math.max(0, value)) / max) * (HEIGHT - 2) - 1;
  const summary = series
    .map(({ name, points }) => {
      const values = points.map((point) => point.value).filter((value) => value !== null);
      if (values.length === 0) return t`${{ name }}: no readings`;
      const last = points.at(-1)?.value;
      const now = last === null || last === undefined ? t`no reading` : format(last);
      const peak = format(Math.max(...values));
      const low = format(Math.min(...values));
      const missed = points.length - values.length;
      return missed > 0
        ? t({
            message: `${{ name }}: now ${{ now }}, peak ${{ peak }}, low ${{ low }}, ${{ missed }} missed`,
            comment: "A graph's summary; {missed} is how many samples couldn't be read",
          })
        : t`${{ name }}: now ${{ now }}, peak ${{ peak }}, low ${{ low }}`;
    })
    .join("; ");
  return (
    <figure data-part="graph" className="flex min-w-0 flex-col gap-1">
      <div className="relative">
        <svg
          viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
          preserveAspectRatio="none"
          role="img"
          aria-label={t({
            message: `${{ label }}, last ${{ span: formatWindow(windowMs) }}. ${{ summary }}.`,
            comment: "A graph: what it shows, the time it covers and its summary",
          })}
          className="h-16 w-full overflow-visible console:h-24"
        >
          <defs>
            <clipPath id={clip}>
              <rect x="0" y="0" width={WIDTH} height={HEIGHT} />
            </clipPath>
          </defs>
          {[0.25, 0.5, 0.75].map((level) => (
            <line
              key={level}
              x1="0"
              x2={WIDTH}
              y1={HEIGHT * level}
              y2={HEIGHT * level}
              stroke="currentColor"
              strokeWidth="1"
              vectorEffect="non-scaling-stroke"
              className="text-border opacity-60"
            />
          ))}
          <g clipPath={`url(#${clip})`}>
            {series.map((item, index) =>
              runs(item.points, x, y, intervalMs * 2.5).map((run) => (
                <g key={`${item.name}-${run[0]?.[0]}`} className={item.tone}>
                  {index === 0 && (
                    <path d={area(run)} fill="currentColor" fillOpacity="0.12" stroke="none" />
                  )}
                  <path
                    d={line(run)}
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                    strokeLinejoin="round"
                    strokeLinecap="round"
                    strokeDasharray={item.dashed ? "5 4" : undefined}
                    vectorEffect="non-scaling-stroke"
                  />
                </g>
              )),
            )}
          </g>
        </svg>
        <span className="pointer-events-none absolute top-0 left-1 font-mono text-[0.65rem] text-fg-subtle">
          {format(max)}
        </span>
      </div>
      <div
        aria-hidden
        className="flex justify-between font-mono text-[0.65rem] tracking-wider text-fg-subtle uppercase"
      >
        <span>−{formatWindow(windowMs)}</span>
        <span>
          {t({ message: "now", comment: "The right end of a history graph's time axis" })}
        </span>
      </div>
    </figure>
  );
}
