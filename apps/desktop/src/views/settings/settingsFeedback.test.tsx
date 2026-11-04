/** Settings that didn't load or save say so beside their section (#42), and
 * discovery's custom folder can be browsed for or dropped (#47). */
import { QueryClient } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { BackupProvider } from "../../api/backup";
import { createBackupFixtureApi } from "../../api/backupFixtures";
import { createFixtureApi } from "../../api/fixtures";
import { LibraryProvider } from "../../api/provider";
import { DropProvider } from "../../drop/DropProvider";
import { fakeDrops } from "../../drop/testing";
import { createDiscoveryFixture, createSettingsFixture } from "../../test/settingsFixture";
import { BackupSettings } from "./backup/BackupSettings";
import { SyncChoices } from "./devices/SyncChoices";
import { DiscoveryProvider } from "./discovery/DiscoveryProvider";
import { LibraryDiscoverySettings } from "./LibraryDiscoverySettings";
import { SettingsProvider } from "./useSettings";

const dialogOpen = vi.hoisted(() => vi.fn<() => Promise<string | null>>());
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: dialogOpen }));

afterEach(() => {
  cleanup();
  dialogOpen.mockReset();
});

/** The core's settings, whose reads or writes of one section can fail. */
function settingsBackend(namespace: string) {
  const fixture = createSettingsFixture();
  const { get, set } = fixture.api;
  const failing = { reads: false, writes: 0 };
  fixture.api.get = async (name) => {
    if (failing.reads && name === namespace) throw new Error("The settings file is locked");
    return get(name);
  };
  fixture.api.set = async (name, value, revision) => {
    if (failing.writes > 0 && name === namespace) {
      failing.writes--;
      throw new Error("The disk is full");
    }
    return set(name, value, revision);
  };
  return { ...fixture, failing };
}

/** One panel over the settings backend, without accounts (an admin's view). */
function renderPanel(
  settings: ReturnType<typeof settingsBackend>,
  panel: ReactNode,
  drop: () => Element | null = () => null,
) {
  const library = createFixtureApi();
  delete library.accounts;
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, networkMode: "always" },
      mutations: { retry: false, networkMode: "always" },
    },
  });
  const drops = fakeDrops(drop);
  render(
    <DropProvider source={drops.source} hitTest={drops.hitTest}>
      <LibraryProvider api={library} client={client}>
        <SettingsProvider api={settings.api}>
          <BackupProvider api={createBackupFixtureApi()}>
            <DiscoveryProvider api={createDiscoveryFixture()}>{panel}</DiscoveryProvider>
          </BackupProvider>
        </SettingsProvider>
      </LibraryProvider>
    </DropProvider>,
  );
  return drops;
}

const checked = (element: HTMLElement) => element.getAttribute("aria-checked") === "true";

it("a backup schedule that didn't save says so, keeps the saved one and retries the change", async () => {
  const settings = settingsBackend("core.backups");
  renderPanel(settings, <BackupSettings />);
  const daily = await screen.findByRole("radio", { name: "Daily" });
  settings.failing.writes = 1;
  fireEvent.click(daily);
  const notice = (await screen.findByText("Automatic backups: your change didn't save")).closest(
    '[role="alert"]',
  ) as HTMLElement;
  expect(notice.textContent).toContain("The disk is full");
  expect(settings.values["core.backups"]?.schedule).toBe("off");
  await waitFor(() => expect(checked(screen.getByRole("radio", { name: "Off" }))).toBe(true));
  expect(checked(screen.getByRole("radio", { name: "Daily" }))).toBe(false);
  fireEvent.click(within(notice).getByRole("button", { name: "Retry" }));
  await waitFor(() => expect(settings.values["core.backups"]?.schedule).toBe("daily"));
  await waitFor(() => expect(checked(screen.getByRole("radio", { name: "Daily" }))).toBe(true));
  expect(screen.queryByText("Automatic backups: your change didn't save")).toBeNull();
});

it("backup settings that didn't load say so with Try again, instead of vanishing", async () => {
  const settings = settingsBackend("core.backups");
  settings.failing.reads = true;
  renderPanel(settings, <BackupSettings />);
  const notices = await screen.findAllByText("Backup settings couldn't be loaded");
  expect(notices.length).toBeGreaterThan(0);
  expect(screen.queryByRole("radio", { name: "Daily" })).toBeNull();
  settings.failing.reads = false;
  const notice = notices[0]?.closest('[role="alert"]') as HTMLElement;
  fireEvent.click(within(notice).getByRole("button", { name: "Try again" }));
  expect(await screen.findByRole("radio", { name: "Daily" })).toBeTruthy();
  expect(screen.queryByText("Backup settings couldn't be loaded")).toBeNull();
});

it("what syncs: a change that didn't save keeps the saved choice, and Retry saves it", async () => {
  const settings = settingsBackend("core.sync");
  renderPanel(settings, <SyncChoices />);
  const library = await screen.findByRole("radiogroup", { name: "Library" });
  settings.failing.writes = 1;
  fireEvent.click(within(library).getByRole("radio", { name: "This device only" }));
  const notice = (await screen.findByText("What syncs: your change didn't save")).closest(
    '[role="alert"]',
  ) as HTMLElement;
  expect(notice.textContent).toContain("The disk is full");
  await waitFor(() =>
    expect(checked(within(library).getByRole("radio", { name: "Sync" }))).toBe(true),
  );
  fireEvent.click(within(notice).getByRole("button", { name: "Retry" }));
  const categories = () => settings.values["core.sync"]?.categories as Record<string, string>;
  await waitFor(() => expect(categories().library).toBe("thisDeviceOnly"));
  await waitFor(() =>
    expect(checked(within(library).getByRole("radio", { name: "This device only" }))).toBe(true),
  );
});

it("what syncs that didn't load says so with Try again", async () => {
  const settings = settingsBackend("core.sync");
  settings.failing.reads = true;
  renderPanel(settings, <SyncChoices />);
  const title = await screen.findByText("What syncs couldn't be loaded");
  settings.failing.reads = false;
  fireEvent.click(
    within(title.closest('[role="alert"]') as HTMLElement).getByRole("button", {
      name: "Try again",
    }),
  );
  expect(await screen.findByRole("radiogroup", { name: "Library" })).toBeTruthy();
});

it("Browse fills the custom folder field, a cancel changes nothing, and Add folder adds it", async () => {
  const settings = settingsBackend("core.discovery");
  renderPanel(settings, <LibraryDiscoverySettings />);
  const field = (await screen.findByRole("textbox", {
    name: "Absolute custom folder",
  })) as HTMLInputElement;
  dialogOpen.mockResolvedValueOnce(null);
  fireEvent.click(screen.getByRole("button", { name: "Browse…" }));
  await waitFor(() => expect(dialogOpen).toHaveBeenCalledOnce());
  await Promise.resolve();
  expect(field.value).toBe("");
  expect(settings.writes).not.toContain("core.discovery");

  dialogOpen.mockResolvedValueOnce("/home/player/Games");
  fireEvent.click(screen.getByRole("button", { name: "Browse…" }));
  await waitFor(() => expect(field.value).toBe("/home/player/Games"));
  // Picking only fills the field; Add folder adds it.
  expect(settings.values["core.discovery"]?.scanPaths).toEqual([]);
  fireEvent.click(screen.getByRole("button", { name: "Add folder" }));
  await waitFor(() =>
    expect(settings.values["core.discovery"]?.scanPaths).toEqual(["/home/player/Games"]),
  );
});

it("a folder dropped on the custom folder field fills it in", async () => {
  const settings = settingsBackend("core.discovery");
  const drops = renderPanel(settings, <LibraryDiscoverySettings />, () =>
    screen.getByRole("textbox", { name: "Absolute custom folder" }),
  );
  const field = (await screen.findByRole("textbox", {
    name: "Absolute custom folder",
  })) as HTMLInputElement;
  drops.drop("/run/media/player/SD Card/Games");
  await waitFor(() => expect(field.value).toBe("/run/media/player/SD Card/Games"));
  expect(settings.values["core.discovery"]?.scanPaths).toEqual([]);
});
