import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, expect, it, vi } from "vitest";
import { events, type JobSnapshot_Serialize } from "./bindings";
import { createNativeInstallApi } from "./installNative";

afterEach(() => {
  clearMocks();
  vi.unstubAllGlobals();
});
const job: JobSnapshot_Serialize = {
  id: "job",
  revision: 7,
  kind: "install",
  status: "needsUser",
  title: "Original fixture",
  input: "/fixture/setup.zip",
  rootId: "root",
  phase: "install",
  progress: null,
  warnings: [],
  question: { type: "choose_installer", value: { artifacts: ["source:0", "source:1"] } },
  gameId: null,
  error: null,
  createdAtMs: 0,
  updatedAtMs: 0,
  candidates: [],
  selectedCandidateIndex: null,
  targetDir: "/fixture/Game",
  dependencies: [],
  verification: null,
  failure: null,
};
it("sends only root IDs, revision and numeric choice indexes through generated IPC", async () => {
  vi.stubGlobal("isTauri", true);
  const invoke = vi.fn((command: string) =>
    command === "jobs_list"
      ? [job]
      : command === "choose_install_files"
        ? ["/fixture/a.zip", "/fixture/b.zip"]
        : command === "choose_install_folder"
          ? null
          : job,
  );
  mockIPC(invoke);
  const api = createNativeInstallApi();
  const jobs = await api.listJobs();
  expect(jobs[0]?.question?.choices.map((choice) => choice.id)).toEqual(["0", "1"]);
  await api.answer("job", "7", "1");
  expect(invoke).toHaveBeenCalledWith("job_answer", {
    id: "job",
    revision: 7,
    answer: { type: "select", index: 1 },
  });
  await api.retry("job", "2");
  expect(invoke).toHaveBeenCalledWith("job_retry", { id: "job", candidateIndex: 2 });
  await api.retry("job");
  expect(invoke).toHaveBeenCalledWith("job_retry", { id: "job", candidateIndex: null });
  await api.install({ type: "path", value: "/fixture/setup.zip" }, "root-selected");
  expect(invoke).toHaveBeenCalledWith("install_start", {
    input: "/fixture/setup.zip",
    rootId: "root-selected",
  });
  expect(await api.choosePaths("file")).toEqual(["/fixture/a.zip", "/fixture/b.zip"]);
  expect(await api.choosePaths("folder")).toEqual([]);
  await expect(api.answer("job", "7", "../Game.exe")).rejects.toThrow("choice is invalid");
  await expect(api.answer("job", "NaN", "0")).rejects.toThrow("choice is invalid");
});
it("normalizes full snapshot events and removes the listener on cleanup", async () => {
  vi.stubGlobal("isTauri", true);
  mockIPC(() => null, { shouldMockEvents: true });
  const listener = vi.fn();
  const stop = await createNativeInstallApi().subscribe(listener);
  await events.jobChanged.emit(job);
  expect(listener.mock.calls[0]?.[0].status).toBe("needs_user");
  expect(listener.mock.calls[0]?.[0].revision).toBe(7);
  stop();
  await events.jobChanged.emit({ ...job, revision: 8, status: "cancelled" });
  expect(listener).toHaveBeenCalledOnce();
});
it("retains backend answer errors and refuses an ordinary browser instead of fake success", async () => {
  vi.stubGlobal("isTauri", true);
  mockIPC(() => Promise.reject("Question changed; refresh"));
  await expect(createNativeInstallApi().answer("job", "7", "0")).rejects.toThrow(
    "Question changed",
  );
  vi.stubGlobal("isTauri", false);
  await expect(createNativeInstallApi().listJobs()).rejects.toThrow("Open Caskade desktop");
});
