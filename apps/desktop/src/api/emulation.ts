/** The view contract for Settings → Emulators. Native adapters project generated
 * service DTOs; neither detection nor command resolution is performed in React. */

import { t } from "@lingui/core/macro";
import type {
  EmulatorInstall,
  EmulatorTarget,
  ReviewChoice,
  ReviewGame_Serialize,
  RomFolderSetup_Serialize,
  RomScanner_Serialize,
  ScanJob_Serialize,
  ScanMode,
  ScannerView_Serialize,
  ScanSummary_Serialize,
  Tracking,
} from "./bindings";
import type { EmulatorSetupApi } from "./emulatorSetup";

export type { EmulatorInstall, EmulatorTarget, ReviewChoice, ScanMode };
/** ROM scanners (#119) cross unchanged from the service. */
export type RomScanner = RomScanner_Serialize;
export type ScannerRow = ScannerView_Serialize;
export type ScanJob = ScanJob_Serialize;
export type ReviewGame = ReviewGame_Serialize;
export type ScanSummary = ScanSummary_Serialize;
export type RomFolderSetup = RomFolderSetup_Serialize;
export type ArgumentsOverride =
  | { type: "default" }
  | { type: "replace"; args: string[]; replaced?: string[] }
  | { type: "append"; args: string[] };
export type ProfileTracking = Tracking;
export interface ProfileHooks {
  before: string[] | null;
  afterExit: string[] | null;
}
export type EmulatorProfile =
  | {
      type: "built_in";
      id: string;
      definitionProfile: string;
      args: ArgumentsOverride;
      hooks: ProfileHooks;
    }
  | {
      type: "custom";
      id: string;
      name: string;
      executable: string | null;
      args: string[];
      workingDir: string | null;
      platforms: string[];
      extensions: string[];
      env: [string, string][];
      tracking: ProfileTracking;
      hooks: ProfileHooks;
    };
export interface BiosStatus {
  file: string;
  required: boolean;
  found: boolean | null;
  /** `wrong_hash`: a file is there, but not the dump the definition names. */
  state?: "found" | "missing" | "wrong_hash" | null;
}
export interface DefinitionProfile {
  name: string;
  platforms: string[];
  extensions: string[];
  args: string[];
  requires: string[];
}
export interface EmulatorDefinition {
  id: string;
  name: string;
  website: string | null;
  profiles: DefinitionProfile[];
  vars: string[];
}
export interface ArgvPreview {
  program: string;
  args: string[];
  cwd: string | null;
  sampleRom: string;
  error: string | null;
}
export interface ResolvedProfile extends ArgvPreview {
  name: string;
  platforms: string[];
  extensions: string[];
  available: boolean;
  missing: string[];
  bios: BiosStatus[];
  /** A Replace override whose definition args changed since it was saved. */
  drift: { overriddenFrom: string[]; definitionArgs: string[] } | null;
}
export interface ConfiguredEmulator {
  id: string;
  name: string;
  definition: string | null;
  install: EmulatorInstall;
  found: boolean;
  version: string | null;
  profiles: { config: EmulatorProfile; resolved: ResolvedProfile }[];
  /** Availability varies by install, never by global definition alone. */
  definitionProfiles: { name: string; available: boolean; missing: string[] }[];
}
export interface ProfileRef {
  emulatorId: string;
  profileId: string;
}
export interface EmulationPlatform {
  id: string;
  name: string;
  short: string;
  extensions: string[];
  romCount: number;
  bios: BiosStatus[];
  default: ProfileRef | null;
  biosDir: string | null;
}
export interface EmulationSnapshot {
  emulators: ConfiguredEmulator[];
  definitions: EmulatorDefinition[];
  platforms: EmulationPlatform[];
  variables: { name: string; description: string }[];
  stats: {
    emulators: number;
    workingPlatforms: number;
    platforms: number;
    roms: number;
    biosIssues: number | null;
  };
}
export interface EmulatorEdit {
  id: string | null;
  name: string;
  definition: string | null;
  install: EmulatorInstall;
}
/** Text is parsed and validated by the backend before changing configuration.
 * A failed preview/save leaves the editable draft intact. */
export interface ProfileEdit {
  profile: EmulatorProfile;
  argsText: string;
  beforeText: string;
  afterExitText: string;
}
export interface DetectionCandidate {
  id: string;
  definition: string;
  name: string;
  install: EmulatorInstall;
  version: string | null;
  configuredAs: string | null;
  profiles: { name: string; available: boolean; missing: string[] }[];
}
export interface EmulationDetection {
  id: string | null;
  revision: number;
  status: "idle" | "searching" | "completed" | "cancelled" | "failed";
  folder: string | null;
  candidates: DetectionCandidate[];
  warnings: string[];
  error: string | null;
  message: string;
}
export interface EmulationApi {
  snapshot(): Promise<EmulationSnapshot>;
  saveEmulator(input: EmulatorEdit): Promise<EmulationSnapshot>;
  removeEmulator(id: string): Promise<EmulationSnapshot>;
  saveProfile(emulatorId: string, input: ProfileEdit): Promise<EmulationSnapshot>;
  removeProfile(emulatorId: string, profileId: string): Promise<EmulationSnapshot>;
  setPlatform(
    platform: string,
    selected: ProfileRef | null,
    biosDir: string | null,
  ): Promise<EmulationSnapshot>;
  preview(emulatorId: string, input: ProfileEdit): Promise<ArgvPreview>;
  detection(): Promise<EmulationDetection>;
  detect(folder: string | null): Promise<EmulationDetection>;
  cancelDetection(): Promise<EmulationDetection>;
  importDetected(ids: string[], revision: number): Promise<EmulationSnapshot>;
  choosePath(kind: "folder" | "file"): Promise<string | null>;
  scanners(): Promise<ScannerRow[]>;
  /** Creates (empty id) or updates a scanner. */
  saveScanner(scanner: RomScanner): Promise<RomScanner>;
  removeScanner(id: string): Promise<void>;
  /** `review` stops at a review list; `import` imports straight away. */
  startScan(scannerId: string, mode: ScanMode): Promise<ScanJob>;
  scanJobs(): Promise<ScanJob[]>;
  cancelScan(id: string): Promise<ScanJob>;
  submitReview(id: string, revision: number, choices: ReviewChoice[]): Promise<ScanJob>;
  /** Every scanner that takes part in library updates imports now. */
  scanAll(): Promise<ScanSummary[]>;
  /** Gives each console folder in a ROM collection (`snes/`, `psx/`, …) a
   * scanner with an emulator that runs it. Nothing is scanned yet. */
  setupRomFolder(folder: string): Promise<RomFolderSetup>;
  /** Exclusions every scanner applies (EMULATION-PLAN §6.3, #135). */
  globalExclusions(): Promise<string[]>;
  /** Replaces them; returns them as stored. The next scan uses them. */
  setGlobalExclusions(patterns: string[]): Promise<string[]>;
  subscribe(listener: () => void): Promise<() => void>;
  subscribeDetection(listener: (snapshot: EmulationDetection) => void): Promise<() => void>;
  subscribeScans(listener: (job: ScanJob) => void): Promise<() => void>;
  /** One-click installs; absent where Caskade can't install emulators. */
  setup?: EmulatorSetupApi;
}

/** Display/edit quoting only, for tokens that need it. Parsing and execution
 * stay in the Rust service. */
export const argvText = (args: string[]) =>
  args
    .map((arg) =>
      arg && !/[\s"'\\]/.test(arg)
        ? arg
        : `"${arg.replaceAll("\\", "\\\\").replaceAll('"', '\\"')}"`,
    )
    .join(" ");
export const installKinds = [
  { id: "folder", label: "Folder" },
  { id: "path", label: "$PATH binary" },
  { id: "flatpak", label: "Flatpak" },
  { id: "app_image", label: "AppImage" },
  { id: "app_bundle", label: "App bundle" },
] as const satisfies { id: EmulatorInstall["type"]; label: string }[];
export const installKindLabel = (install: EmulatorInstall) => {
  switch (install.type) {
    case "folder":
      return t`Folder`;
    case "path":
      return t`$PATH binary`;
    case "app_bundle":
      return t`App bundle`;
    default:
      return installKinds.find((kind) => kind.id === install.type)?.label ?? install.type;
  }
};
export function profileEdit(profile: EmulatorProfile): ProfileEdit {
  return {
    profile: structuredClone(profile),
    argsText: argvText(
      profile.type === "custom"
        ? profile.args
        : profile.args.type === "default"
          ? []
          : profile.args.args,
    ),
    beforeText: argvText(profile.hooks.before ?? []),
    afterExitText: argvText(profile.hooks.afterExit ?? []),
  };
}
export function installLocation(install: EmulatorInstall): string {
  switch (install.type) {
    case "folder":
      return install.dir;
    case "path":
      return install.binary;
    case "flatpak":
      return install.appId;
    case "app_image":
      return install.file;
    case "app_bundle":
      return install.bundle;
  }
}
export const emptyCustomProfile = (): ProfileEdit => ({
  profile: {
    type: "custom",
    id: "",
    name: t`Custom profile`,
    executable: null,
    args: [],
    workingDir: null,
    platforms: [],
    extensions: [],
    env: [],
    tracking: { type: "default" },
    hooks: { before: null, afterExit: null },
  },
  argsText: '"{RomPath}"',
  beforeText: "",
  afterExitText: "",
});
