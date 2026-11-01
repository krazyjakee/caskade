import type { JobFailure, JobQuestion, JobSnapshot } from "./install";
import { formatSize } from "./presentation";

/** Structural subset of the generated installer DTO. Native transport is
 * checked against this boundary; extra backend execution fields stay private. */
export interface RawCandidate {
  handler: string;
  strategy: string;
  confidence: number | null;
  fidelity: string;
  requires: { executes_code: boolean; may_prompt_elevation: boolean };
}
export type RawQuestion =
  | {
      type: "choose_launch_target";
      value: { targets: { path: string; args: string[]; reason: string }[] };
    }
  | { type: "choose_installer"; value: { artifacts: string[] } }
  | {
      type: "choose_asset";
      value: { assets: { name: string; url: string; size: number | null }[] };
    }
  | { type: "choose_install_directory"; value: { paths: string[]; reason: string } }
  | { type: "reinstall_after_move"; value: { title: string; reason: string } }
  | { type: "low_confidence" | "confirm_execution"; value: { candidates: RawCandidate[] } }
  | {
      type: "choose_destination";
      value: {
        reason: string;
        options: { label: string; folder: string | null; copy: boolean }[];
      };
    };
export interface RawFailure {
  kind: JobFailure["kind"];
  stage: JobFailure["stage"];
  path: string | null;
  transient: boolean;
  security: JobFailure["security"] | null;
  summary: string;
  detail: string;
  actions: JobFailure["actions"];
  occurrences: number;
  retryAtMs: number | null;
}
export interface RawJobSnapshot {
  id: string;
  revision: number;
  kind: string;
  status:
    | "queued"
    | "downloading"
    | "installing"
    | "needsUser"
    | "completed"
    | "failed"
    | "cancelled";
  title: string;
  input: string;
  phase: string | null;
  progress: {
    bytes: number;
    totalBytes: number | null;
    files: number;
    etaSecs: number | null;
    message: string;
  } | null;
  warnings: { code: string; message: string }[];
  question: RawQuestion | null;
  gameId: string | null;
  error: string | null;
  targetDir?: string | null;
  candidates?: RawCandidate[];
  selectedCandidateIndex?: number | null;
  dependencies?: { id: string; name: string; installed: boolean }[];
  verification?: {
    checkedFiles: number;
    missing: string[];
    changed: string[];
    valid: boolean;
  } | null;
  failure?: RawFailure | null;
  rootId?: string | null;
}
function mapFailure(failure: RawFailure): JobFailure {
  return {
    ...failure,
    path: failure.path ?? undefined,
    security: failure.security ?? undefined,
    retryAtMs: failure.retryAtMs ?? undefined,
  };
}
const readable = (value: string) => value.replaceAll("_", " ");
function candidateLabel(candidate: RawCandidate) {
  return `${readable(candidate.strategy)} · ${candidate.handler}`;
}
function candidateReason(candidate: RawCandidate) {
  return `${candidate.confidence === null ? "Unknown confidence" : `${Math.round(candidate.confidence * 100)}% confidence`} · ${candidate.fidelity} · ${candidate.requires.executes_code ? "Runs installer code" : "Does not run installer code"}${candidate.requires.may_prompt_elevation ? " · May request elevation" : ""}`;
}
export function mapJobQuestion(question: RawQuestion, revision: number): JobQuestion {
  const base = { id: String(revision), kind: question.type };
  switch (question.type) {
    case "choose_launch_target":
      return {
        ...base,
        title: "Choose a launch target",
        message: "Several possible launch targets were found. Choose the game you want to launch.",
        choices: question.value.targets.map((target, index) => ({
          id: String(index),
          label: target.path,
          detail: `${target.reason}${target.args.length ? ` · Arguments: ${JSON.stringify(target.args)}` : ""}`,
        })),
      };
    case "choose_installer":
      return {
        ...base,
        title: "Choose an installer",
        message: "Several installers were found in this input.",
        choices: question.value.artifacts.map((artifact, index) => ({
          id: String(index),
          label: artifact,
        })),
      };
    case "choose_asset": {
      // A torrent's payloads (#159) are magnet links: the link says nothing
      // the name and size don't.
      const torrent = question.value.assets.some((asset) => asset.url.startsWith("magnet:"));
      const size = (bytes: number | null) => (bytes !== null ? formatSize(bytes) : "");
      return {
        ...base,
        title: torrent ? "Choose what to download" : "Choose a download asset",
        message: torrent
          ? "This torrent holds more than one game or installer. Choose the one to download and install."
          : "Choose the release file to download and install.",
        choices: question.value.assets.map((asset, index) => ({
          id: String(index),
          label: asset.name,
          detail: torrent
            ? size(asset.size)
            : `${asset.url}${asset.size !== null ? ` · ${size(asset.size)}` : ""}`,
        })),
      };
    }
    case "choose_install_directory":
      return {
        ...base,
        canAnswer: false,
        title: "Choose the installed folder",
        message: `${question.value.reason} This folder requires explicit adoption or a new installation. It will not be moved automatically.`,
        choices: question.value.paths.map((path, index) => ({ id: String(index), label: path })),
      };
    case "reinstall_after_move":
      return {
        ...base,
        canAnswer: false,
        title: `Reinstall ${question.value.title}?`,
        message: `${question.value.reason} Start a new installation from the retained source after reviewing the moved game.`,
        choices: [{ id: "0", label: "Reinstall from the retained source" }],
      };
    case "choose_destination":
      return {
        ...base,
        title: "Where should these ROMs go?",
        message: question.value.reason,
        choices: question.value.options.map((option, index) => ({
          id: String(index),
          label: option.label,
          detail: option.folder ?? "Imported from where they are now",
        })),
      };
    case "low_confidence":
    case "confirm_execution":
      return {
        ...base,
        title:
          question.type === "low_confidence"
            ? "Choose an installation strategy"
            : "Run the installer program?",
        message:
          question.type === "low_confidence"
            ? "The installer could not confidently select a strategy. Review its requirements before continuing."
            : "Installing this game means running its installer program on this computer, with your account's access to your files. Run it only if you trust where it came from, or choose a way that doesn't run it.",
        choices: question.value.candidates.map((candidate, index) => ({
          id: String(index),
          label: candidateLabel(candidate),
          detail: candidateReason(candidate),
        })),
      };
  }
}
export function mapJob(job: RawJobSnapshot): JobSnapshot {
  const candidates =
    job.candidates ??
    (job.question?.type === "low_confidence" || job.question?.type === "confirm_execution"
      ? job.question.value.candidates
      : []);
  const progress = job.progress;
  return {
    id: job.id,
    revision: job.revision,
    kind: job.kind,
    title: job.title,
    status:
      job.status === "needsUser"
        ? "needs_user"
        : job.status === "installing" || job.status === "downloading"
          ? "running"
          : job.status,
    phase: job.phase ?? undefined,
    targetDir: job.targetDir ?? undefined,
    message: progress?.message,
    progress: progress
      ? {
          completed:
            progress.totalBytes !== null || progress.bytes > 0 ? progress.bytes : progress.files,
          total: progress.totalBytes ?? undefined,
          unit: progress.totalBytes !== null || progress.bytes > 0 ? "bytes" : "files",
        }
      : undefined,
    warnings: job.warnings,
    dependencies: job.dependencies ?? [],
    strategies: candidates.map((candidate, index) => ({
      id: String(index),
      label: candidateLabel(candidate),
      reason: candidateReason(candidate),
      selected: job.selectedCandidateIndex === index,
    })),
    question: job.question ? mapJobQuestion(job.question, job.revision) : undefined,
    gameId: job.gameId ?? undefined,
    error: job.error ?? undefined,
    verification: job.verification ?? undefined,
    failure: job.failure ? mapFailure(job.failure) : undefined,
    rootId: job.rootId ?? undefined,
  };
}
