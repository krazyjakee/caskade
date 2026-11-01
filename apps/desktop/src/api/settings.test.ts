import { expect, it, vi } from "vitest";
import { createSettingsApi, type SettingsSnapshot, type SettingsTransport } from "./settings";

it("maps typed command replies and preserves revisions and backend validation errors", async () => {
  const snapshot: SettingsSnapshot = {
    namespace: "core.general",
    version: 1,
    revision: 3,
    value: { confirmQuit: true },
  };
  const transport: SettingsTransport = {
    settingsSections: vi.fn(async () => ({ status: "ok" as const, data: [] })),
    settingsGet: vi.fn(async () => ({ status: "ok" as const, data: snapshot })),
    settingsSet: vi.fn(async () => ({ status: "error" as const, error: "Invalid preference" })),
    settingsReset: vi.fn(async () => ({
      status: "ok" as const,
      data: { ...snapshot, revision: 4 },
    })),
  };
  const api = createSettingsApi(transport, async () => () => {});
  expect(await api.get("core.general")).toEqual(snapshot);
  await expect(api.set("core.general", { confirmQuit: false }, 3)).rejects.toThrow(
    "Invalid preference",
  );
  expect(transport.settingsSet).toHaveBeenCalledWith("core.general", { confirmQuit: false }, 3);
  expect((await api.reset("core.general", 3)).revision).toBe(4);
});
