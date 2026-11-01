import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, expect, it, vi } from "vitest";
import type {
  DetectJob_Serialize,
  EmulationCatalog_Serialize,
  EmulatorView_Serialize,
  PlatformView_Serialize,
  ScanJob_Serialize,
} from "./bindings";
import { createNativeEmulationApi } from "./emulationNative";

afterEach(() => {
  clearMocks();
  vi.unstubAllGlobals();
});
const catalog: EmulationCatalog_Serialize = {
  platforms: [{ id: "ps2", name: "PlayStation 2", short: "PS2", extensions: ["iso"], bios: [] }],
  definitions: [
    {
      id: "pcsx2",
      name: "PCSX2",
      website: null,
      vars: [],
      profiles: [
        { name: "PS2", platforms: ["ps2"], extensions: ["iso"], args: ["{RomPath}"], requires: [] },
      ],
    },
  ],
  variables: [{ name: "RomPath", aliases: ["ImagePath"], description: "The ROM file" }],
};
const emulator: EmulatorView_Serialize = {
  emulator: {
    id: "e1",
    name: "PCSX2",
    definition: "pcsx2",
    install: { type: "app_image", file: "/apps/pcsx2.AppImage" },
    profiles: [
      {
        type: "built_in",
        id: "p1",
        definitionProfile: "PS2",
        args: { type: "replace", args: ["-batch", "{RomPath}"], replaced: ["{RomPath}"] },
        hooks: { before: null, afterExit: null },
      },
    ],
  },
  status: "some_missing",
  profiles: [
    {
      id: "p1",
      name: "PS2",
      platforms: ["ps2"],
      extensions: ["iso"],
      availability: { type: "missing_files", missing: ["/bios/ps2.bin"] },
      bios: [
        {
          file: "ps2.bin",
          required: true,
          found: null,
        } as EmulatorView_Serialize["profiles"][number]["bios"][number],
      ],
      preview: {
        sampleRom: "/roms/Game.iso",
        program: "/apps/pcsx2.AppImage",
        args: ["-batch", "/roms/Game.iso"],
        cwd: null,
        error: null,
      },
      drift: { overriddenFrom: ["{RomPath}"], definitionArgs: ["-fastboot", "{RomPath}"] },
    },
  ],
  addable: [],
};
const platform: PlatformView_Serialize = {
  platform: catalog.platforms[0] as PlatformView_Serialize["platform"],
  settings: { platform: "ps2", default: { emulatorId: "e1", profileId: "p1" }, biosDir: null },
  romCount: 3,
  compatible: [],
};
const job: DetectJob_Serialize = {
  id: "d1",
  revision: 4,
  status: "done",
  folder: "/portable",
  error: null,
  results: [
    {
      key: "k1",
      definitionId: "pcsx2",
      name: "PCSX2",
      install: { type: "flatpak", appId: "net.pcsx2.PCSX2" },
      version: "2.2",
      profiles: [{ name: "PS2", available: true }],
      configured: false,
    },
  ],
};
const scanJob: ScanJob_Serialize = {
  id: "s1",
  revision: 2,
  scannerId: "r1",
  mode: "review",
  status: "review",
  progress: null,
  review: [],
  summary: null,
  warnings: [],
  error: null,
};
function backend() {
  const invoke = vi.fn((command: string, args?: unknown) => {
    switch (command) {
      case "emulation_catalog":
        return catalog;
      case "emulation_overview":
        return { emulators: 1, platformsWithDefault: 0, roms: 3, biosIssues: null };
      case "emulators_list":
        return [emulator];
      case "emulation_platforms":
        return [platform];
      case "args_split":
        return String((args as { text: string }).text)
          .split(" ")
          .filter(Boolean);
      case "emulator_save":
        return emulator;
      case "emulation_detect_latest":
        return job;
      case "emulation_detect_import":
        return { emulators: ["e2"], defaultsSet: ["ps2"] };
      case "rom_scan_start":
      case "rom_scan_submit":
        return { ...scanJob, status: command === "rom_scan_start" ? "running" : "done" };
      case "rom_scan_jobs":
        return [scanJob];
      case "rom_scan_all":
        return [];
      default:
        throw new Error(`unexpected ${command}`);
    }
  });
  mockIPC(invoke);
  return invoke;
}
it("projects the generated #116 DTOs without resolving anything in the webview", async () => {
  vi.stubGlobal("isTauri", true);
  backend();
  const snapshot = await createNativeEmulationApi().snapshot();
  expect(snapshot.stats).toEqual({
    emulators: 1,
    workingPlatforms: 0,
    platforms: 1,
    roms: 3,
    biosIssues: null,
  });
  expect(snapshot.variables.map((row) => row.name)).toEqual(["RomPath", "ImagePath"]);
  const [row] = snapshot.emulators;
  expect(row?.found).toBe(true);
  expect(row?.profiles[0]?.resolved).toMatchObject({
    available: false,
    missing: ["/bios/ps2.bin"],
    args: ["-batch", "/roms/Game.iso"],
    drift: { definitionArgs: ["-fastboot", "{RomPath}"] },
  });
  expect(snapshot.platforms[0]).toMatchObject({ romCount: 3, default: { profileId: "p1" } });
});
it("splits drafts in Rust and keeps the reviewed Replace baseline when saving", async () => {
  vi.stubGlobal("isTauri", true);
  const invoke = backend();
  await createNativeEmulationApi().saveProfile("e1", {
    profile: {
      type: "built_in",
      id: "p1",
      definitionProfile: "PS2",
      args: { type: "replace", args: [], replaced: ["-fastboot", "{RomPath}"] },
      hooks: { before: null, afterExit: null },
    },
    argsText: "-batch {RomPath}",
    beforeText: "",
    afterExitText: "",
  });
  expect(invoke).toHaveBeenCalledWith("args_split", { text: "-batch {RomPath}" });
  const save = invoke.mock.calls.find(([command]) => command === "emulator_save");
  expect(save?.[1]).toMatchObject({
    emulator: {
      id: "e1",
      profiles: [
        {
          id: "p1",
          args: {
            type: "replace",
            args: ["-batch", "{RomPath}"],
            replaced: ["-fastboot", "{RomPath}"],
          },
        },
      ],
    },
  });
});
it("picks up the latest detection and imports the reviewed revision", async () => {
  vi.stubGlobal("isTauri", true);
  const invoke = backend();
  const api = createNativeEmulationApi();
  const detection = await api.detection();
  expect(detection).toMatchObject({
    id: "d1",
    revision: 4,
    status: "completed",
    folder: "/portable",
  });
  expect(detection.candidates[0]?.id).toBe("k1");
  await api.importDetected(["k1"], detection.revision);
  expect(invoke).toHaveBeenCalledWith("emulation_detect_import", {
    id: "d1",
    revision: 4,
    keys: ["k1"],
  });
});
it("refuses to run outside the desktop app", async () => {
  await expect(createNativeEmulationApi().snapshot()).rejects.toThrow(
    "Open Caskade desktop to configure emulators.",
  );
});
it("drives ROM scans through the generated scanner commands", async () => {
  vi.stubGlobal("isTauri", true);
  const invoke = backend();
  const api = createNativeEmulationApi();
  await api.startScan("r1", "review");
  expect(invoke).toHaveBeenCalledWith("rom_scan_start", { scannerId: "r1", mode: "review" });
  expect((await api.scanJobs())[0]?.id).toBe("s1");
  const choices = [
    { roms: ["a.chd", "b.chd"], include: true, title: "A", platform: "ps2", regions: [] },
  ];
  await api.submitReview("s1", 2, choices);
  expect(invoke).toHaveBeenCalledWith("rom_scan_submit", { id: "s1", revision: 2, choices });
  expect(await api.scanAll()).toEqual([]);
});
