/** Settings → General's Reset to defaults (#44) and its layout (#41). */
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { type AppUpdateApi, AppUpdateProvider, type AppUpdateStatus } from "../../api/appUpdate";
import type { SettingsApi } from "../../api/settings";
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
function general(change?: (api: SettingsApi) => Partial<SettingsApi>) {
  const fixture = createSettingsFixture({
    ...coreSettingsDefaults(),
    "core.metadata": { online: true },
    "core.appUpdates": { check: true },
    "core.onboarding": { completed: false, sounds: true },
  });
  (fixture.values["core.onboarding"] as Record<string, unknown>).completed = true;
  const wrap = (children: ReactNode) => (
    <AppUpdateProvider api={appUpdate}>
      <DefaultSettingsApi value={{ ...fixture.api, ...change?.(fixture.api) }}>
        {children}
      </DefaultSettingsApi>
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

it("says it can't tell what would change without defaults, and can still reset (#65)", async () => {
  const { fixture, open } = general(() => ({ sections: async () => [] }));
  Object.assign(fixture.values["core.general"] as object, { timeFormat: "24h" });
  open();

  const dialog = await openReset();
  await within(dialog).findByText(
    "Caskade can't tell what would change. Reset puts all of these settings back to their defaults.",
  );
  // Fields of other sections have no default to go back to here.
  expect(
    within(dialog).getByText(
      "Caskade can't tell what would change for some settings here, so those stay as they are.",
    ),
  ).toBeTruthy();
  // No list of every field, and no claim that nothing would change.
  expect(within(dialog).queryByRole("list")).toBeNull();
  expect(within(dialog).queryByText("Every setting here already uses its default.")).toBeNull();

  fireEvent.click(within(dialog).getByRole("button", { name: "Reset" }));
  await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
  expect(fixture.values["core.general"]).toEqual(coreSettingsDefaults()["core.general"]);
});

it("offers Reset only once it has checked what would change (#65)", async () => {
  general(() => ({ sections: () => new Promise(() => {}) })).open();
  const dialog = await openReset();
  await within(dialog).findByText("Checking what would change…");
  expect(within(dialog).queryByRole("button", { name: "Reset" })).toBeNull();
  expect(within(dialog).getByRole("button", { name: "Cancel" })).toBeTruthy();
});

it("a check that failed says so with Try again, and offers no Reset (#65)", async () => {
  let failing = true;
  const { fixture, open } = general((api) => ({
    sections: async () => {
      if (failing) throw new Error("the core didn't answer");
      return api.sections();
    },
  }));
  Object.assign(fixture.values["core.general"] as object, { confirmQuit: false });
  open();

  const dialog = await openReset();
  const text = await within(dialog).findByText("the core didn't answer", {}, { timeout: 5_000 });
  expect(within(dialog).queryByRole("button", { name: "Reset" })).toBeNull();
  failing = false;
  const notice = text.closest('[role="alert"]') as HTMLElement;
  fireEvent.click(within(notice).getByRole("button", { name: "Try again" }));
  await within(dialog).findByText("Confirm before quitting");
  expect(within(dialog).getByRole("button", { name: "Reset" })).toBeTruthy();
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
