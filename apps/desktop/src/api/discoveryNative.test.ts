import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, expect, it, vi } from "vitest";
import { type DiscoverySnapshot_Serialize, events } from "./bindings";
import { nativeDiscoveryApi } from "./discoveryNative";

afterEach(() => {
  clearMocks();
  vi.unstubAllGlobals();
});
const snapshot: DiscoverySnapshot_Serialize = {
  revision: 7,
  os: "windows",
  phase: "complete",
  activeSources: [],
  checks: [{ id: "registry", label: "Registry", sources: ["uninstall"] }],
  launchers: [],
  sources: [
    {
      source: "uninstall",
      count: 1,
      duration_ms: 12,
      warnings: ["Fixture warning"],
      error: { type: "timed_out" },
    },
  ],
  found: 1,
  imported: 0,
  review: [
    {
      id: "r1",
      title: "Possible original game",
      sources: ["uninstall"],
      location: null,
      evidence: ["fixture.rule: +20"],
    },
  ],
  warnings: [],
  error: null,
  watching: false,
  storage: null,
};
it("uses generated discovery commands, backend platform and revision-checked retained IDs", async () => {
  vi.stubGlobal("isTauri", true);
  const invoke = vi.fn((_command: string) => snapshot);
  mockIPC(invoke);
  const result = await nativeDiscoveryApi.status();
  expect(result.os).toBe("windows");
  expect(result.sources[0]?.durationMs).toBe(12);
  expect(result.sources[0]?.error).toContain("timed_out");
  await nativeDiscoveryApi.review(["r1"], true, 7);
  expect(invoke).toHaveBeenCalledWith("discovery_review", {
    ids: ["r1"],
    accept: true,
    revision: 7,
  });
  await nativeDiscoveryApi.scan();
  await nativeDiscoveryApi.cancel();
  await nativeDiscoveryApi.launchers();
  expect(invoke.mock.calls.map(([command]) => command)).toEqual([
    "discovery_status",
    "discovery_review",
    "discovery_scan",
    "discovery_cancel",
    "discovery_launchers",
  ]);
});
it("normalizes typed events and unregisters the listener", async () => {
  vi.stubGlobal("isTauri", true);
  mockIPC(() => null, { shouldMockEvents: true });
  const listener = vi.fn();
  const stop = await nativeDiscoveryApi.subscribe(listener);
  await events.discoveryChanged.emit({ ...snapshot, phase: "scanning", activeSources: ["steam"] });
  expect(listener.mock.calls[0]?.[0].sources[0].durationMs).toBe(12);
  expect(listener.mock.calls[0]?.[0].activeSources).toEqual(["steam"]);
  stop();
  await events.discoveryChanged.emit({ ...snapshot, revision: 8 });
  expect(listener).toHaveBeenCalledOnce();
});
it("surfaces stale decisions and ordinary-browser errors without synthetic success", async () => {
  vi.stubGlobal("isTauri", true);
  mockIPC(() => Promise.reject("Discovery changed. Review the latest list."));
  await expect(nativeDiscoveryApi.review(["r1"], false, 7)).rejects.toThrow("Discovery changed");
  vi.stubGlobal("isTauri", false);
  await expect(nativeDiscoveryApi.scan()).rejects.toThrow("Open Caskade desktop");
});
