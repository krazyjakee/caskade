import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { EmulationApi, EmulationSnapshot } from "../../../api/emulation";
import { createEmulationFixtureApi } from "../../../api/emulationFixtures";
import { EmulationPanel, type EmulationSection } from "./EmulationPanel";
import { EmulationProvider } from "./EmulationProvider";

afterEach(cleanup);
function Panel() {
  const [section, setSection] = useState<EmulationSection>("emulators");
  return <EmulationPanel section={section} setSection={setSection} />;
}
/** The fixture's one-click install backend. */
function setupOf(api: EmulationApi) {
  if (!api.setup) throw new Error("the fixture has no setup API");
  return api.setup;
}
/** The list item around `text`. */
function itemOf(list: HTMLElement, text: string) {
  const item = [...list.querySelectorAll("li")].find((li) => li.textContent?.includes(text));
  if (!item) throw new Error(`no item with ${text}`);
  return item;
}
function mount(api: EmulationApi = createEmulationFixtureApi()) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Infinity } },
  });
  return render(
    <QueryClientProvider client={client}>
      <EmulationProvider api={api}>
        <Panel />
      </EmulationProvider>
    </QueryClientProvider>,
  );
}
it("shows loading, actionable errors and the real empty state", async () => {
  const api = createEmulationFixtureApi();
  const load = vi
    .spyOn(api, "snapshot")
    .mockImplementationOnce(() => new Promise<EmulationSnapshot>(() => {}));
  const view = mount(api);
  expect(screen.getByRole("status").textContent).toBe("Loading emulators…");
  view.unmount();
  load.mockRejectedValue(new Error("Fixture backend unavailable"));
  const failed = mount(api);
  await screen.findByText("Fixture backend unavailable");
  expect(screen.getByRole("button", { name: "Try again" })).toBeTruthy();
  failed.unmount();
  load.mockRestore();
  const snapshot = await api.snapshot();
  vi.spyOn(api, "snapshot").mockResolvedValue({
    ...snapshot,
    emulators: [],
    stats: { ...snapshot.stats, emulators: 0 },
  });
  mount(api);
  await screen.findByText("No emulators yet. Detect installed emulators or add one by hand.");
});
it("imports retained detection choices and updates emulator/platform sections without reload", async () => {
  const api = createEmulationFixtureApi();
  const imported = vi.spyOn(api, "importDetected");
  mount(api);
  fireEvent.click(await screen.findByRole("button", { name: "Detect emulators" }));
  const already = await screen.findByRole("checkbox", { name: /RetroArch/ });
  expect((already as HTMLInputElement).disabled).toBe(true);
  expect((already as HTMLInputElement).checked).toBe(false);
  expect((screen.getByRole("checkbox", { name: /PCSX2/ }) as HTMLInputElement).checked).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Import selected (1)" }));
  await screen.findByRole("option", { name: /PCSX2/ });
  expect(imported).toHaveBeenCalledWith(["fixture-pcsx2-install"], 1);
  fireEvent.click(screen.getByRole("radio", { name: "Platforms" }));
  await screen.findByText("PlayStation 2");
  expect(
    screen.getByRole("button", { name: /Default emulator for PlayStation 2/ }).textContent,
  ).toContain("PCSX2 · PS2");
});
it("uses the optional detection folder and preserves it when Browse is cancelled", async () => {
  const api = createEmulationFixtureApi();
  const detect = vi.spyOn(api, "detect");
  vi.spyOn(api, "choosePath").mockResolvedValue(null);
  mount(api);
  fireEvent.click(await screen.findByRole("button", { name: "Detect emulators" }));
  const folder = await screen.findByRole("textbox", { name: "Extra folder" });
  fireEvent.change(folder, { target: { value: "/fixture/portable-emulators" } });
  fireEvent.click(screen.getByRole("button", { name: "Browse for Extra folder" }));
  await waitFor(() => expect(api.choosePath).toHaveBeenCalledWith("folder"));
  expect((folder as HTMLInputElement).value).toBe("/fixture/portable-emulators");
  fireEvent.click(screen.getByRole("button", { name: "Search again" }));
  await waitFor(() => expect(detect).toHaveBeenLastCalledWith("/fixture/portable-emulators"));
});
it("saves an emulator using separate folder/executable fields and backend-assigned identity", async () => {
  const api = createEmulationFixtureApi();
  const saved = vi.spyOn(api, "saveEmulator");
  mount(api);
  const header = await screen.findByRole("region", { name: "Emulation overview" });
  fireEvent.click(within(header).getByRole("button", { name: "Add emulator" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Emulator name" }), {
    target: { value: "My portable emulator" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Browse for Emulator folder" }));
  await waitFor(() =>
    expect(
      (screen.getByRole("textbox", { name: "Emulator folder" }) as HTMLInputElement).value,
    ).toBe("/fixture/tools"),
  );
  fireEvent.click(screen.getByRole("button", { name: "Browse for Emulator executable" }));
  await waitFor(() =>
    expect(
      (screen.getByRole("textbox", { name: "Emulator executable" }) as HTMLInputElement).value,
    ).toBe("/fixture/tools/emulator"),
  );
  fireEvent.click(screen.getByRole("button", { name: "Save emulator" }));
  await screen.findByRole("option", { name: /My portable emulator/ });
  expect(saved).toHaveBeenCalledWith({
    id: null,
    name: "My portable emulator",
    definition: null,
    install: { type: "folder", dir: "/fixture/tools", executable: "/fixture/tools/emulator" },
  });
});
it("edits argument modes and inserts variables while preserving argv token boundaries", async () => {
  const api = createEmulationFixtureApi();
  const saved = vi.spyOn(api, "saveProfile");
  mount(api);
  fireEvent.click(await screen.findByRole("button", { name: /^SNES \(Snes9x\)/ }));
  fireEvent.click(screen.getByRole("button", { name: "Edit SNES (Snes9x)" }));
  fireEvent.click(screen.getByRole("radio", { name: "Append" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Extra arguments" }), {
    target: { value: "--fullscreen" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Insert {RomPath}: Full path of the ROM" }));
  fireEvent.click(screen.getByRole("button", { name: "Save profile" }));
  await waitFor(() => expect(saved).toHaveBeenCalledOnce());
  await waitFor(() => expect(screen.queryByRole("button", { name: "Save profile" })).toBeNull());
  const record = (await api.snapshot()).emulators[0]?.profiles.find(
    (row) => row.config.id === "fixture-snes",
  );
  expect(record?.config.type === "built_in" && record.config.args).toEqual({
    type: "append",
    args: ["--fullscreen", "{RomPath}"],
  });
  expect(record?.resolved.args.at(-1)).toBe("/fixture/ROMs/snes/Sample Game (USA).sfc");
});
it("keeps invalid custom drafts visible, then saves platforms, environment and hooks", async () => {
  const api = createEmulationFixtureApi();
  mount(api);
  fireEvent.click(await screen.findByRole("button", { name: "Add custom profile" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Arguments" }), {
    target: { value: '"{Unknown}"' },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save profile" }));
  await screen.findByText("Unknown variable {Unknown}.");
  expect(screen.getByRole("button", { name: "Save profile" })).toBeTruthy();
  expect((await api.snapshot()).emulators[0]?.profiles).toHaveLength(2);
  fireEvent.change(screen.getByRole("textbox", { name: "Arguments" }), {
    target: { value: '"{RomPath}" --limit 60' },
  });
  fireEvent.change(screen.getByRole("textbox", { name: "Profile name" }), {
    target: { value: "Validated custom profile" },
  });
  fireEvent.click(screen.getByRole("checkbox", { name: "Super Nintendo" }));
  fireEvent.click(screen.getByRole("button", { name: "Add environment variable" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Environment name 1" }), {
    target: { value: "FIXTURE" },
  });
  fireEvent.change(screen.getByRole("textbox", { name: "Environment value 1" }), {
    target: { value: "with spaces" },
  });
  fireEvent.change(screen.getByRole("textbox", { name: "Before launch" }), {
    target: { value: "echo ready" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save profile" }));
  await waitFor(() => expect(screen.queryByRole("button", { name: "Save profile" })).toBeNull());
  const profile = (await api.snapshot()).emulators[0]?.profiles.find(
    (row) => row.config.type === "custom",
  )?.config;
  expect(profile?.type === "custom" && profile.args).toEqual(["{RomPath}", "--limit", "60"]);
  expect(profile?.type === "custom" && profile.env).toEqual([["FIXTURE", "with spaces"]]);
  expect(profile?.hooks.before).toEqual(["echo", "ready"]);
});
it("offers compatible defaults only and saves an explicit BIOS folder", async () => {
  const api = createEmulationFixtureApi();
  const save = vi.spyOn(api, "setPlatform");
  mount(api);
  fireEvent.click(await screen.findByRole("radio", { name: "Platforms" }));
  const row = (await screen.findByText("Super Nintendo")).closest("tr");
  if (!row) throw new Error("platform row missing");
  fireEvent.click(within(row).getByRole("button", { name: /Default emulator/ }));
  expect(await screen.findByRole("option", { name: "RetroArch · SNES (Snes9x)" })).toBeTruthy();
  expect(screen.queryByRole("option", { name: /Beetle/ })).toBeNull();
  fireEvent.click(screen.getByRole("option", { name: "None" }));
  await waitFor(() => expect(save).toHaveBeenCalledWith("snes", null, null));
  fireEvent.change(within(row).getByRole("textbox", { name: "BIOS folder for SNES" }), {
    target: { value: "/fixture/firmware" },
  });
  fireEvent.click(within(row).getByRole("button", { name: "Save BIOS folder" }));
  await waitFor(() => expect(save).toHaveBeenLastCalledWith("snes", null, "/fixture/firmware"));
});
it("adds an available built-in profile and reports unknown BIOS status honestly", async () => {
  const api = createEmulationFixtureApi();
  const original = api.snapshot.bind(api);
  vi.spyOn(api, "snapshot").mockImplementation(async () => {
    const data = await original();
    return { ...data, stats: { ...data.stats, biosIssues: null } };
  });
  mount(api);
  await screen.findByText("Not checked");
  fireEvent.click(screen.getByRole("button", { name: "Add built-in profile" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: /GBA \(mGBA\)/ }));
  fireEvent.click(screen.getByRole("button", { name: "Save profile" }));
  await screen.findByRole("button", { name: /^GBA \(mGBA\)/ });
});
it("flags a drifted Replace override and records the reviewed arguments on save", async () => {
  const api = createEmulationFixtureApi();
  const config = (await api.snapshot()).emulators[0]?.profiles.find(
    (row) => row.config.id === "fixture-snes",
  )?.config;
  if (config?.type !== "built_in") throw new Error("fixture profile missing");
  await api.saveProfile("fixture-retroarch", {
    profile: { ...config, args: { type: "replace", args: [], replaced: ["--old"] } },
    argsText: '"{RomPath}"',
    beforeText: "",
    afterExitText: "",
  });
  mount(api);
  fireEvent.click(await screen.findByRole("button", { name: /^SNES \(Snes9x\)/ }));
  await screen.findByText(/Edit the profile to review them/);
  fireEvent.click(screen.getByRole("button", { name: "Edit SNES (Snes9x)" }));
  expect(screen.getByText(/Replaced then: --old/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Keep my arguments" }));
  fireEvent.click(screen.getByRole("button", { name: "Save profile" }));
  await waitFor(() => expect(screen.queryByText(/Edit the profile to review them/)).toBeNull());
  const saved = (await api.snapshot()).emulators[0]?.profiles.find(
    (row) => row.config.id === "fixture-snes",
  );
  expect(saved?.config.type === "built_in" && saved.config.args).toEqual({
    type: "replace",
    args: ["{RomPath}"],
    replaced: ["-L", "{cores}/snes9x.so", "{RomPath}"],
  });
  expect(saved?.resolved.drift).toBeNull();
});
it("installs the recommended emulator for a platform with games and none to play them", async () => {
  const api = createEmulationFixtureApi();
  const installFor = vi.spyOn(setupOf(api), "installFor");
  mount(api);
  const missing = await screen.findByRole("region", { name: "2 platforms have no emulator" });
  expect(within(missing).getByText("12 games · Recommended: DuckStation")).toBeTruthy();
  expect(within(missing).getByText(/can't install an emulator for Switch/)).toBeTruthy();
  fireEvent.click(within(missing).getByRole("button", { name: "Install DuckStation" }));
  await waitFor(() => expect(installFor).toHaveBeenCalledWith("sony_playstation"));
  // Sizes come from Intl, with a no-break space before the unit.
  await within(missing).findByText(/^Downloading 0\.0\skB of 40\.0\sMB$/);
  // Done: the platform has an emulator now.
  await screen.findByRole("region", { name: "1 platform has no emulator" });
  expect(screen.queryByText("Sony PlayStation")).toBeNull();
});
it("browses the emulators Caskade can install and installs one", async () => {
  const api = createEmulationFixtureApi();
  const install = vi.spyOn(setupOf(api), "install");
  const view = mount(api);
  fireEvent.click(await screen.findByRole("radio", { name: "Get emulators" }));
  const list = await screen.findByRole("list", { name: "Emulators to install" });
  const dolphin = itemOf(list, "Dolphin");
  expect(within(dolphin).getByText("Updated by Flatpak")).toBeTruthy();
  const mgba = itemOf(list, "mGBA");
  expect(within(mgba).getByText("Updates automatically")).toBeTruthy();
  fireEvent.click(within(mgba).getByRole("button", { name: "Install mGBA" }));
  await waitFor(() => expect(install).toHaveBeenCalledWith("mgba"));
  await within(mgba).findByText("Installed v1.0");
  await within(mgba).findByRole("button", { name: "Reinstall mGBA" });
  view.unmount();
});
it("leaves out one-click installs when the backend has none", async () => {
  const api = createEmulationFixtureApi();
  delete api.setup;
  mount(api);
  await screen.findByRole("listbox", { name: "Emulators" });
  expect(screen.queryByRole("region", { name: /no emulator/ })).toBeNull();
  fireEvent.click(screen.getByRole("radio", { name: "Get emulators" }));
  await screen.findByText("Caskade can't install emulators on this system yet.");
});
it("offers a newer version of a Caskade-installed emulator, with Skip", async () => {
  const api = createEmulationFixtureApi();
  const update = vi.spyOn(setupOf(api), "update");
  mount(api);
  fireEvent.click(await screen.findByRole("radio", { name: "Get emulators" }));
  const list = await screen.findByRole("list", { name: "Emulators to install" });
  const ppsspp = itemOf(list, "PPSSPP");
  expect(within(ppsspp).getByText("Installed v1.20.3")).toBeTruthy();
  fireEvent.click(within(ppsspp).getByRole("button", { name: "Skip PPSSPP v1.20.4" }));
  await within(ppsspp).findByRole("button", { name: "Reinstall PPSSPP" });
  expect(update).not.toHaveBeenCalled();
});
it("updates a Caskade-installed emulator to the newest version", async () => {
  const api = createEmulationFixtureApi();
  const update = vi.spyOn(setupOf(api), "update");
  mount(api);
  fireEvent.click(await screen.findByRole("radio", { name: "Get emulators" }));
  const list = await screen.findByRole("list", { name: "Emulators to install" });
  const ppsspp = itemOf(list, "PPSSPP");
  fireEvent.click(within(ppsspp).getByRole("button", { name: "Update PPSSPP to v1.20.4" }));
  await waitFor(() => expect(update).toHaveBeenCalledWith("managed-ppsspp"));
  await within(ppsspp).findByText("Installed v1.0");
});
it("gets a platform's missing BIOS files from RomM", async () => {
  const api = createEmulationFixtureApi();
  const fetchBios = vi.spyOn(setupOf(api), "fetchBios");
  await api.setPlatform("ps1", { emulatorId: "fixture-retroarch", profileId: "fixture-ps1" }, null);
  mount(api);
  fireEvent.click(await screen.findByRole("radio", { name: "Platforms" }));
  const row = (await screen.findByText("PlayStation")).closest("tr") as HTMLElement;
  fireEvent.click(within(row).getByRole("button", { name: "Get from RomM" }));
  await within(row).findByText("Got scph5500.bin.");
  expect(fetchBios).toHaveBeenCalledWith("ps1");
});
