import type { MessageDescriptor } from "@lingui/core";
import { msg, plural, t as translate } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import {
  CircleSlash,
  Cpu,
  HardDrive,
  type LucideIcon,
  MemoryStick,
  MonitorCog,
  Network,
} from "lucide-react";
import { type ReactNode, useId } from "react";
import { formatBytes, formatPercent, formatTime } from "../../../api/presentation";
import type {
  DiskSample,
  GpuSample,
  InterfaceSample,
  SystemSample,
  SystemStatsState,
} from "../../../api/systemStats";
import { formatRate, formatWindow, niceCeiling, percentOf } from "./format";
import { Gauge, HIGH } from "./Gauge";
import { Graph, type GraphPoint } from "./Graph";

type Metric = "cpu" | "memory" | "gpu" | "network" | "storage";

/** One metric. Focusable, so a pad or the keyboard reaches and scrolls to
 * every card without hovering a chart. */
function Card({
  metric,
  title,
  detail,
  icon: Icon,
  wide = false,
  children,
}: {
  metric: Metric;
  title: string;
  detail?: string;
  icon: LucideIcon;
  wide?: boolean;
  children: ReactNode;
}) {
  const heading = useId();
  return (
    <article
      data-part="card"
      data-metric={metric}
      aria-labelledby={heading}
      // biome-ignore lint/a11y/noNoninteractiveTabindex: a pad scrolls the dashboard card by card.
      tabIndex={0}
      className={`flex min-w-0 flex-col gap-4 rounded-panel border border-border bg-surface-raised p-4 transition-opacity duration-(--cs-motion-duration-base) group-data-[status=stale]:opacity-70 focus-visible:focus-ring sm:p-5 ${wide ? "lg:col-span-2" : ""}`}
    >
      <header className="flex min-w-0 items-center gap-2">
        <Icon aria-hidden className="size-4 shrink-0 text-accent-alt" />
        <h3 id={heading} className="truncate font-display text-lg font-semibold tracking-wide">
          {title}
        </h3>
        {detail && (
          <span className="ml-auto shrink-0 font-mono text-xs tracking-wider text-fg-muted uppercase">
            {detail}
          </span>
        )}
      </header>
      {children}
    </article>
  );
}

function Unavailable({ children }: { children: ReactNode }) {
  return (
    <p data-part="unavailable" className="flex items-start gap-2 text-sm text-fg-muted">
      <CircleSlash aria-hidden className="mt-0.5 size-4 shrink-0 text-fg-subtle" />
      <span>{children}</span>
    </p>
  );
}

function Facts({ items }: { items: [string, string][] }) {
  return (
    <dl className="grid min-w-0 flex-1 grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
      {items.map(([term, value]) => (
        <div key={term} className="contents">
          <dt className="text-fg-muted">{term}</dt>
          <dd className="truncate text-right font-mono tabular-nums text-fg">{value}</dd>
        </div>
      ))}
    </dl>
  );
}

interface Frame {
  end: number;
  windowMs: number;
  intervalMs: number;
}

function series(
  samples: SystemSample[],
  pick: (sample: SystemSample) => number | null,
): GraphPoint[] {
  return samples.map((sample) => ({ at: sample.at, value: sample.error ? null : pick(sample) }));
}

/** Why the newest value is missing. */
function missingReason(latest: SystemSample | undefined, everRead: boolean, measuring: string) {
  if (latest?.error) return latest.error;
  return everRead ? translate`No reading this time.` : measuring;
}

function CpuCard({ samples, frame }: { samples: SystemSample[]; frame: Frame }) {
  const latest = samples.at(-1);
  const usage = latest?.error ? null : (latest?.cpu.usage ?? null);
  const points = series(samples, (sample) => sample.cpu.usage);
  const everRead = points.some((point) => point.value !== null);
  const cores = samples.findLast((sample) => sample.cpu.cores > 0)?.cpu.cores;
  const { t } = useLingui();
  return (
    <Card
      metric="cpu"
      title={t({ message: "CPU", comment: "The processor's statistics card" })}
      icon={Cpu}
      detail={
        cores ? plural({ threads: cores }, { one: "# thread", other: "# threads" }) : undefined
      }
    >
      <div className="flex items-center gap-4">
        <Gauge label={t`CPU usage`} value={usage} />
        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <Facts items={[[t({ message: "Usage", context: "statistics" }), formatPercent(usage)]]} />
          {usage === null && (
            <Unavailable>{missingReason(latest, everRead, t`Measuring…`)}</Unavailable>
          )}
        </div>
      </div>
      <Graph
        label={t`CPU usage`}
        series={[
          {
            name: t({ message: "Usage", context: "statistics" }),
            points,
            tone: "text-accent-alt",
          },
        ]}
        max={100}
        format={formatPercent}
        {...frame}
      />
    </Card>
  );
}

function MemoryCard({ samples, frame }: { samples: SystemSample[]; frame: Frame }) {
  const latest = samples.at(-1);
  const memory = latest?.error ? null : (latest?.memory ?? null);
  const usage = memory ? percentOf(memory.used, memory.total) : null;
  const points = series(samples, (sample) =>
    sample.memory ? percentOf(sample.memory.used, sample.memory.total) : null,
  );
  const everRead = points.some((point) => point.value !== null);
  const { t } = useLingui();
  return (
    <Card
      metric="memory"
      title={t({ message: "Memory", comment: "The RAM statistics card" })}
      icon={MemoryStick}
      detail={
        memory
          ? t({
              message: `${{ size: formatBytes(memory.total) }} RAM`,
              comment: "Installed memory",
            })
          : undefined
      }
    >
      <div className="flex items-center gap-4">
        <Gauge label={t`Memory used`} value={usage} />
        {memory ? (
          <Facts
            items={[
              [t({ message: "Used", context: "memory" }), formatBytes(memory.used)],
              [t({ message: "Available", context: "memory" }), formatBytes(memory.available)],
              [t({ message: "Total", context: "memory" }), formatBytes(memory.total)],
            ]}
          />
        ) : (
          <Unavailable>
            {missingReason(latest, everRead, t`Memory usage couldn't be read on this system.`)}
          </Unavailable>
        )}
      </div>
      <Graph
        label={t`Memory used`}
        series={[{ name: t({ message: "Used", context: "memory" }), points, tone: "text-accent" }]}
        max={100}
        format={formatPercent}
        {...frame}
      />
    </Card>
  );
}

const vendors: Record<GpuSample["vendor"], string | MessageDescriptor> = {
  amd: "AMD",
  intel: "Intel",
  nvidia: "NVIDIA",
  other: msg({ message: "GPU", comment: "A graphics card from an unknown vendor" }),
};

function GpuCard({
  gpu,
  samples,
  frame,
}: {
  gpu: GpuSample;
  samples: SystemSample[];
  frame: Frame;
}) {
  const latest = samples.at(-1);
  const now = latest?.error ? undefined : latest?.gpus.devices.find((item) => item.id === gpu.id);
  const usage = now?.usage ?? null;
  const points = series(
    samples,
    (sample) => sample.gpus.devices.find((item) => item.id === gpu.id)?.usage ?? null,
  );
  const everRead = points.some((point) => point.value !== null);
  const { t } = useLingui();
  const memory =
    gpu.memoryTotal !== null
      ? t({
          message: `${{ used: gpu.memoryUsed === null ? "—" : formatBytes(gpu.memoryUsed) }} of ${{
            total: formatBytes(gpu.memoryTotal),
          }}`,
          comment: "Video memory used of the GPU's total",
        })
      : "—";
  const reason =
    now?.unavailable ?? (now ? null : missingReason(latest, everRead, t`This GPU is gone.`));
  const vendor = vendors[gpu.vendor];
  const usageLabel = t`${{ name: gpu.name }} usage`;
  return (
    <Card
      metric="gpu"
      title={gpu.name}
      icon={MonitorCog}
      detail={`${typeof vendor === "string" ? vendor : t(vendor)} · ${gpu.id}`}
    >
      <div className="flex items-center gap-4">
        <Gauge label={usageLabel} value={usage} />
        <div className="flex min-w-0 flex-1 flex-col gap-2">
          <Facts
            items={[
              [t({ message: "Usage", context: "statistics" }), formatPercent(usage)],
              [t`Video memory`, memory],
            ]}
          />
          {usage === null && reason && <Unavailable>{reason}</Unavailable>}
        </div>
      </div>
      {everRead && (
        <Graph
          label={usageLabel}
          series={[
            {
              name: t({ message: "Usage", context: "statistics" }),
              points,
              tone: "text-accent-alt",
            },
          ]}
          max={100}
          format={formatPercent}
          {...frame}
        />
      )}
    </Card>
  );
}

const kinds: Record<InterfaceSample["kind"], MessageDescriptor> = {
  physical: msg({ message: "Counted", comment: "A network interface counted in the totals" }),
  virtual: msg`Virtual, not counted`,
  loopback: msg`Loopback, not counted`,
};

function Swatch({ dashed }: { dashed?: boolean }) {
  return (
    <svg aria-hidden viewBox="0 0 16 4" className="h-1 w-4 shrink-0 overflow-visible">
      <line
        x1="0"
        x2="16"
        y1="2"
        y2="2"
        stroke="currentColor"
        strokeWidth="2"
        strokeDasharray={dashed ? "4 3" : undefined}
      />
    </svg>
  );
}

function NetworkCard({ samples, frame }: { samples: SystemSample[]; frame: Frame }) {
  const latest = samples.at(-1);
  const good = samples.findLast((sample) => !sample.error);
  const network = latest?.error ? null : (latest?.network ?? null);
  const received = series(samples, (sample) => sample.network.received);
  const sent = series(samples, (sample) => sample.network.transmitted);
  const peak = Math.max(0, ...[...received, ...sent].map((point) => point.value ?? 0));
  const interfaces = good?.network.interfaces ?? [];
  const { t } = useLingui();
  const receiving = t({ message: "Receiving", context: "network" });
  const sending = t({ message: "Sending", context: "network" });
  return (
    <Card
      metric="network"
      title={t({ message: "Network", comment: "The network statistics card" })}
      icon={Network}
      wide
    >
      {interfaces.length === 0 ? (
        <Unavailable>{latest?.error ?? t`No network interfaces found.`}</Unavailable>
      ) : (
        <>
          <div className="flex flex-wrap gap-x-8 gap-y-2">
            <p className="flex items-center gap-2 text-accent-alt">
              <Swatch />
              <span className="text-sm text-fg-muted">↓ {receiving}</span>
              <span className="font-display text-xl font-semibold tabular-nums text-fg">
                {formatRate(network?.received ?? null)}
              </span>
            </p>
            <p className="flex items-center gap-2 text-accent">
              <Swatch dashed />
              <span className="text-sm text-fg-muted">↑ {sending}</span>
              <span className="font-display text-xl font-semibold tabular-nums text-fg">
                {formatRate(network?.transmitted ?? null)}
              </span>
            </p>
          </div>
          <Graph
            label={t`Network traffic`}
            series={[
              { name: receiving, points: received, tone: "text-accent-alt" },
              { name: sending, points: sent, tone: "text-accent", dashed: true },
            ]}
            max={niceCeiling(peak)}
            format={formatRate}
            {...frame}
          />
          <ul className="flex flex-col divide-y divide-border text-sm">
            {interfaces.map((item) => (
              <li
                key={item.name}
                data-part="interface"
                data-kind={item.kind}
                className="flex flex-wrap items-baseline gap-x-4 gap-y-1 py-1.5 data-[kind=loopback]:text-fg-muted data-[kind=virtual]:text-fg-muted"
              >
                <span className="min-w-0 truncate font-mono">{item.name}</span>
                <span className="text-xs text-fg-muted">{t(kinds[item.kind])}</span>
                <span className="ml-auto font-mono tabular-nums">
                  ↓ {formatRate(item.received)} · ↑ {formatRate(item.transmitted)}
                </span>
              </li>
            ))}
          </ul>
          <p className="text-xs text-fg-muted">
            <Trans>
              Totals add up physical interfaces only, so traffic through a VPN, a container or
              loopback isn't counted twice. — means no rate yet.
            </Trans>
          </p>
        </>
      )}
    </Card>
  );
}

function Volume({ disk }: { disk: DiskSample }) {
  const used = disk.total - disk.available;
  const percent = percentOf(used, disk.total) ?? 0;
  const full = percent >= HIGH;
  const mounts = [disk.mount, ...disk.otherMounts].join(", ");
  const { t } = useLingui();
  return (
    <li data-part="volume" className="flex min-w-0 flex-col gap-1.5">
      <div className="flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-0.5">
        <span className="font-medium">{disk.name}</span>
        <span className="min-w-0 truncate font-mono text-xs text-fg-muted" title={mounts}>
          {mounts}
        </span>
        {disk.removable && (
          <span className="rounded-control border border-border px-1.5 text-xs text-fg-muted">
            <Trans comment="A removable drive">Removable</Trans>
          </span>
        )}
        <span className="ml-auto font-mono text-xs tabular-nums">
          {full
            ? t({
                message: `${{ percent: formatPercent(percent) }} full · almost out of space`,
                comment: "A drive at or above 90 % full",
              })
            : t({
                message: `${{ percent: formatPercent(percent) }} full`,
                comment: "A drive's use",
              })}
        </span>
      </div>
      {/* biome-ignore lint/a11y/useSemanticElements: WebKitGTK's native <meter> ignores the theme's colours. */}
      <div
        data-part="capacity"
        role="meter"
        aria-label={t`${{ name: disk.name }} (${{ mount: disk.mount }}) space used`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(percent)}
        aria-valuetext={t`${{ used: formatBytes(used) }} used of ${{ total: formatBytes(disk.total) }}, ${{ free: formatBytes(disk.available) }} free`}
        className="h-2.5 overflow-hidden rounded-full bg-surface-base/70 console:h-3.5"
      >
        <div
          className={`h-full rounded-full transition-[width] duration-(--cs-motion-duration-slow) ${full ? "bg-warning" : "bg-accent-alt"}`}
          style={{ width: `${percent}%` }}
        />
      </div>
      <div className="flex flex-wrap justify-between gap-x-4 font-mono text-xs text-fg-muted tabular-nums">
        <span>{t`${{ used: formatBytes(used) }} used`}</span>
        <span>
          {t`${{ free: formatBytes(disk.available) }} free of ${{ total: formatBytes(disk.total) }}`}
          {disk.fileSystem ? ` · ${disk.fileSystem}` : ""}
        </span>
      </div>
    </li>
  );
}

function StorageCard({ samples }: { samples: SystemSample[] }) {
  // Capacity changes slowly: a failed sample keeps the last volumes.
  const good = samples.findLast((sample) => !sample.error);
  const disks = good?.disks ?? [];
  const { t } = useLingui();
  return (
    <Card
      metric="storage"
      title={t({ message: "Storage", comment: "The drives statistics card" })}
      icon={HardDrive}
      detail={t({ message: "Capacity", comment: "The storage card shows capacity, not speed" })}
      wide
    >
      {disks.length === 0 ? (
        <Unavailable>
          <Trans>No drives found.</Trans>
        </Unavailable>
      ) : (
        <ul className="flex flex-col gap-4">
          {disks.map((disk) => (
            <Volume key={disk.id} disk={disk} />
          ))}
        </ul>
      )}
    </Card>
  );
}

/** `error` as the end of a sentence. */
const clause = (error: string) => error.trim().replace(/\.$/, "");

function statusText(stats: SystemStatsState) {
  const shown = formatWindow(stats.history * stats.intervalMs);
  const latest = stats.samples.at(-1);
  switch (stats.status) {
    case "loading":
      return translate`Waiting for the first reading…`;
    case "live": {
      if (stats.intervalMs === 1000)
        return translate`Live: updates every second, graphs show the last ${{ span: shown }}.`;
      const interval = formatWindow(stats.intervalMs);
      return translate`Live: updates every ${{ interval }}, graphs show the last ${{ span: shown }}.`;
    }
    case "stale": {
      const when = latest ? formatTime(latest.at, { seconds: true }) : translate`a while ago`;
      if (!stats.error)
        return translate`Not updating: the last reading is from ${{ when }}. Trying again.`;
      const reason = clause(stats.error);
      return translate({
        message: `Not updating: the last reading is from ${{ when }}. ${{ reason }}. Trying again.`,
        comment: "{reason} is the sampler's error",
      });
    }
    case "error": {
      const reason = clause(stats.error ?? translate`the sampler didn't answer`);
      return translate`Statistics aren't available: ${{ reason }}. Trying again.`;
    }
  }
}

/** Settings → Statistics: this machine's CPU, GPUs, memory, network and
 * storage, from `stats` (`useSystemStats`). */
export function StatisticsDashboard({ stats }: { stats: SystemStatsState }) {
  const { samples, intervalMs, history } = stats;
  const latest = samples.at(-1);
  const frame: Frame = { end: latest?.at ?? 0, windowMs: history * intervalMs, intervalMs };
  const gpus = samples.findLast((sample) => !sample.error)?.gpus;
  const title = useId();
  const { t } = useLingui();
  return (
    <section
      data-cs="system-stats"
      data-status={stats.status}
      aria-labelledby={title}
      className="group flex min-w-0 flex-col gap-5 console:gap-7"
    >
      <header className="flex flex-col gap-1">
        <h2
          id={title}
          data-part="title"
          className="font-display text-2xl font-semibold tracking-wide"
        >
          <Trans>Statistics</Trans>
        </h2>
        <p
          data-part="status"
          role="status"
          className="flex items-center gap-2 text-sm text-fg-muted"
        >
          <span
            aria-hidden
            className="size-2 shrink-0 rounded-full bg-fg-subtle group-data-[status=live]:bg-status-installed group-data-[status=error]:bg-danger group-data-[status=stale]:bg-warning"
          />
          {statusText(stats)}
        </p>
      </header>
      {samples.length > 0 && (
        <div className="grid gap-4 lg:grid-cols-2 console:gap-6">
          <CpuCard samples={samples} frame={frame} />
          <MemoryCard samples={samples} frame={frame} />
          {gpus && gpus.devices.length > 0 ? (
            gpus.devices.map((gpu) => (
              <GpuCard key={gpu.id} gpu={gpu} samples={samples} frame={frame} />
            ))
          ) : (
            <Card
              metric="gpu"
              title={t({ message: "GPU", comment: "The graphics card statistics card" })}
              icon={MonitorCog}
            >
              <Unavailable>{gpus?.unavailable ?? latest?.error ?? t`No GPU found.`}</Unavailable>
            </Card>
          )}
          <NetworkCard samples={samples} frame={frame} />
          <StorageCard samples={samples} />
        </div>
      )}
    </section>
  );
}
