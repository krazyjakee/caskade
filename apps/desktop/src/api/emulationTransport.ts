import { plural, t } from "@lingui/core/macro";
import type {
  DetectImport,
  DetectJob_Serialize,
  EmulationCatalog_Serialize,
  EmulationOverview_Serialize,
  Emulator_Deserialize,
  Emulator_Serialize,
  EmulatorView_Serialize,
  PlatformInfo_Serialize,
  PlatformSettings_Serialize,
  PlatformView_Serialize,
  ProfileDraft_Deserialize,
  ReviewChoice,
  RomFolderSetup_Serialize,
  RomScanner_Deserialize,
  RomScanner_Serialize,
  ScanJob_Serialize,
  ScanMode,
  ScannerView_Serialize,
  ScanSummary_Serialize,
} from "./bindings";
import type {
  ArgvPreview,
  EmulationApi,
  EmulationDetection,
  EmulationSnapshot,
  EmulatorProfile,
  ProfileEdit,
} from "./emulation";
import { type Reply, unwrap } from "./reply";

/** The generated #115/#116 command surface. The native adapter passes the
 * bindings straight through, so a DTO change fails typechecking here. All
 * writes and previews go through the Rust validator. */
export type EmulatorRecord = Emulator_Serialize;
export type EmulatorView = EmulatorView_Serialize;
export type PlatformInfo = PlatformInfo_Serialize;
export type PlatformView = PlatformView_Serialize;
export type DetectJob = DetectJob_Serialize;
export interface EmulationTransport {
  emulationCatalog(): Promise<Reply<EmulationCatalog_Serialize>>;
  emulationOverview(): Promise<Reply<EmulationOverview_Serialize>>;
  emulatorsList(): Promise<Reply<EmulatorView[]>>;
  emulationPlatforms(): Promise<Reply<PlatformView[]>>;
  emulatorSave(emulator: Emulator_Deserialize): Promise<Reply<EmulatorView>>;
  emulatorDelete(id: string): Promise<Reply<null>>;
  profilePreview(draft: ProfileDraft_Deserialize): Promise<Reply<ArgvPreview>>;
  argsSplit(text: string): Promise<Reply<string[]>>;
  platformSettingsSet(settings: PlatformSettings_Serialize): Promise<Reply<PlatformView>>;
  emulationDetectStart(folder: string | null): Promise<Reply<DetectJob>>;
  emulationDetectCancel(id: string): Promise<Reply<DetectJob>>;
  emulationDetectGet(id: string): Promise<Reply<DetectJob>>;
  emulationDetectLatest(): Promise<Reply<DetectJob | null>>;
  emulationDetectImport(id: string, revision: number, keys: string[]): Promise<Reply<DetectImport>>;
  romScannersList(): Promise<Reply<ScannerView_Serialize[]>>;
  romScannerSave(scanner: RomScanner_Deserialize): Promise<Reply<RomScanner_Serialize>>;
  romScannerDelete(id: string): Promise<Reply<null>>;
  romScanStart(scannerId: string, mode: ScanMode): Promise<Reply<ScanJob_Serialize>>;
  romScanJobs(): Promise<Reply<ScanJob_Serialize[]>>;
  romScanCancel(id: string): Promise<Reply<ScanJob_Serialize>>;
  romScanSubmit(
    id: string,
    revision: number,
    choices: ReviewChoice[],
  ): Promise<Reply<ScanJob_Serialize>>;
  romScanAll(): Promise<Reply<ScanSummary_Serialize[]>>;
  romFolderSetup(folder: string): Promise<Reply<RomFolderSetup_Serialize>>;
  romExclusions(): Promise<Reply<string[]>>;
  romExclusionsSet(patterns: string[]): Promise<Reply<string[]>>;
}
export function createEmulationApi(
  transport: EmulationTransport,
  options: {
    subscribe: EmulationApi["subscribe"];
    subscribeDetection(listener: (job: DetectJob) => void): Promise<() => void>;
    subscribeScans: EmulationApi["subscribeScans"];
    choosePath: EmulationApi["choosePath"];
    setup?: EmulationApi["setup"];
  },
): EmulationApi {
  let job: DetectJob | null = null;
  async function installed(id: string): Promise<Emulator_Deserialize> {
    const emulator = (await unwrap(transport.emulatorsList())).find(
      (row) => row.emulator.id === id,
    );
    if (!emulator) throw new Error(t`This emulator no longer exists. Refresh the list.`);
    return structuredClone(emulator.emulator);
  }
  async function profile(draft: ProfileEdit): Promise<EmulatorProfile> {
    const [args, before, afterExit] = await Promise.all([
      unwrap(transport.argsSplit(draft.argsText)),
      unwrap(transport.argsSplit(draft.beforeText)),
      unwrap(transport.argsSplit(draft.afterExitText)),
    ]);
    const hooks = {
      before: before.length ? before : null,
      afterExit: afterExit.length ? afterExit : null,
    };
    if (draft.profile.type === "custom") return { ...draft.profile, args, hooks };
    return {
      ...draft.profile,
      hooks,
      args:
        draft.profile.args.type === "default"
          ? { type: "default" }
          : draft.profile.args.type === "append"
            ? { type: "append", args }
            : // An empty `replaced` keeps the stored one; the editor sets it to
              // the current definition args when the user reviews a drift.
              { type: "replace", args, replaced: draft.profile.args.replaced ?? [] },
    };
  }
  const detection = (value: DetectJob | null): EmulationDetection =>
    value
      ? {
          id: value.id,
          revision: value.revision,
          status:
            value.status === "running"
              ? "searching"
              : value.status === "done"
                ? "completed"
                : value.status,
          folder: value.folder,
          candidates: value.results.map((row) => ({
            id: row.key,
            definition: row.definitionId,
            name: row.name,
            install: row.install,
            version: row.version,
            configuredAs: row.configured ? row.key : null,
            profiles: row.profiles.map((item) => ({ ...item, missing: [] })),
          })),
          warnings: [],
          error: value.error ?? (value.status === "failed" ? t`Emulator detection failed.` : null),
          message:
            value.status === "running"
              ? t`Searching… ${{ found: value.results.length }} found so far`
              : value.status === "cancelled"
                ? t`Detection cancelled · ${{ found: value.results.length }} found`
                : plural(value.results.length, {
                    one: "# emulator installation found",
                    other: "# emulator installations found",
                  }),
        }
      : {
          id: null,
          revision: 0,
          status: "idle",
          folder: null,
          candidates: [],
          warnings: [],
          error: null,
          message: t`No detection run yet.`,
        };
  const api: EmulationApi = {
    async snapshot() {
      const [catalog, overview, installed, platforms] = await Promise.all([
        unwrap(transport.emulationCatalog()),
        unwrap(transport.emulationOverview()),
        unwrap(transport.emulatorsList()),
        unwrap(transport.emulationPlatforms()),
      ]);
      const data: EmulationSnapshot = {
        definitions: catalog.definitions,
        variables: catalog.variables.flatMap((row) => [
          { name: row.name, description: row.description },
          ...row.aliases.map((name) => ({ name, description: `Alias for {${row.name}}` })),
        ]),
        platforms: platforms.map((row) => ({
          ...row.platform,
          romCount: row.romCount,
          default: row.settings.default,
          biosDir: row.settings.biosDir,
          bios: row.platform.bios,
        })),
        emulators: installed.map((row) => ({
          id: row.emulator.id,
          name: row.emulator.name,
          definition: row.emulator.definition,
          install: row.emulator.install,
          found: row.status !== "not_found",
          version: null,
          definitionProfiles: row.addable.map((item) => ({
            name: item.name,
            available: item.available,
            missing: [],
          })),
          profiles: row.emulator.profiles.map((config) => {
            const state = row.profiles.find((item) => item.id === config.id);
            if (!state)
              throw new Error(
                t`Profile status is missing for ${{ profile: config.id }}. Refresh the list.`,
              );
            return {
              config,
              resolved: {
                ...state.preview,
                name: state.name,
                platforms: state.platforms,
                extensions: state.extensions,
                available: state.availability.type === "available",
                missing:
                  state.availability.type === "missing_files" ? state.availability.missing : [],
                bios: state.bios,
                drift: state.drift,
              },
            };
          }),
        })),
        stats: {
          emulators: overview.emulators,
          workingPlatforms: overview.platformsWithDefault,
          platforms: catalog.platforms.length,
          roms: overview.roms,
          biosIssues: overview.biosIssues,
        },
      };
      return data;
    },
    async saveEmulator(input) {
      const record = input.id ? await installed(input.id) : { ...input, id: "", profiles: [] };
      await unwrap(
        transport.emulatorSave({
          ...record,
          name: input.name,
          definition: input.definition,
          install: input.install,
          profiles:
            record.definition === input.definition
              ? record.profiles
              : record.profiles.filter((item) => item.type === "custom"),
        }),
      );
      return api.snapshot();
    },
    async removeEmulator(id) {
      await unwrap(transport.emulatorDelete(id));
      return api.snapshot();
    },
    async saveProfile(id, draft) {
      const record = await installed(id);
      const parsed = await profile(draft);
      const index = record.profiles.findIndex((item) => item.id === parsed.id);
      if (index >= 0) record.profiles[index] = parsed;
      else record.profiles.push(parsed);
      await unwrap(transport.emulatorSave(record));
      return api.snapshot();
    },
    async removeProfile(id, profileId) {
      const record = await installed(id);
      record.profiles = record.profiles.filter((item) => item.id !== profileId);
      await unwrap(transport.emulatorSave(record));
      return api.snapshot();
    },
    async setPlatform(platform, selected, biosDir) {
      await unwrap(transport.platformSettingsSet({ platform, default: selected, biosDir }));
      return api.snapshot();
    },
    async preview(id, draft) {
      const emulator = await installed(id);
      return unwrap(
        transport.profilePreview({
          install: emulator.install,
          definition: emulator.definition,
          profile: await profile(draft),
        }),
      );
    },
    async detection() {
      // A detection may have been started elsewhere, e.g. by a library scan.
      if (!job) job = await unwrap(transport.emulationDetectLatest());
      else {
        const id = job.id;
        const next = await unwrap(transport.emulationDetectGet(id));
        if (job.id === id && job.revision <= next.revision) job = next;
      }
      return detection(job);
    },
    async detect(folder) {
      job = await unwrap(transport.emulationDetectStart(folder));
      return detection(job);
    },
    async cancelDetection() {
      if (job) {
        const id = job.id;
        const next = await unwrap(transport.emulationDetectCancel(id));
        if (job.id === id && job.revision <= next.revision) job = next;
      }
      return detection(job);
    },
    async importDetected(ids, revision) {
      if (!job) throw new Error(t`Run emulator detection before importing.`);
      await unwrap(transport.emulationDetectImport(job.id, revision, ids));
      return api.snapshot();
    },
    choosePath: options.choosePath,
    scanners: () => unwrap(transport.romScannersList()),
    saveScanner: (scanner) => unwrap(transport.romScannerSave(scanner)),
    async removeScanner(id) {
      await unwrap(transport.romScannerDelete(id));
    },
    startScan: (scannerId, mode) => unwrap(transport.romScanStart(scannerId, mode)),
    scanJobs: () => unwrap(transport.romScanJobs()),
    cancelScan: (id) => unwrap(transport.romScanCancel(id)),
    submitReview: (id, revision, choices) => unwrap(transport.romScanSubmit(id, revision, choices)),
    scanAll: () => unwrap(transport.romScanAll()),
    setupRomFolder: (folder) => unwrap(transport.romFolderSetup(folder)),
    globalExclusions: () => unwrap(transport.romExclusions()),
    setGlobalExclusions: (patterns) => unwrap(transport.romExclusionsSet(patterns)),
    subscribeScans: options.subscribeScans,
    subscribe: options.subscribe,
    setup: options.setup,
    subscribeDetection: (listener) =>
      options.subscribeDetection((next) => {
        if (job && (job.id !== next.id || job.revision > next.revision)) return;
        job = next;
        listener(detection(job));
      }),
  };
  return api;
}
