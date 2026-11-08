import { t } from "@lingui/core/macro";
import { isTauri } from "@tauri-apps/api/core";
import { commands, events } from "./bindings";
import type { InstallApi, JobRecovery } from "./install";
import { mapJob, type RawJobSnapshot } from "./installMapping";
import { type Reply, unwrap } from "./reply";

export interface InstallTransport {
  jobsList(): Promise<RawJobSnapshot[]>;
  installStart(input: string, rootId: string | null): Promise<Reply<RawJobSnapshot>>;
  jobCancel(id: string): Promise<Reply<RawJobSnapshot>>;
  jobRemove(id: string): Promise<Reply<null>>;
  jobRetry(id: string, candidateIndex: number | null): Promise<Reply<RawJobSnapshot>>;
  jobRecover(id: string, recovery: JobRecovery): Promise<Reply<RawJobSnapshot>>;
  jobAnswer(
    id: string,
    revision: number,
    answer: { type: "select"; index: number },
  ): Promise<Reply<RawJobSnapshot>>;
  chooseInstallFiles(): Promise<Reply<string[]>>;
  chooseInstallFolder(): Promise<Reply<string | null>>;
}
function index(value: string): number {
  const number = Number(value);
  if (!Number.isInteger(number) || number < 0 || number > 0xffffffff || String(number) !== value)
    throw new Error(t`The job choice is invalid. Refresh Activity.`);
  return number;
}
/** Only an index/revision crosses the answer boundary. Plans and paths remain
 * server-held; the core checks revisions and bounds again under its job lock. */
export function createInstallApi(
  transport: InstallTransport,
  subscribe: (listener: (snapshot: RawJobSnapshot) => void) => Promise<() => void>,
): InstallApi {
  return {
    async listJobs() {
      return (await transport.jobsList()).map(mapJob);
    },
    async install(input, rootId) {
      return mapJob(await unwrap(transport.installStart(input.value, rootId ?? null)));
    },
    async choosePaths(kind) {
      if (kind === "file") return unwrap(transport.chooseInstallFiles());
      const folder = await unwrap(transport.chooseInstallFolder());
      return folder ? [folder] : [];
    },
    async cancel(id) {
      await unwrap(transport.jobCancel(id));
    },
    async remove(id) {
      await unwrap(transport.jobRemove(id));
    },
    async retry(id, strategyId) {
      const candidate = strategyId === undefined ? null : index(strategyId);
      return mapJob(await unwrap(transport.jobRetry(id, candidate)));
    },
    async recover(id, recovery) {
      return mapJob(await unwrap(transport.jobRecover(id, recovery)));
    },
    async answer(id, questionId, choiceId) {
      await unwrap(
        transport.jobAnswer(id, index(questionId), { type: "select", index: index(choiceId) }),
      );
    },
    subscribe: (listener) => subscribe((snapshot) => listener(mapJob(snapshot))),
  };
}
export function createNativeInstallApi(): InstallApi {
  const check = () => {
    if (!isTauri()) throw new Error("Open Caskade desktop to manage installations.");
  };
  return createInstallApi(
    {
      jobsList: () => {
        check();
        return commands.jobsList();
      },
      installStart: (input, rootId) => {
        check();
        return commands.installStart(input, rootId);
      },
      jobCancel: (id) => {
        check();
        return commands.jobCancel(id);
      },
      jobRemove: (id) => {
        check();
        return commands.jobRemove(id);
      },
      jobRetry: (id, index) => {
        check();
        return commands.jobRetry(id, index);
      },
      jobRecover: (id, recovery) => {
        check();
        return commands.jobRecover(id, recovery);
      },
      jobAnswer: (id, revision, answer) => {
        check();
        return commands.jobAnswer(id, revision, answer);
      },
      chooseInstallFiles: () => {
        check();
        return commands.chooseInstallFiles();
      },
      chooseInstallFolder: () => {
        check();
        return commands.chooseInstallFolder();
      },
    },
    async (listener) =>
      isTauri() ? events.jobChanged.listen(({ payload }) => listener(payload)) : () => {},
  );
}
export async function connectInstallApi(): Promise<InstallApi> {
  if (import.meta.env.DEV && import.meta.env.VITE_CASKADE_FIXTURES === "1") {
    const { createInstallFixtureApi } = await import("./installFixtures");
    return createInstallFixtureApi();
  }
  return createNativeInstallApi();
}
