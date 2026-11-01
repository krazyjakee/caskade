import { t } from "@lingui/core/macro";
import { isTauri } from "@tauri-apps/api/core";
import {
  type BiosFetch_Serialize,
  commands,
  events,
  type Installable_Serialize,
  type ManagedEmulator_Serialize,
  type MissingPlatform_Serialize,
  type SetupJob_Serialize,
} from "./bindings";
import { formatSize } from "./presentation";
import { unwrap } from "./reply";

/** One-click emulator installs (EMULATOR-SETUP-PLAN §7). */
export type Installable = Installable_Serialize;
export type MissingPlatform = MissingPlatform_Serialize;
export type SetupJob = SetupJob_Serialize;
export type ManagedEmulator = ManagedEmulator_Serialize;
export type BiosFetch = BiosFetch_Serialize;

export interface EmulatorSetupApi {
  /** Every emulator Caskade can install on this system. */
  installable(): Promise<Installable[]>;
  /** Platforms with games that no emulator runs yet. */
  missing(): Promise<MissingPlatform[]>;
  install(definitionId: string): Promise<SetupJob>;
  /** Installs the platform's recommended emulator. */
  installFor(platformId: string): Promise<SetupJob>;
  jobs(): Promise<SetupJob[]>;
  cancel(jobId: string): Promise<SetupJob>;
  /** Looks for a newer version of a Caskade-installed emulator now. */
  check(emulatorId: string): Promise<ManagedEmulator>;
  /** Stops offering the newest version found. */
  skip(emulatorId: string): Promise<ManagedEmulator>;
  /** Installs the newest version, keeping the user's files. */
  update(emulatorId: string): Promise<SetupJob>;
  /** Fetches a platform's missing BIOS files from RomM. */
  fetchBios(platformId: string): Promise<BiosFetch>;
  subscribe(listener: (job: SetupJob) => void): Promise<() => void>;
}

export const setupRunning = (job: SetupJob) =>
  job.status !== "done" && job.status !== "failed" && job.status !== "cancelled";

/** What a job is doing, in a few words. */
export function setupProgress(job: SetupJob): string {
  switch (job.status) {
    case "queued":
      return t`Waiting…`;
    case "downloading": {
      if (job.total)
        return t`Downloading ${{ done: formatSize(job.downloaded) }} of ${{ total: formatSize(job.total) }}`;
      return job.downloaded
        ? t`Downloading ${{ done: formatSize(job.downloaded) }}`
        : t`Downloading…`;
    }
    case "unpacking":
      return t`Unpacking…`;
    case "configuring":
      return t`Setting up…`;
    case "done":
      return job.version
        ? t`Installed ${{ version: job.version.split("@")[0] ?? job.version }}`
        : t`Installed`;
    case "failed":
      return job.error ?? t`The install failed`;
    case "cancelled":
      return t`Cancelled`;
  }
}

export function createNativeEmulatorSetupApi(): EmulatorSetupApi {
  return {
    installable: () => unwrap(commands.emulatorSetupInstallable()),
    missing: () => unwrap(commands.emulatorSetupMissing()),
    install: (definitionId) => unwrap(commands.emulatorSetupInstall(definitionId)),
    installFor: (platformId) => unwrap(commands.emulatorSetupInstallFor(platformId)),
    jobs: () => commands.emulatorSetupJobs(),
    cancel: (jobId) => unwrap(commands.emulatorSetupCancel(jobId)),
    check: (emulatorId) => unwrap(commands.emulatorSetupCheck(emulatorId)),
    skip: (emulatorId) => unwrap(commands.emulatorSetupSkip(emulatorId)),
    update: (emulatorId) => unwrap(commands.emulatorSetupUpdate(emulatorId)),
    fetchBios: (platformId) => unwrap(commands.emulatorSetupFetchBios(platformId)),
    subscribe: async (listener) =>
      isTauri()
        ? events.emulationSetupChanged.listen(({ payload }) => listener(payload))
        : () => {},
  };
}
