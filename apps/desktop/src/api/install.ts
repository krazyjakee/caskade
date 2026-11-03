/** Activity view projection. Native answers use opaque backend choice IDs;
 * the UI never submits a plan, executable or arbitrary installation path. */
export type JobStatus = "queued" | "running" | "needs_user" | "completed" | "failed" | "cancelled";
export interface JobChoice {
  id: string;
  label: string;
  detail?: string;
}
export interface JobQuestion {
  id: string;
  kind: string;
  title: string;
  message: string;
  choices: JobChoice[];
  /** Some decisions need an explicit adoption/reinstall flow, not an index answer. */
  canAnswer?: boolean;
}
/** Why a job failed (#158), from the installer's classification. */
export type FailureKind =
  | "missingSource"
  | "missingStaged"
  | "missingOutput"
  | "accessDenied"
  | "locked"
  | "integrity"
  | "storageDisconnected"
  | "diskFull"
  | "download"
  | "other";
export type RecoveryAction =
  | "retry"
  | "redownload"
  | "locateSource"
  | "chooseDestination"
  | "cancel";
export interface JobFailure {
  kind: FailureKind;
  stage: "download" | "inspect" | "install" | "finalize" | "commit";
  path?: string;
  /** Retrying unchanged may work; such failures retry on their own. */
  transient: boolean;
  /** Security software may be (possible) or was (confirmed) involved. */
  security?: "possible" | "confirmed";
  summary: string;
  /** Redacted technical details. */
  detail: string;
  actions: RecoveryAction[];
  occurrences: number;
  /** When an automatic retry is due, Unix milliseconds. */
  retryAtMs?: number;
}
/** What the user chose to do about a failed install or update. Only a
 * path or URL the user picked, or a root ID, crosses the boundary. */
export type JobRecovery =
  | { type: "retry" }
  | { type: "redownload" }
  | { type: "locateSource"; input: string }
  | { type: "chooseDestination"; rootId: string };
export interface JobSnapshot {
  id: string;
  revision: number;
  kind: string;
  status: JobStatus;
  title: string;
  targetDir?: string;
  phase?: string;
  progress?: { completed: number; total?: number; unit: "bytes" | "files" };
  message?: string;
  error?: string;
  warnings: { code: string; message: string }[];
  dependencies: { id: string; name: string; installed: boolean }[];
  strategies: { id: string; label: string; reason: string; selected: boolean }[];
  question?: JobQuestion;
  gameId?: string;
  verification?: { checkedFiles: number; missing: string[]; changed: string[]; valid: boolean };
  failure?: JobFailure;
  /** The storage root the job installs into. */
  rootId?: string;
}
export type InstallInput = { type: "path" | "url"; value: string };
export interface InstallApi {
  listJobs(): Promise<JobSnapshot[]>;
  install(input: InstallInput, rootId?: string): Promise<JobSnapshot>;
  choosePaths(kind: "file" | "folder"): Promise<string[]>;
  cancel(jobId: string): Promise<void>;
  /** Without a strategy, the job runs again as it ran before. */
  retry(jobId: string, strategyId?: string): Promise<JobSnapshot>;
  answer(jobId: string, questionId: string, choiceId: string): Promise<void>;
  /** Recovers a failed or cancelled install or update (#158). */
  recover(jobId: string, recovery: JobRecovery): Promise<JobSnapshot>;
  subscribe(listener: (snapshot: JobSnapshot) => void): Promise<() => void>;
}

/** Older events and a late initial list must never overwrite a newer state. */
export function mergeJobs(current: JobSnapshot[] = [], incoming: JobSnapshot[]): JobSnapshot[] {
  const jobs = new Map(current.map((job) => [job.id, job]));
  for (const job of incoming) {
    const previous = jobs.get(job.id);
    if (!previous || job.revision > previous.revision) jobs.set(job.id, job);
  }
  return [...jobs.values()];
}
export const isActiveJob = (job: JobSnapshot) =>
  job.status === "running" || job.status === "queued" || job.status === "needs_user";
/** A failed or cancelled install, update or copy from another device can
 * run again; a copy goes on from what arrived (#233). */
export const canRetry = (job: JobSnapshot) =>
  (job.status === "failed" || job.status === "cancelled") &&
  (job.kind === "install" || job.kind === "update" || job.kind === "peerCopy");
