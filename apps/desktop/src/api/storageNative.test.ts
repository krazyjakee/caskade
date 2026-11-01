import { afterEach, describe, expect, it, vi } from "vitest";
import type { JobSnapshot_Serialize } from "./bindings";
import { createStorageApi, type StorageTransport } from "./storageNative";

afterEach(() => vi.useRealTimers());
const ok = <T>(data: T) => Promise.resolve({ status: "ok" as const, data });
function job(status: JobSnapshot_Serialize["status"]): JobSnapshot_Serialize {
  return {
    id: "move-job",
    revision: 1,
    kind: "move",
    status,
    title: "Fixture",
    input: "entry-id",
    rootId: "root-id",
    phase: "move",
    progress: null,
    warnings: [],
    question: null,
    gameId: "game-id",
    error: null,
    candidates: [],
    selectedCandidateIndex: null,
    targetDir: null,
    dependencies: [],
    verification: null,
    failure: null,
    createdAtMs: 1,
    updatedAtMs: 1,
  };
}
function transport(initial: JobSnapshot_Serialize): StorageTransport {
  return {
    storageList: async () => ok([]),
    storageAdd: async () => {
      throw new Error("not used");
    },
    storageEdit: async () => {
      throw new Error("not used");
    },
    storageDefault: async () => {
      throw new Error("not used");
    },
    storageRemove: vi.fn(async () => ok(null)),
    storageChooseDirectory: async () => ok(null),
    moveGame: vi.fn(async () => ok(initial)),
    uninstallGame: vi.fn(async () => ok(initial)),
    jobGet: vi.fn(async () => ok(job("completed"))),
    settingsGet: async (namespace) =>
      ok({ namespace, version: 1, revision: 0, value: { alwaysUseDefault: true } }),
  };
}
describe("native storage adapter", () => {
  it("waits for completed jobs before allowing move-all removal to continue", async () => {
    vi.useFakeTimers();
    const native = transport(job("queued"));
    const api = createStorageApi(native);
    let completed = false;
    const moving = api.move("entry-id", "target-root").then(() => {
      completed = true;
    });
    await Promise.resolve();
    expect(completed).toBe(false);
    await vi.advanceTimersByTimeAsync(400);
    await moving;
    expect(native.moveGame).toHaveBeenCalledWith("entry-id", "target-root");
    expect(native.jobGet).toHaveBeenCalledWith("move-job");
    expect(completed).toBe(true);
  });
  it("reports failed/cancelled and needs-user jobs instead of treating them as completed", async () => {
    for (const status of ["failed", "cancelled", "needsUser"] as const) {
      const api = createStorageApi(transport(job(status)));
      await expect(api.move("entry", "root")).rejects.toThrow();
    }
  });
  it("reads validated picker preferences and keeps chooser cancellation explicit", async () => {
    const api = createStorageApi(transport(job("completed")));
    expect(await api.preferences()).toEqual({ alwaysUseDefault: true });
    expect(await api.chooseDirectory()).toBeNull();
    await api.remove("root-id");
  });
});
