/** Settings → General's Reset to defaults (#44) and its layout (#41). */
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { type AppUpdateApi, AppUpdateProvider, type AppUpdateStatus } from "../../api/appUpdate";
import { renderApp } from "../../test/harness";
import { coreSettingsDefaults, createSettingsFixture } from "../../test/settingsFixture";
import { DefaultSettingsApi } from "./useSettings";

vi.setConfig({ testTimeout: 20_000 });

afterEach(cleanup);

const status: AppUpdateStatus = {
  currentVersion: "0.1.0",
  unavailable: null,
  phase: "upToDate",
  release: null,
  checkedAt: null,
  downloaded: 0,
  total: null,
  error: null,
};
const appUpdate: AppUpdateApi = {
  status: async () => status,
  check: async () => status,
  install: async () => status,
  restart: async () => {},
  subscribe: () => () => {},
};

/** The sections General shows, at their defaults; first-run setup is done. */
function general() {
  const fixture = createSettingsFixture({
    ...coreSettingsDefaults(),
    "core.metadata": { online: true },
    "core.appUpdates": { check: true },
    "core.onboarding": { completed: false, sounds: true },
  });
  (fixture.values["core.onboarding"] as Record<string, unknown>).completed = true;
  const wrap = (children: ReactNode) => (
    <AppUpdateProvider api={appUpdate}>
      <DefaultSettingsApi value={fixture.api}>{children}</DefaultSettingsApi>
    </AppUpdateProvider>
  );
  return { fixture, open: () => renderApp("/settings?tab=general", { wrap }) };
}

async function openReset() {
  fireEvent.click(await screen.findByRole("button", { name: "Reset general settings" }));
  return screen.findByRole("alertdialog");
}

it("lists the page's changed settings, from every section it shows, and resets only those", async () => {
  const { fixture, open } = general();
  const { values } = fixture;
  Object.assign(values["core.general"] as object, { launchOnLogin: true, timeFormat: "24h" });
  Object.assign(values["core.console"] as object, { start: "desktop", active: true });
  Object.assign(values["core.metadata"] as object, { online: false });
  open();

  const dialog = await openReset();
  expect(within(dialog).getByRole("heading").textContent).toBe("Reset General settings?");
  await within(dialog).findByText("Start in");
  for (const label of [
    "Launch on system startup",
    "Time format",
    "Fetch artwork and game info online",
  ])
    expect(within(dialog).getByText(label)).toBeTruthy();
  // Settings already at their defaults aren't listed.
  expect(within(dialog).queryByText("Check automatically")).toBeNull();
  expect(within(dialog).queryByText("Open to")).toBeNull();
  // Nothing changes until it is confirmed.
  expect(fixture.writes).toEqual([]);

  fireEvent.click(within(dialog).getByRole("button", { name: "Reset" }));
  await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
  expect(values["core.general"]).toEqual(coreSettingsDefaults()["core.general"]);
  expect(values["core.console"]).toEqual({ start: "auto", active: true });
  expect(values["core.metadata"]).toEqual({ online: true });
  // First-run setup stays done, and untouched sections aren't written.
  expect(values["core.onboarding"]).toEqual({ completed: true, sounds: true });
  expect(fixture.writes.sort()).toEqual(["core.console", "core.general", "core.metadata"]);
});

it("changes nothing when cancelled, and says when everything is already default", async () => {
  const { fixture, open } = general();
  Object.assign(fixture.values["core.general"] as object, { confirmQuit: false });
  open();

  let dialog = await openReset();
  await within(dialog).findByText("Confirm before quitting");
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
  expect(fixture.writes).toEqual([]);
  expect((fixture.values["core.general"] as Record<string, unknown>).confirmQuit).toBe(false);

  cleanup();
  general().open();
  dialog = await openReset();
  await within(dialog).findByText("Every setting here already uses its default.");
  expect(within(dialog).queryByRole("button", { name: "Reset" })).toBeNull();
  expect(within(dialog).getByRole("button", { name: "Close" })).toBeTruthy();
});

it("chooses General's columns from the panel's own width (#41)", async () => {
  general().open();
  await screen.findByRole("button", { name: "Reset general settings" });
  const panel = document.querySelector('[data-settings="general"]') as HTMLElement;
  expect(panel.classList).toContain("@container");
  const grid = panel.firstElementChild as HTMLElement;
  expect(grid.classList).toContain("grid");
  expect(grid.className).toMatch(/(^| )@\w+:grid-cols-2( |$)/);
  // No viewport breakpoint, which ignores the app's and Settings' navigation.
  expect(grid.className).not.toMatch(/(^| )(sm|md|lg|xl|2xl):grid-cols-/);
});
