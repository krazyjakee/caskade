import type { InstallApi, JobSnapshot } from "./install";

/** Original synthetic jobs, imported only by tests or the explicit DEV switch. */
export function fixtureJob(overrides: Partial<JobSnapshot> = {}): JobSnapshot {
  return {
    id: "fixture-job",
    revision: 1,
    kind: "install",
    status: "running",
    title: "Fixture installation",
    phase: "install",
    targetDir: "/fixture/Games/FixtureGame",
    message: "Extracting files",
    progress: { completed: 40, total: 100, unit: "bytes" },
    warnings: [{ code: "scripts_skipped", message: "Package scripts were skipped." }],
    dependencies: [{ id: "runtime", name: "Fixture runtime", installed: false }],
    strategies: [
      {
        id: "0",
        label: "Extract archive",
        reason: "Confined extraction; no installer code runs.",
        selected: true,
      },
      {
        id: "1",
        label: "Run installer",
        reason: "Run the installer with known switches.",
        selected: false,
      },
    ],
    ...overrides,
  };
}
export function createInstallFixtureApi(
  initial: JobSnapshot[] = [fixtureJob()],
): InstallApi & { emit(job: JobSnapshot): void } {
  let jobs = initial;
  const listeners = new Set<(job: JobSnapshot) => void>();
  const emit = (job: JobSnapshot) => {
    jobs = [...jobs.filter((item) => item.id !== job.id), job];
    for (const listener of listeners) listener(job);
  };
  const find = (id: string) => {
    const job = jobs.find((job) => job.id === id);
    if (!job) throw new Error("Unknown fixture job");
    return job;
  };
  return {
    emit,
    async listJobs() {
      return jobs;
    },
    async install(input) {
      const job = fixtureJob({
        id: `fixture-${jobs.length + 1}`,
        title: input.value,
        status: "queued",
      });
      emit(job);
      return job;
    },
    async choosePaths(kind) {
      return [kind === "file" ? "/fixture/setup.zip" : "/fixture/portable"];
    },
    async cancel(id) {
      const job = find(id);
      emit({ ...job, revision: job.revision + 1, status: "cancelled", question: undefined });
    },
    async remove(id) {
      find(id);
      jobs = jobs.filter((job) => job.id !== id);
    },
    async retry(id, strategyId) {
      const job = find(id);
      const next = {
        ...job,
        revision: job.revision + 1,
        status: "queued" as const,
        error: undefined,
        strategies:
          strategyId === undefined
            ? job.strategies
            : job.strategies.map((strategy) => ({
                ...strategy,
                selected: strategy.id === strategyId,
              })),
      };
      emit(next);
      return next;
    },
    async recover(id, recovery) {
      const job = find(id);
      const next = {
        ...job,
        revision: job.revision + 1,
        status: "queued" as const,
        error: undefined,
        failure: undefined,
        rootId: recovery.type === "chooseDestination" ? recovery.rootId : job.rootId,
      };
      emit(next);
      return next;
    },
    async answer(id, questionId, choiceId) {
      const job = find(id);
      if (
        job.question?.id !== questionId ||
        !job.question.choices.some((choice) => choice.id === choiceId)
      )
        throw new Error("The question changed. Review the latest choices.");
      emit({ ...job, revision: job.revision + 1, status: "running", question: undefined });
    },
    async subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}
