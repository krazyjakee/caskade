/** Original synthetic emulator data, imported only in explicit development
 * fixture mode or tests. No emulator, ROM or BIOS is read or executed. */

import type {
  Emulator_Deserialize,
  EmulatorProfile_Deserialize,
  EmulatorProfile_Serialize,
  ProfileHooks_Deserialize,
  RomScanner_Serialize,
} from "./bindings";
import type { EmulatorDefinition, ProfileRef } from "./emulation";
import { createScanFixtures } from "./emulationScanFixtures";
import {
  createEmulationApi,
  type DetectJob,
  type EmulationTransport,
  type EmulatorRecord,
  type EmulatorView,
  type PlatformInfo,
  type PlatformView,
} from "./emulationTransport";
import { createEmulatorSetupFixtureApi } from "./emulatorSetupFixtures";

const definitions: EmulatorDefinition[] = [
  {
    id: "retroarch",
    name: "RetroArch",
    website: "https://www.retroarch.com/",
    vars: ["cores"],
    profiles: [
      {
        name: "SNES (Snes9x)",
        platforms: ["snes"],
        extensions: ["sfc", "smc"],
        args: ["-L", "{cores}/snes9x.so", "{RomPath}"],
        requires: ["{cores}/snes9x.so"],
      },
      {
        name: "PS1 (Beetle)",
        platforms: ["ps1"],
        extensions: ["cue", "chd"],
        args: ["-L", "{cores}/beetle.so", "{RomPath}"],
        requires: ["{cores}/beetle.so"],
      },
      {
        name: "GBA (mGBA)",
        platforms: ["gba"],
        extensions: ["gba"],
        args: ["-L", "{cores}/mgba.so", "{RomPath}"],
        requires: ["{cores}/mgba.so"],
      },
    ],
  },
  {
    id: "pcsx2",
    name: "PCSX2",
    website: "https://pcsx2.net/",
    vars: [],
    profiles: [
      {
        name: "PS2",
        platforms: ["ps2"],
        extensions: ["iso", "chd"],
        args: ["-batch", "{RomPath}"],
        requires: [],
      },
    ],
  },
];
/** BIOS status as #122 reports it: found, missing or the wrong dump. */
const bios = (file: string, required: boolean, state: "found" | "missing" | "wrong_hash") =>
  ({
    file,
    required,
    found: state === "found",
    state,
    path: state === "missing" ? null : `/fixture/bios/${file}`,
    platform: null,
  }) as PlatformInfo["bios"][number];
const platforms: PlatformInfo[] = [
  { id: "snes", name: "Super Nintendo", short: "SNES", extensions: ["sfc", "smc"], bios: [] },
  {
    id: "ps1",
    name: "PlayStation",
    short: "PS1",
    extensions: ["cue", "chd"],
    bios: [
      bios("scph5500.bin", false, "missing"),
      bios("scph5501.bin", true, "found"),
      bios("scph5502.bin", false, "wrong_hash"),
    ],
  },
  { id: "gba", name: "Game Boy Advance", short: "GBA", extensions: ["gba"], bios: [] },
  {
    id: "ps2",
    name: "PlayStation 2",
    short: "PS2",
    extensions: ["iso", "chd"],
    bios: [bios("ps2-bios.bin", true, "missing")],
  },
];
const builtIn = (id: string, name: string): EmulatorProfile_Serialize => ({
  type: "built_in",
  id,
  definitionProfile: name,
  args: { type: "default" },
  hooks: { before: null, afterExit: null },
});
const copy = <T>(value: T): T => structuredClone(value);
const ok = async <T>(data: T) => ({ status: "ok" as const, data: copy(data) });
/** Fixture-only simulation of the parser endpoint; production uses argsSplit. */
function fixtureWords(text: string): string[] {
  const words: string[] = [];
  let word = "";
  let open = false;
  let quote = "";
  for (let index = 0; index < text.length; index++) {
    const char = text.charAt(index);
    if (char === "\\" && quote !== "'" && index + 1 < text.length) {
      word += text.charAt(++index);
      open = true;
    } else if (quote) {
      if (char === quote) quote = "";
      else word += char;
    } else if (char === '"' || char === "'") {
      quote = char;
      open = true;
    } else if (/\s/.test(char)) {
      if (open) words.push(word);
      word = "";
      open = false;
    } else {
      word += char;
      open = true;
    }
  }
  if (quote) throw new Error("Unbalanced quoting in arguments.");
  if (open) words.push(word);
  return words;
}
type FixtureRecord = EmulatorRecord;
const hooks = (value: ProfileHooks_Deserialize) => ({
  before: value.before ?? null,
  afterExit: value.afterExit ?? null,
});
/** What the service stores: defaults filled in, Replace overrides with the args they replaced. */
function serialized(profile: EmulatorProfile_Deserialize): EmulatorProfile_Serialize {
  if (profile.type === "custom")
    return {
      ...profile,
      executable: profile.executable ?? null,
      workingDir: profile.workingDir ?? null,
      hooks: hooks(profile.hooks),
    };
  const { args } = profile;
  return {
    ...profile,
    hooks: hooks(profile.hooks),
    args: args.type === "replace" ? { ...args, replaced: args.replaced ?? [] } : args,
  };
}
const stored = (record: Emulator_Deserialize): EmulatorRecord => ({
  ...structuredClone(record),
  definition: record.definition ?? null,
  profiles: record.profiles.map(serialized),
});
export function createEmulationFixtureApi() {
  let nextId = 0;
  let records: FixtureRecord[] = [
    {
      id: "fixture-retroarch",
      name: "RetroArch",
      definition: "retroarch",
      install: { type: "flatpak", appId: "org.libretro.RetroArch" },
      profiles: [builtIn("fixture-snes", "SNES (Snes9x)"), builtIn("fixture-ps1", "PS1 (Beetle)")],
    },
  ];
  const defaults = new Map<string, ProfileRef | null>([
    ["snes", { emulatorId: "fixture-retroarch", profileId: "fixture-snes" }],
  ]);
  const biosFolders = new Map<string, string | null>();
  const listeners = new Set<() => void>();
  const detectionListeners = new Set<(job: DetectJob) => void>();
  let job: DetectJob | null = null;
  const emit = () => {
    for (const listener of listeners) listener();
  };
  const emitJob = () => {
    if (job) for (const listener of detectionListeners) listener(copy(job));
  };
  const profileView = (
    record: FixtureRecord,
    profile: EmulatorProfile_Serialize,
  ): EmulatorView["profiles"][number] => {
    const definition = definitions.find((row) => row.id === record.definition);
    const base =
      profile.type === "built_in"
        ? definition?.profiles.find((row) => row.name === profile.definitionProfile)
        : null;
    if (profile.type === "built_in" && !base) throw new Error("Unknown specification profile.");
    const ids = profile.type === "custom" ? profile.platforms : (base?.platforms ?? []);
    if (ids.some((id) => !platforms.some((platform) => platform.id === id)))
      throw new Error("Unknown platform.");
    const sampleRom = `/fixture/ROMs/${ids[0] ?? "games"}/Sample Game (USA).${(profile.type === "custom" ? profile.extensions[0] : base?.extensions[0]) ?? "rom"}`;
    const vars: Record<string, string> = {
      RomPath: sampleRom,
      ImagePath: sampleRom,
      RomName: sampleRom.split("/").at(-1) ?? "Sample Game",
      EmulatorDir: "/fixture/tools",
      UserHome: "/fixture/home",
      cores: "/fixture/cores",
    };
    const expand = (text: string) =>
      text.replace(/\{(\w+)\}/g, (_, name: string) => {
        if (!(name in vars)) throw new Error(`Unknown variable {${name}}.`);
        return vars[name] ?? "";
      });
    const args =
      profile.type === "custom"
        ? profile.args
        : profile.args.type === "default"
          ? (base?.args ?? [])
          : profile.args.type === "replace"
            ? profile.args.args
            : [...(base?.args ?? []), ...profile.args.args];
    for (const hook of [...(profile.hooks.before ?? []), ...(profile.hooks.afterExit ?? [])])
      expand(hook);
    let program = "";
    let prefix: string[] = [];
    switch (record.install.type) {
      case "folder":
        program = record.install.executable.startsWith("/")
          ? record.install.executable
          : `${record.install.dir}/${record.install.executable}`;
        break;
      case "path":
        program = record.install.binary;
        break;
      case "flatpak":
        program = "flatpak";
        prefix = ["run", record.install.appId];
        break;
      case "app_image":
        program = record.install.file;
        break;
      case "app_bundle":
        program = `${record.install.bundle}/Contents/MacOS/FixtureEmulator`;
        break;
    }
    const missing = base?.name.includes("Beetle") ? ["/fixture/cores/beetle.so"] : [];
    return {
      id: profile.id,
      name: profile.type === "custom" ? profile.name : profile.definitionProfile,
      platforms: ids,
      extensions: profile.type === "custom" ? profile.extensions : (base?.extensions ?? []),
      availability: missing.length ? { type: "missing_files", missing } : { type: "available" },
      bios: ids.flatMap((id) => platforms.find((row) => row.id === id)?.bios ?? []),
      drift:
        profile.type === "built_in" &&
        profile.args.type === "replace" &&
        profile.args.replaced?.join("\0") !== base?.args.join("\0")
          ? { overriddenFrom: profile.args.replaced ?? [], definitionArgs: base?.args ?? [] }
          : null,
      preview: {
        sampleRom,
        program: profile.type === "custom" && profile.executable ? profile.executable : program,
        args: [...prefix, ...args.map(expand)],
        cwd: profile.type === "custom" ? profile.workingDir : null,
        error: null,
      },
    };
  };
  const view = (record: FixtureRecord): EmulatorView => {
    const profiles = record.profiles.map((profile) => profileView(record, profile));
    return {
      emulator: copy(record),
      status: profiles.some((row) => row.availability.type !== "available")
        ? "some_missing"
        : "ready",
      profiles,
      addable: (definitions.find((row) => row.id === record.definition)?.profiles ?? [])
        .filter(
          (row) =>
            !record.profiles.some(
              (profile) => profile.type === "built_in" && profile.definitionProfile === row.name,
            ),
        )
        .map((row) => ({
          name: row.name,
          platforms: row.platforms,
          available: !row.name.includes("Beetle"),
        }))
        .sort((a, b) => Number(b.available) - Number(a.available)),
    };
  };
  const platformViews = (): PlatformView[] =>
    platforms.map((platform) => ({
      platform: copy(platform),
      settings: {
        platform: platform.id,
        default: defaults.get(platform.id) ?? null,
        biosDir: biosFolders.get(platform.id) ?? null,
      },
      romCount: scans.romCount(platform.id),
      compatible: records.flatMap((record) =>
        view(record)
          .profiles.filter((profile) => profile.platforms.includes(platform.id))
          .map((profile) => ({
            emulatorId: record.id,
            profileId: profile.id,
            emulatorName: record.name,
            profileName: profile.name,
            available: profile.availability.type === "available",
          })),
      ),
    }));
  const scans = createScanFixtures((ref) => {
    const record = records.find((row) => row.id === ref.emulatorId);
    const config = record?.profiles.find((row) => row.id === ref.profileId);
    if (!record || !config) return null;
    const view = profileView(record, config);
    return {
      platforms: view.platforms,
      extensions: view.extensions,
      emulatorName: record.name,
      flatpak: record.install.type === "flatpak",
    };
  }, emit);
  const transport: EmulationTransport = {
    ...scans.transport,
    // The collection has a folder for every platform an emulator runs.
    async romFolderSetup(folder) {
      const added: RomScanner_Serialize[] = [];
      for (const view of platformViews()) {
        const profile =
          view.settings.default ?? view.compatible.find((candidate) => candidate.available);
        if (!profile) continue;
        const saved = await scans.transport.romScannerSave({
          id: "",
          name: view.platform.name,
          folder: `${folder}/${view.platform.short.toLowerCase()}`,
          platform: view.platform.id,
          profile: { emulatorId: profile.emulatorId, profileId: profile.profileId },
          launchWith: { type: "platform_default" },
          recursive: true,
          scanArchives: false,
          skipCloudPlaceholders: true,
          exclusions: [],
          inLibraryUpdate: true,
        });
        if (saved.status === "ok") added.push(saved.data);
      }
      return ok({ added, existing: 0, noEmulator: [], unknown: [] });
    },
    emulationCatalog: () =>
      ok({
        platforms,
        definitions,
        variables: [
          { name: "RomPath", aliases: ["ImagePath"], description: "Full path of the ROM" },
          { name: "EmulatorDir", aliases: [], description: "Emulator installation directory" },
        ],
      }),
    emulationOverview: () =>
      ok({
        emulators: records.length,
        platformsWithDefault: platformViews().filter((row) =>
          row.compatible.some(
            (profile) =>
              profile.available &&
              profile.emulatorId === row.settings.default?.emulatorId &&
              profile.profileId === row.settings.default.profileId,
          ),
        ).length,
        roms: platforms.reduce((total, platform) => total + scans.romCount(platform.id), 0),
        biosIssues: 1,
      }),
    emulatorsList: () => ok(records.map(view)),
    emulationPlatforms: () => ok(platformViews()),
    async emulatorSave(input) {
      const next = stored(input);
      if (!next.name.trim()) throw new Error("Emulator name is required.");
      if (next.install.type === "folder" && !next.install.dir.startsWith("/"))
        throw new Error("Choose an absolute emulator folder.");
      if (!next.id) next.id = `fixture-emulator-${++nextId}`;
      for (const profile of next.profiles) {
        if (!profile.id) profile.id = `fixture-profile-${++nextId}`;
        if (
          profile.type === "built_in" &&
          profile.args.type === "replace" &&
          !profile.args.replaced?.length
        )
          profile.args.replaced =
            definitions
              .find((row) => row.id === next.definition)
              ?.profiles.find((row) => row.name === profile.definitionProfile)?.args ?? [];
      }
      const result = view(next);
      records = [...records.filter((record) => record.id !== next.id), next];
      emit();
      return ok(result);
    },
    async emulatorDelete(id) {
      records = records.filter((record) => record.id !== id);
      for (const [platform, selected] of defaults)
        if (selected?.emulatorId === id) defaults.set(platform, null);
      emit();
      return ok(null);
    },
    profilePreview: async (draft) => {
      const record = stored({
        id: "preview",
        name: "Preview",
        ...draft,
        profiles: [draft.profile],
      });
      const [profile] = record.profiles;
      if (!profile) throw new Error("Missing profile.");
      return ok(profileView(record, profile).preview);
    },
    argsSplit: async (text) => ok(fixtureWords(text)),
    async platformSettingsSet(settings) {
      const platform = platformViews().find((row) => row.platform.id === settings.platform);
      if (!platform) throw new Error("Unknown platform.");
      if (
        settings.default &&
        !platform.compatible.some(
          (row) =>
            row.emulatorId === settings.default?.emulatorId &&
            row.profileId === settings.default.profileId,
        )
      )
        throw new Error("That profile does not run this platform.");
      defaults.set(settings.platform, settings.default);
      biosFolders.set(settings.platform, settings.biosDir);
      emit();
      const next = platformViews().find((row) => row.platform.id === settings.platform);
      if (!next) throw new Error("Missing platform.");
      return ok(next);
    },
    async emulationDetectStart(folder) {
      job = {
        id: `fixture-detect-${++nextId}`,
        revision: 1,
        status: "done",
        folder,
        error: null,
        results: [
          {
            key: "fixture-retroarch-install",
            definitionId: "retroarch",
            name: "RetroArch",
            install: { type: "flatpak", appId: "org.libretro.RetroArch" },
            version: "1.20.0",
            profiles: [{ name: "SNES (Snes9x)", available: true }],
            configured: records.some((row) => row.definition === "retroarch"),
          },
          {
            key: "fixture-pcsx2-install",
            definitionId: "pcsx2",
            name: "PCSX2",
            install: { type: "app_image", file: "/fixture/Applications/PCSX2.AppImage" },
            version: "2.0.0",
            profiles: [{ name: "PS2", available: true }],
            configured: records.some((row) => row.definition === "pcsx2"),
          },
        ],
      };
      emitJob();
      return ok(job);
    },
    emulationDetectLatest: () => ok(job),
    async emulationDetectGet(id) {
      if (!job || job.id !== id) throw new Error("Unknown detection run.");
      return ok(job);
    },
    async emulationDetectCancel(id) {
      if (!job || job.id !== id) throw new Error("Unknown detection run.");
      job = { ...job, revision: job.revision + 1, status: "cancelled" };
      emitJob();
      return ok(job);
    },
    async emulationDetectImport(id, revision, keys) {
      if (!job || job.id !== id || job.revision !== revision)
        throw new Error("Detection changed. Review the latest results.");
      const selected = keys.map((key) => {
        const row = job?.results.find((row) => row.key === key);
        if (!row || row.configured)
          throw new Error("Unknown or already configured detection result.");
        return row;
      });
      const imported: string[] = [];
      const defaultsSet: string[] = [];
      for (const row of selected) {
        const record: FixtureRecord = {
          id: `fixture-emulator-${++nextId}`,
          name: row.name,
          definition: row.definitionId,
          install: row.install,
          profiles: row.profiles
            .filter((profile) => profile.available)
            .map((profile) => builtIn(`fixture-profile-${++nextId}`, profile.name)),
        };
        records.push(record);
        imported.push(record.id);
        for (const profile of view(record).profiles)
          for (const platform of profile.platforms)
            if (!defaults.get(platform)) {
              defaults.set(platform, { emulatorId: record.id, profileId: profile.id });
              defaultsSet.push(platform);
            }
      }
      job = {
        ...job,
        revision: job.revision + 1,
        results: job.results.map((row) =>
          keys.includes(row.key) ? { ...row, configured: true } : row,
        ),
      };
      emit();
      emitJob();
      return ok({ emulators: imported, defaultsSet });
    },
  };
  return createEmulationApi(transport, {
    subscribe: async (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    subscribeDetection: async (listener) => {
      detectionListeners.add(listener);
      return () => {
        detectionListeners.delete(listener);
      };
    },
    subscribeScans: scans.subscribe,
    choosePath: async (kind) => (kind === "folder" ? "/fixture/tools" : "/fixture/tools/emulator"),
    setup: createEmulatorSetupFixtureApi(),
  });
}
