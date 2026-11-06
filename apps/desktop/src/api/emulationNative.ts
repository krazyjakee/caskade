import { t } from "@lingui/core/macro";
import { isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { commands, events } from "./bindings";
import type { EmulationApi } from "./emulation";
import { createEmulationApi } from "./emulationTransport";
import { createNativeEmulatorSetupApi } from "./emulatorSetup";

function native<T>(request: () => Promise<T>): Promise<T> {
  if (!isTauri()) return Promise.reject(new Error("Open Caskade desktop to configure emulators."));
  return request();
}
const ok = <T>(data: T) => ({ status: "ok" as const, data });

/** #116 settings commands and #115 detection, straight from the generated bindings. */
export function createNativeEmulationApi(): EmulationApi {
  return createEmulationApi(
    {
      emulationCatalog: () => native(async () => ok(await commands.emulationCatalog())),
      emulationOverview: () => native(commands.emulationOverview),
      emulatorsList: () => native(commands.emulatorsList),
      emulatorSave: (emulator) => native(() => commands.emulatorSave(emulator)),
      emulatorDelete: (id) => native(() => commands.emulatorDelete(id)),
      profilePreview: (draft) => native(async () => ok(await commands.profilePreview(draft))),
      argsSplit: (text) => native(() => commands.argsSplit(text)),
      emulationPlatforms: () => native(commands.emulationPlatforms),
      platformSettingsSet: (settings) => native(() => commands.platformSettingsSet(settings)),
      emulationDetectStart: (folder) => native(() => commands.emulationDetectStart(folder)),
      emulationDetectGet: (id) => native(() => commands.emulationDetectGet(id)),
      emulationDetectLatest: () => native(async () => ok(await commands.emulationDetectLatest())),
      emulationDetectCancel: (id) => native(() => commands.emulationDetectCancel(id)),
      emulationDetectImport: (id, revision, keys) =>
        native(() => commands.emulationDetectImport(id, revision, keys)),
      romScannersList: () => native(commands.romScannersList),
      romScannerSave: (scanner) => native(() => commands.romScannerSave(scanner)),
      romScannerDelete: (id) => native(() => commands.romScannerDelete(id)),
      romScanStart: (scannerId, mode) => native(() => commands.romScanStart(scannerId, mode)),
      romScanJobs: () => native(async () => ok(await commands.romScanJobs())),
      romScanCancel: (id) => native(() => commands.romScanCancel(id)),
      romScanSubmit: (id, revision, choices) =>
        native(() => commands.romScanSubmit(id, revision, choices)),
      romScanAll: () => native(commands.romScanAll),
      romFolderSetup: (folder) => native(() => commands.romFolderSetup(folder)),
      romExclusions: () => native(commands.romExclusions),
      romExclusionsSet: (patterns) => native(() => commands.romExclusionsSet(patterns)),
    },
    {
      subscribe: async (listener) =>
        isTauri() ? events.emulationChanged.listen(() => listener()) : () => {},
      subscribeDetection: async (listener) =>
        isTauri()
          ? events.emulationDetectChanged.listen(({ payload }) => listener(payload))
          : () => {},
      subscribeScans: async (listener) =>
        isTauri()
          ? events.emulationScanChanged.listen(({ payload }) => listener(payload))
          : () => {},
      choosePath: (kind) =>
        native(async () => {
          const selected = await open({
            directory: kind === "folder",
            multiple: false,
            title: kind === "folder" ? t`Choose a folder` : t`Choose a file`,
          });
          if (selected !== null && typeof selected !== "string")
            throw new Error(t`Choose one path.`);
          return selected;
        }),
      setup: createNativeEmulatorSetupApi(),
    },
  );
}

export async function connectEmulationApi(): Promise<EmulationApi> {
  // Vite eliminates this import and fixture bytes from production builds.
  if (import.meta.env.DEV && import.meta.env.VITE_CASKADE_FIXTURES === "1") {
    const { createEmulationFixtureApi } = await import("./emulationFixtures");
    return createEmulationFixtureApi();
  }
  return createNativeEmulationApi();
}
