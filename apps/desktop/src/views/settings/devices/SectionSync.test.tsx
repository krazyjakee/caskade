/** A synced settings section kept on this device (#244), from the section
 * itself: Appearance, the rating board and RomM servers. */
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.setConfig({ testTimeout: 30_000 });

/** The whole app re-renders on each write; a loaded machine needs time. */
const slow = { timeout: 5_000 };

import { createFixtureApi } from "../../../api/fixtures";
import { createLibraryQueryClient, LibraryProvider } from "../../../api/provider";
import { SyncProvider } from "../../../api/sync";
import { createSyncFixtureApi } from "../../../api/syncFixtures";
import { fakeLayout, fakeViewport, renderApp } from "../../../test/harness";
import { createSettingsFixture } from "../../../test/settingsFixture";
import { SettingsProvider } from "../useSettings";
import { SectionSync } from "./SectionSync";

let undoLayout = () => {};
beforeEach(() => {
  undoLayout = fakeLayout();
  fakeViewport();
});
afterEach(() => {
  undoLayout();
  cleanup();
});

const withDevices = (path: string, options: Parameters<typeof renderApp>[1] = {}) =>
  renderApp(path, {
    ...options,
    wrap: (children: ReactNode) => (
      <SyncProvider api={createSyncFixtureApi()}>{children}</SyncProvider>
    ),
  });

const sections = (app: ReturnType<typeof renderApp>) =>
  (app.settings.values["core.sync"] as { sections?: Record<string, string> }).sections ?? {};

it("keeps Appearance on this device from Appearance itself, and back", async () => {
  const app = withDevices("/settings?tab=appearance");
  const toggle = await screen.findByRole("switch", { name: "Same on your other devices" }, slow);
  expect((toggle as HTMLInputElement).checked).toBe(true);
  expect(screen.getByText("Changes here reach your other devices.")).toBeTruthy();
  fireEvent.click(toggle);
  await waitFor(() => expect(sections(app)["core.appearance"]).toBe("thisDeviceOnly"), slow);
  await screen.findByText(/Kept on this device/, {}, slow);
  fireEvent.click(screen.getByRole("switch", { name: "Same on your other devices" }));
  await waitFor(() => expect(sections(app)["core.appearance"]).toBe("sync"), slow);
});

it("the rating board and RomM servers have the same choice", async () => {
  const app = withDevices("/settings?tab=accounts");
  fireEvent.click(await screen.findByRole("switch", { name: "Same on your other devices" }, slow));
  await waitFor(() => expect(sections(app)["core.accounts"]).toBe("thisDeviceOnly"), slow);
  cleanup();
  // RomM servers' section (Settings → Emulation → RomM), on its own.
  const fixture = createSettingsFixture();
  render(
    <LibraryProvider api={createFixtureApi()} client={createLibraryQueryClient()}>
      <SettingsProvider api={fixture.api}>
        <SyncProvider api={createSyncFixtureApi()}>
          <SectionSync namespace="core.romm" />
        </SyncProvider>
      </SettingsProvider>
    </LibraryProvider>,
  );
  fireEvent.click(await screen.findByRole("switch", { name: "Same on your other devices" }, slow));
  await waitFor(
    () =>
      expect(
        (fixture.values["core.sync"] as { sections?: Record<string, string> }).sections?.[
          "core.romm"
        ],
      ).toBe("thisDeviceOnly"),
    slow,
  );
});

// The core refuses `core.sync` changes from anyone but an admin
// (`permissions.rs`); the switch is disabled for them too (`useDeviceAdmin`).
it("a category kept here says so", async () => {
  const app = withDevices("/settings?tab=appearance");
  await screen.findByRole("switch", { name: "Same on your other devices" }, slow);
  // Appearance as a whole made This device only in Settings → Devices.
  const current = await app.settings.api.get("core.sync");
  const value = structuredClone(current.value) as { categories: Record<string, string> };
  value.categories.appearance = "thisDeviceOnly";
  await act(() => app.settings.api.set("core.sync", value, current.revision));
  await screen.findByText(/Not synced: this kind of setting is This device only/, {}, slow);
  const off = screen.getByRole("switch", {
    name: "Same on your other devices",
  }) as HTMLInputElement;
  expect(off.checked).toBe(false);
  expect(off.disabled).toBe(true);
});

it("without paired devices there's nothing to choose", async () => {
  renderApp("/settings?tab=appearance");
  await screen.findByText("Interface scale");
  expect(screen.queryByRole("switch", { name: "Same on your other devices" })).toBeNull();
});
