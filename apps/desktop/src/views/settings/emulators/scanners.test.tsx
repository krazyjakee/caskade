import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { EmulationApi } from "../../../api/emulation";
import { createEmulationFixtureApi } from "../../../api/emulationFixtures";
import { EmulationPanel, type EmulationSection } from "./EmulationPanel";
import { EmulationProvider } from "./EmulationProvider";

afterEach(cleanup);
function Panel() {
  const [section, setSection] = useState<EmulationSection>("scanners");
  return <EmulationPanel section={section} setSection={setSection} />;
}
function mount(api: EmulationApi = createEmulationFixtureApi()) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <EmulationProvider api={api}>
        <Panel />
      </EmulationProvider>
    </QueryClientProvider>,
  );
  return api;
}
const review = () => screen.findByRole("region", { name: /Review scan results/ });

it("lists scanners with their profile, last scan and the Flatpak folder hint", async () => {
  mount();
  const row = (await screen.findByRole("button", { name: "SNES collection" })).closest("tr");
  if (!row) throw new Error("scanner row missing");
  expect(within(row).getByText("/fixture/ROMs/snes")).toBeTruthy();
  expect(within(row).getByText("RetroArch · SNES (Snes9x)")).toBeTruthy();
  expect(within(row).getByText("Never")).toBeTruthy();
  expect(await screen.findByText(/runs as a Flatpak and can't see this folder/)).toBeTruthy();
  expect(screen.getByRole("list", { name: "Exclusions" }).textContent).toContain("bios/");
});

it("reviews a manual scan: untick, merge and rename survive Scan all", async () => {
  const api = mount();
  const submit = vi.spyOn(api, "submitReview");
  fireEvent.click(await screen.findByRole("button", { name: "Scan SNES collection" }));
  const list = await review();
  // The fixture's two Star Ocean discs are already one game.
  expect(within(list).getByRole("list", { name: "ROMs of Star Ocean" }).children).toHaveLength(2);

  fireEvent.click(within(list).getByRole("checkbox", { name: "Select Star Ocean" }));
  fireEvent.click(within(list).getByRole("checkbox", { name: "Select Star Ocean Bonus" }));
  fireEvent.click(within(list).getByRole("button", { name: "Merge into one game" }));
  expect(within(list).queryByRole("checkbox", { name: "Select Star Ocean Bonus" })).toBeNull();
  fireEvent.change(
    within(list).getByRole("textbox", { name: "Title of Star Ocean (Japan) (Disc 1).sfc" }),
    { target: { value: "Star Ocean (Complete)" } },
  );
  const metroid = within(list)
    .getByRole("textbox", { name: "Title of Super Metroid (Japan, USA).sfc" })
    .closest("li");
  if (!metroid) throw new Error("review row missing");
  fireEvent.click(within(metroid).getByRole("checkbox", { name: "Import" }));
  fireEvent.click(within(list).getByRole("button", { name: "Import 3 games" }));

  await screen.findByText("Last scan: 3 added, 0 updated, 0 missing.");
  const [, , choices] = submit.mock.calls[0] ?? [];
  expect(choices?.find((choice) => choice.title === "Star Ocean (Complete)")?.roms).toEqual([
    "Star Ocean (Japan) (Disc 1).sfc",
    "Star Ocean (Japan) (Disc 2).sfc",
    "Star Ocean Bonus (Japan).sfc",
  ]);
  expect(choices?.find((choice) => choice.title === "Super Metroid")?.include).toBe(false);
  expect((await screen.findByRole("list", { name: "Exclusions" })).textContent).toContain(
    "Super Metroid (Japan, USA).sfc",
  );

  // The next automatic scan agrees: nothing new, the same three games.
  const header = screen.getByRole("region", { name: "Emulation overview" });
  fireEvent.click(within(header).getByRole("button", { name: "Scan all" }));
  await within(header).findByText("Scanned 1 scanner: 0 new games");
  const row = screen.getByRole("button", { name: "SNES collection" }).closest("tr");
  if (!row) throw new Error("scanner row missing");
  await waitFor(() => expect(within(row).getAllByText("3").length).toBeGreaterThan(0));
});

it("a new scanner is saved with every field and its first run is a review", async () => {
  const api = mount();
  const save = vi.spyOn(api, "saveScanner");
  fireEvent.click(await screen.findByRole("button", { name: "Add scanner" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Scanner name" }), {
    target: { value: "Imports" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Browse for ROM folder" }));
  await waitFor(() =>
    expect((screen.getByRole("textbox", { name: "ROM folder" }) as HTMLInputElement).value).toBe(
      "/fixture/tools",
    ),
  );
  fireEvent.change(screen.getByRole("textbox", { name: "New exclusion pattern" }), {
    target: { value: ">*.txt" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Add" }));
  fireEvent.click(screen.getByRole("switch", { name: /Include in library update/ }));
  fireEvent.click(screen.getByRole("button", { name: "Save and scan" }));
  await review();
  expect(save).toHaveBeenCalledWith(
    expect.objectContaining({
      id: "",
      name: "Imports",
      folder: "/fixture/tools",
      exclusions: [">*.txt"],
      inLibraryUpdate: false,
      profile: { emulatorId: "fixture-retroarch", profileId: "fixture-snes" },
      launchWith: { type: "platform_default" },
    }),
  );
  expect(await screen.findByRole("button", { name: "Imports" })).toBeTruthy();
});

it("shows backend validation errors without losing the draft", async () => {
  const api = mount();
  vi.spyOn(api, "saveScanner").mockRejectedValueOnce(new Error("scanner folder must be absolute"));
  fireEvent.click(await screen.findByRole("button", { name: "Add scanner" }));
  fireEvent.change(screen.getByRole("textbox", { name: "ROM folder" }), {
    target: { value: "relative/roms" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save and scan" }));
  expect((await screen.findByRole("alert")).textContent).toContain("must be absolute");
  expect((screen.getByRole("textbox", { name: "ROM folder" }) as HTMLInputElement).value).toBe(
    "relative/roms",
  );
});

it("global exclusions apply to every scanner from the next scan (#135)", async () => {
  const api = mount();
  const section = await screen.findByRole("region", { name: "Global exclusions" });
  const list = await within(section).findByRole("list", { name: "Global exclusions" });
  expect(list.textContent).toContain("No exclusions.");

  fireEvent.change(within(section).getByRole("textbox", { name: "New global exclusion pattern" }), {
    target: { value: " >Chrono* " },
  });
  fireEvent.click(within(section).getByRole("button", { name: "Add global exclusion" }));
  await waitFor(() => expect(list.textContent).toContain(">Chrono*"));
  expect(await api.globalExclusions()).toEqual([">Chrono*"]);

  // Scan all skips the excluded ROM: 4 of the fixture's 5 games.
  const header = screen.getByRole("region", { name: "Emulation overview" });
  fireEvent.click(within(header).getByRole("button", { name: "Scan all" }));
  await within(header).findByText("Scanned 1 scanner: 4 new games");

  // A pattern that doesn't compile is refused and shown.
  fireEvent.change(within(section).getByRole("textbox", { name: "New global exclusion pattern" }), {
    target: { value: "?(" },
  });
  fireEvent.click(within(section).getByRole("button", { name: "Add global exclusion" }));
  expect((await within(section).findByRole("alert")).textContent).toContain("?(");
  expect(await api.globalExclusions()).toEqual([">Chrono*"]);

  fireEvent.click(
    await within(section).findByRole("button", { name: "Remove global exclusion >Chrono*" }),
  );
  await waitFor(() => expect(list.textContent).toContain("No exclusions."));
  expect(await api.globalExclusions()).toEqual([]);
});

it("asks before removing a scanner, and says its games stay (#49)", async () => {
  const api = createEmulationFixtureApi();
  const remove = vi.spyOn(api, "removeScanner");
  mount(api);
  fireEvent.click(await screen.findByRole("button", { name: "SNES collection" }));
  fireEvent.click(await screen.findByRole("button", { name: "Remove" }));
  const confirm = await screen.findByRole("alertdialog", { name: "Remove SNES collection?" });
  expect(confirm.textContent).toContain("Its games stay in your library");
  expect(remove).not.toHaveBeenCalled();
  fireEvent.click(within(confirm).getByRole("button", { name: "Remove" }));
  await waitFor(() => expect(remove).toHaveBeenCalledTimes(1));
});
