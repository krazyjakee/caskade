/** Made-up machines for the Statistics dashboard: the browser preview, the
 * tests and Storybook. Values follow smooth curves, so graphs look alive
 * and every run draws the same. */
import type {
  DiskSample,
  GpuSample,
  InterfaceSample,
  SystemSample,
  SystemStatsApi,
  SystemStatsSnapshot,
  SystemStatsState,
} from "./systemStats";

const GIB = 1024 ** 3;
const MIB = 1024 ** 2;

/** `normal`: two GPUs, four volumes, physical and virtual interfaces.
 * `unavailable`: a Mac-like machine: no GPU source, no volumes, no
 * interfaces, no CPU measurement yet. `failing`: gaps from failed
 * samples, a GPU whose driver doesn't report usage, a removable drive. */
export type StatsScenario = "normal" | "unavailable" | "failing";

const wave = (seq: number, period: number, low: number, high: number, phase = 0) =>
  low + ((Math.sin((seq / period) * Math.PI * 2 + phase) + 1) / 2) * (high - low);

const round = (value: number) => Math.round(value * 10) / 10;

function gpus(seq: number, scenario: StatsScenario): GpuSample[] {
  if (scenario === "unavailable") return [];
  const amd: GpuSample = {
    id: "card1",
    name: "Radeon RX 6700 XT",
    vendor: "amd",
    usage: round(wave(seq, 23, 18, 92)),
    memoryUsed: Math.round(wave(seq, 40, 3, 7)) * GIB,
    memoryTotal: 12 * GIB,
    unavailable: null,
  };
  if (scenario === "failing")
    return [
      amd,
      {
        id: "card0",
        name: "Intel GPU",
        vendor: "intel",
        usage: null,
        memoryUsed: null,
        memoryTotal: null,
        unavailable: "Intel's driver doesn't report GPU usage to apps.",
      },
    ];
  return [
    amd,
    {
      id: "card2",
      name: "NVIDIA GeForce RTX 3060",
      vendor: "nvidia",
      usage: round(wave(seq, 17, 2, 64, 1)),
      memoryUsed: 2300 * MIB,
      memoryTotal: 12288 * MIB,
      unavailable: null,
    },
  ];
}

function disks(scenario: StatsScenario): DiskSample[] {
  if (scenario === "unavailable") return [];
  const disk = (
    name: string,
    mount: string,
    total: number,
    free: number,
    extra: Partial<DiskSample> = {},
  ): DiskSample => ({
    id: mount,
    name,
    mount,
    otherMounts: [],
    device: "",
    fileSystem: "ext4",
    removable: false,
    total: total * GIB,
    available: free * GIB,
    ...extra,
  });
  const list = [
    disk("System", "/", 500, 210, { otherMounts: ["/home"], fileSystem: "btrfs" }),
    disk("Games", "/mnt/games", 2000, 140),
    disk("SD Card", "/run/media/player/SD Card", 256, 200, {
      removable: true,
      fileSystem: "exfat",
    }),
  ];
  if (scenario === "normal") list.splice(2, 0, disk("Backup", "/mnt/backup", 4000, 3800));
  return list;
}

function interfaces(seq: number, scenario: StatsScenario, first: boolean): InterfaceSample[] {
  if (scenario === "unavailable") return [];
  const rate = (value: number) => (first ? null : Math.round(value));
  const list: InterfaceSample[] = [
    {
      name: "eth0",
      kind: "physical",
      received: rate(wave(seq, 11, 40_000, 9_500_000)),
      transmitted: rate(wave(seq, 13, 8_000, 620_000, 2)),
    },
    {
      name: "wlan0",
      kind: "physical",
      received: rate(wave(seq, 29, 0, 120_000, 1)),
      transmitted: rate(wave(seq, 31, 0, 40_000)),
    },
    {
      name: "docker0",
      kind: "virtual",
      received: rate(wave(seq, 7, 0, 30_000)),
      transmitted: rate(wave(seq, 7, 0, 25_000)),
    },
    { name: "lo", kind: "loopback", received: rate(2_000), transmitted: rate(2_000) },
  ];
  return scenario === "failing" ? list.filter((item) => item.name !== "wlan0") : list;
}

/** Sample `seq` of `scenario`, taken at `at`. `failing` loses every
 * seventh sample. */
export function fixtureSample(
  seq: number,
  at: number,
  scenario: StatsScenario = "normal",
): SystemSample {
  if (scenario === "failing" && seq % 7 === 3)
    return {
      seq,
      at,
      error: "This sample couldn't be read.",
      cpu: { usage: null, cores: 0 },
      memory: null,
      gpus: { devices: [], unavailable: null },
      disks: [],
      network: { interfaces: [], received: null, transmitted: null },
    };
  const first = seq <= 1;
  const list = interfaces(seq, scenario, first);
  const physical = list.filter((item) => item.kind === "physical");
  const sum = (pick: (item: InterfaceSample) => number | null) =>
    physical.length === 0 || first
      ? null
      : physical.reduce((total, item) => total + (pick(item) ?? 0), 0);
  const total = 32 * GIB;
  const used = Math.round(wave(seq, 37, 9, 21) * 4) * (GIB / 4);
  const devices = gpus(seq, scenario);
  return {
    seq,
    at,
    error: null,
    cpu: {
      usage: scenario === "unavailable" || first ? null : round(wave(seq, 19, 4, 78)),
      cores: 16,
    },
    memory: scenario === "unavailable" ? null : { total, used, available: total - used },
    gpus: {
      devices,
      unavailable: devices.length === 0 ? "macOS doesn't report GPU usage to apps." : null,
    },
    disks: disks(scenario),
    network: {
      interfaces: list,
      received: sum((item) => item.received),
      transmitted: sum((item) => item.transmitted),
    },
  };
}

/** `count` samples `intervalMs` apart, the newest at `end`. */
export function fixtureHistory(
  count: number,
  options: { scenario?: StatsScenario; intervalMs?: number; end?: number } = {},
): SystemSample[] {
  const { scenario = "normal", intervalMs = 1000, end = Date.now() } = options;
  return Array.from({ length: count }, (_, i) =>
    fixtureSample(i + 1, end - (count - 1 - i) * intervalMs, scenario),
  );
}

/** A dashboard state to render directly (Storybook, view tests). */
export function fixtureStatsState(
  scenario: StatsScenario = "normal",
  overrides: Partial<SystemStatsState> = {},
): SystemStatsState {
  return {
    status: "live",
    samples: fixtureHistory(60, { scenario, end: Date.UTC(2026, 9, 28, 12) }),
    intervalMs: 1000,
    history: 60,
    error: null,
    ...overrides,
  };
}

/** The API over `scenario`. `live` samples every `intervalMs` while
 * anyone watches; otherwise `emit` sends the next sample by hand. */
export function createSystemStatsFixtureApi({
  scenario = "normal",
  intervalMs = 1000,
  history = 60,
  preload = 20,
  live = false,
  failWatch,
}: {
  scenario?: StatsScenario;
  intervalMs?: number;
  history?: number;
  /** Samples already in the history at the first watch. */
  preload?: number;
  live?: boolean;
  /** Makes watching fail with this message. */
  failWatch?: string;
} = {}) {
  const listeners = new Set<(sample: SystemSample) => void>();
  const watchers = new Set<string>();
  let samples = fixtureHistory(preload, { scenario, intervalMs });
  let seq = samples.length;
  let timer: ReturnType<typeof setInterval> | undefined;
  const calls: { watch: [string, boolean][]; unwatch: string[] } = { watch: [], unwatch: [] };

  const emit = () => {
    seq += 1;
    const sample = fixtureSample(seq, Date.now(), scenario);
    samples = [...samples, sample].slice(-history);
    for (const listener of listeners) listener(sample);
    return sample;
  };
  const api: SystemStatsApi & {
    emit: typeof emit;
    calls: typeof calls;
    watching: () => number;
  } = {
    async watch(watcher, withHistory): Promise<SystemStatsSnapshot> {
      calls.watch.push([watcher, withHistory]);
      if (failWatch) throw new Error(failWatch);
      watchers.add(watcher);
      if (live && !timer) timer = setInterval(emit, intervalMs);
      return { intervalMs, history, samples: withHistory ? samples : [] };
    },
    async unwatch(watcher) {
      calls.unwatch.push(watcher);
      watchers.delete(watcher);
      if (watchers.size === 0 && timer) {
        clearInterval(timer);
        timer = undefined;
      }
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    emit,
    calls,
    watching: () => watchers.size,
  };
  return api;
}
