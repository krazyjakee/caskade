import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { formatBytes } from "../../../api/presentation";
import {
  mergeSamples,
  type SystemStatsApi,
  SystemStatsApiContext,
  statsStatus,
} from "../../../api/systemStats";
import {
  createSystemStatsFixtureApi,
  fixtureHistory,
  fixtureSample,
  fixtureStatsState,
} from "../../../api/systemStatsFixtures";
import { formatRate, niceCeiling } from "./format";
import { StatisticsDashboard } from "./StatisticsDashboard";
import { StatisticsSettings } from "./StatisticsSettings";

afterEach(cleanup);

function show(api: SystemStatsApi) {
  return render(
    <SystemStatsApiContext value={api}>
      <StatisticsSettings />
    </SystemStatsApiContext>,
  );
}

const card = (name: string | RegExp) => screen.getByRole("article", { name });
const status = () => screen.getByRole("status").textContent ?? "";

describe("the Statistics dashboard", () => {
  it("shows every metric of a machine with two GPUs, four volumes and several interfaces", async () => {
    const api = createSystemStatsFixtureApi();
    show(api);
    await screen.findByRole("meter", { name: "CPU usage" });
    expect(api.calls.watch[0]?.[1]).toBe(true);
    expect(status()).toMatch(/Live: updates every second, graphs show the last 60 s/);

    const latest = fixtureSample(20, 0);
    const cpu = screen.getByRole("meter", { name: "CPU usage" });
    expect(cpu.getAttribute("aria-valuenow")).toBe(String(Math.round(latest.cpu.usage ?? -1)));
    expect(within(card("CPU")).getByText("16 threads")).toBeTruthy();
    // Text values beside the gauges, not colour alone.
    const memory = latest.memory;
    expect(within(card("Memory")).getByText(formatBytes(memory?.used ?? 0))).toBeTruthy();

    for (const gpu of latest.gpus.devices) {
      const meter = screen.getByRole("meter", { name: `${gpu.name} usage` });
      expect(meter.getAttribute("aria-valuenow")).toBe(String(Math.round(gpu.usage ?? -1)));
    }
    expect(screen.getAllByRole("article", { name: /Radeon|GeForce/ })).toHaveLength(2);

    const storage = card("Storage");
    const volumes = within(storage).getAllByRole("meter");
    expect(volumes.map((meter) => meter.getAttribute("aria-label"))).toEqual([
      "System (/) space used",
      "Games (/mnt/games) space used",
      "Backup (/mnt/backup) space used",
      "SD Card (/run/media/player/SD Card) space used",
    ]);
    expect(volumes[1]?.getAttribute("aria-valuetext")).toBe(
      "1.8 TiB used of 2.0 TiB, 140 GiB free",
    );
    expect(within(storage).getByText("/, /home")).toBeTruthy();
    expect(within(storage).getByText(/almost out of space/)).toBeTruthy();
    expect(within(storage).getByText("Removable")).toBeTruthy();

    const network = card("Network");
    expect(within(network).getByText(formatRate(latest.network.received))).toBeTruthy();
    const interfaces = network.querySelectorAll('[data-part="interface"]');
    expect([...interfaces].map((item) => item.getAttribute("data-kind"))).toEqual([
      "physical",
      "physical",
      "virtual",
      "loopback",
    ]);
    expect(within(network).getByText("Virtual, not counted")).toBeTruthy();
    // Graphs carry their own summary.
    expect(
      screen.getByRole("img", {
        name: /^CPU usage, last 60 s\. Usage: now \d+%, peak \d+%, low \d+%, 1 missed\.$/,
      }),
    ).toBeTruthy();

    // A new sample moves everything on.
    let next = latest;
    act(() => {
      next = api.emit();
    });
    await waitFor(() =>
      expect(screen.getByRole("meter", { name: "CPU usage" }).getAttribute("aria-valuenow")).toBe(
        String(Math.round(next.cpu.usage ?? -1)),
      ),
    );
  });

  it("says what isn't available instead of showing zeros", async () => {
    show(createSystemStatsFixtureApi({ scenario: "unavailable" }));
    await screen.findByRole("meter", { name: "CPU usage" });
    for (const meter of screen.getAllByRole("meter")) {
      expect(meter.getAttribute("aria-valuenow")).toBeNull();
      expect(meter.getAttribute("aria-valuetext")).toBe("No reading");
    }
    expect(within(card("GPU")).getByText("macOS doesn't report GPU usage to apps.")).toBeTruthy();
    expect(within(card("Storage")).getByText("No drives found.")).toBeTruthy();
    expect(within(card("Network")).getByText("No network interfaces found.")).toBeTruthy();
    expect(within(card("Memory")).getByText(/couldn't be read/)).toBeTruthy();
  });

  it("shows a failed sample as a gap while storage keeps its last reading", async () => {
    // The 17th sample of `failing` couldn't be read.
    show(createSystemStatsFixtureApi({ scenario: "failing", preload: 17 }));
    await screen.findByRole("meter", { name: "CPU usage" });
    expect(within(card("CPU")).getByText("This sample couldn't be read.")).toBeTruthy();
    expect(
      screen.getByRole("meter", { name: "CPU usage" }).getAttribute("aria-valuenow"),
    ).toBeNull();
    expect(within(card("Storage")).getAllByRole("meter")).toHaveLength(3);
    // Seq 1 has no measurement yet; 3, 10 and 17 failed.
    expect(screen.getByRole("img", { name: /^CPU usage, .*, 4 missed\.$/ })).toBeTruthy();
    cleanup();

    render(<StatisticsDashboard stats={fixtureStatsState("failing")} />);
    expect(
      within(card("Intel GPU")).getByText("Intel's driver doesn't report GPU usage to apps."),
    ).toBeTruthy();
    expect(screen.queryByRole("img", { name: /^Intel GPU usage/ })).toBeNull();
  });

  it("says when watching fails, and when samples stop coming", async () => {
    show(createSystemStatsFixtureApi({ failWatch: "The sampler isn't running." }));
    await waitFor(() =>
      expect(status()).toBe(
        "Statistics aren't available: The sampler isn't running. Trying again.",
      ),
    );
    expect(screen.queryByRole("article")).toBeNull();
    cleanup();

    const api = createSystemStatsFixtureApi({ intervalMs: 20, preload: 0 });
    show(api);
    expect(status()).toBe("Waiting for the first reading…");
    await waitFor(() => expect(api.watching()).toBe(1));
    act(() => void api.emit());
    await waitFor(() => expect(status()).toMatch(/^Live/));
    await waitFor(() => expect(status()).toMatch(/^Not updating: the last reading is from/));
  });

  it("watches only while open and visible", async () => {
    const api = createSystemStatsFixtureApi();
    const view = show(api);
    await waitFor(() => expect(api.watching()).toBe(1));
    Object.defineProperty(document, "visibilityState", { configurable: true, value: "hidden" });
    document.dispatchEvent(new Event("visibilitychange"));
    await waitFor(() => expect(api.watching()).toBe(0));
    Object.defineProperty(document, "visibilityState", { configurable: true, value: "visible" });
    document.dispatchEvent(new Event("visibilitychange"));
    await waitFor(() => expect(api.watching()).toBe(1));
    expect(api.calls.watch.at(-1)?.[1]).toBe(true);
    Reflect.deleteProperty(document, "visibilityState");
    view.unmount();
    await waitFor(() => expect(api.watching()).toBe(0));
  });

  it("renders the same structure for every status", () => {
    for (const status of ["loading", "live", "stale", "error"] as const) {
      const state = fixtureStatsState("normal", {
        status,
        ...(status === "loading" || status === "error" ? { samples: [] } : {}),
        ...(status === "error" ? { error: "Denied." } : {}),
      });
      const { container } = render(<StatisticsDashboard stats={state} />);
      const root = container.querySelector('[data-cs="system-stats"]');
      expect(root?.getAttribute("data-status")).toBe(status);
      expect(root?.querySelectorAll('[data-part="card"]').length).toBe(
        state.samples.length ? 6 : 0,
      );
      cleanup();
    }
  });
});

describe("the dashboard's data", () => {
  it("merges samples once each, oldest first, within the history", () => {
    const history = fixtureHistory(5, { end: 10_000 });
    const merged = mergeSamples(history.slice(2), [...history.slice(0, 3), ...history.slice(3)], 4);
    expect(merged.map((sample) => sample.seq)).toEqual([2, 3, 4, 5]);
    expect(mergeSamples(history, [], 4)).toBe(history);
  });

  it("is loading, live, stale or failed", () => {
    const base = { samples: fixtureHistory(2), intervalMs: 1000, history: 60, error: null };
    expect(statsStatus({ ...base, samples: [] }, null, 0)).toBe("loading");
    expect(statsStatus({ ...base, samples: [], error: "x" }, null, 0)).toBe("error");
    expect(statsStatus(base, 10_000, 12_000)).toBe("live");
    expect(statsStatus(base, 10_000, 13_001)).toBe("stale");
    expect(statsStatus({ ...base, error: "x" }, 10_000, 10_500)).toBe("stale");
  });

  it("rounds rate graphs to a readable top", () => {
    expect(niceCeiling(0)).toBe(1024);
    expect(niceCeiling(1_300)).toBe(2_000);
    expect(niceCeiling(9_500_000)).toBe(10_000_000);
  });
});
