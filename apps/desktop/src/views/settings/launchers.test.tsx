/** #48: Launchers lists what discovery looks for on this device, with
 * readable names and whether each was found, and its switches write the
 * source ids discovery reads. */
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { DiscoveryApi, DiscoverySnapshot } from "../../api/discovery";
import { createSettingsFixture } from "../../test/settingsFixture";
import { DiscoveryProvider } from "./discovery/DiscoveryProvider";
import { LaunchersSettings } from "./LaunchersSettings";
import { SettingsProvider } from "./useSettings";

afterEach(cleanup);

type Detected = DiscoverySnapshot["launchers"];

const detected: Detected = [
  { source: "lutris", found: false, path: null, version: null, kind: null },
  { source: "steam", found: true, path: "/fixture/Steam", version: "1.0", kind: null },
  { source: "legendary", found: false, path: null, version: null, kind: null },
  { source: "rare", found: true, path: "/fixture/Rare", version: null, kind: null },
];

function discovery(launchers: Detected) {
  const snapshot: DiscoverySnapshot = {
    revision: 1,
    os: "linux",
    checks: [],
    phase: "complete",
    launchers,
    sources: [],
    found: 0,
    imported: 0,
    review: [],
    warnings: [],
    error: null,
    watching: false,
  };
  const api = {
    status: async () => snapshot,
    launchers: vi.fn(async () => ({ ...snapshot, revision: 2, launchers: detected })),
    scan: async () => snapshot,
    cancel: async () => snapshot,
    review: async () => snapshot,
    subscribe: async () => () => {},
  } satisfies DiscoveryApi;
  return api;
}

function mount(api: DiscoveryApi) {
  const settings = createSettingsFixture();
  render(
    <QueryClientProvider client={new QueryClient()}>
      <SettingsProvider api={settings.api}>
        <DiscoveryProvider api={api}>
          <LaunchersSettings />
        </DiscoveryProvider>
      </SettingsProvider>
    </QueryClientProvider>,
  );
  return settings;
}

const rows = () =>
  [...document.querySelectorAll('[data-part="launcher"]')].map((row) => ({
    name: row.querySelector("p")?.textContent,
    status: row.getAttribute("data-status"),
  }));

it("lists this device's launchers by name, found ones first, each once", async () => {
  mount(discovery(detected));
  await screen.findByText("/fixture/Steam · 1.0");
  expect(rows()).toEqual([
    { name: "Steam", status: "detected" },
    // Rare's games are in Legendary's records: one launcher.
    { name: "Legendary / Rare", status: "detected" },
    { name: "Lutris", status: "not-found" },
  ]);
  expect(screen.getByText("/fixture/Rare")).toBeTruthy();
  const lutris = screen.getByText("Lutris").closest("li") as HTMLElement;
  expect(within(lutris).getByText("Not found on this device")).toBeTruthy();
  // The fixture's settings have Lutris off.
  expect((within(lutris).getByRole("switch") as HTMLInputElement).checked).toBe(false);
});

it("a switch turns every source of its launcher off", async () => {
  const settings = mount(discovery(detected));
  fireEvent.click(await screen.findByRole("switch", { name: "Legendary / Rare" }));
  await waitFor(() =>
    expect(settings.values["core.launchers"]?.enabled).toMatchObject({
      legendary: false,
      rare: false,
      steam: true,
    }),
  );
});

it("looks for launchers when nothing was detected yet", async () => {
  const api = discovery([]);
  mount(api);
  await waitFor(() => expect(api.launchers).toHaveBeenCalledTimes(1));
  await screen.findByRole("switch", { name: "Steam" });
});

it("resets after saying what changes", async () => {
  const settings = mount(discovery(detected));
  fireEvent.click(await screen.findByRole("switch", { name: "Steam" }));
  await waitFor(() =>
    expect(settings.values["core.launchers"]?.enabled).toMatchObject({ steam: false }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Reset launcher settings" }));
  const confirm = await screen.findByRole("alertdialog", { name: "Reset Launchers settings?" });
  await within(confirm).findByText("Which launchers are scanned");
  fireEvent.click(within(confirm).getByRole("button", { name: "Reset" }));
  await waitFor(() =>
    expect(settings.values["core.launchers"]?.enabled).toMatchObject({ steam: true }),
  );
});
