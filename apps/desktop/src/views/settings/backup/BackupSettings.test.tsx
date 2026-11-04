import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { type BackupApi, BackupProvider, type BackupStatus } from "../../../api/backup";
import { createBackupFixtureApi } from "../../../api/backupFixtures";
import { DropProvider } from "../../../drop/DropProvider";
import { fakeDrops } from "../../../drop/testing";
import { createSettingsFixture } from "../../../test/settingsFixture";
import { SettingsProvider } from "../useSettings";
import { BackupSettings } from "./BackupSettings";

afterEach(cleanup);

function renderPanel(
  initial?: Partial<BackupStatus>,
  drop?: () => Element | null,
  plan?: (planned: Awaited<ReturnType<BackupApi["plan"]>>) => object,
) {
  const api = createBackupFixtureApi(initial);
  if (plan) {
    const original = api.plan;
    api.plan = async (...args) => {
      const planned = await original(...args);
      return { ...planned, ...plan(planned) };
    };
  }
  const settings = createSettingsFixture();
  const drops = fakeDrops(drop ?? (() => null));
  render(
    <DropProvider source={drops.source} hitTest={drops.hitTest}>
      <QueryClientProvider client={new QueryClient()}>
        <SettingsProvider api={settings.api}>
          <BackupProvider api={api}>
            <BackupSettings />
          </BackupProvider>
        </SettingsProvider>
      </QueryClientProvider>
    </DropProvider>,
  );
  return { api, settings, drops };
}

it("backs up now, says it was checked and shows the file", async () => {
  const { api } = renderPanel();
  const button = await screen.findByRole("button", { name: "Back up now" });
  // What it holds and leaves out, before anything is written.
  expect(await screen.findByText(/Never included: passwords and sign-ins/)).toBeTruthy();
  fireEvent.click(button);
  await screen.findByText(/Backed up .*, checked\./);
  expect(api.calls).toContain("start");
  fireEvent.click(screen.getByRole("button", { name: "Show in folder" }));
  await waitFor(() =>
    expect(api.calls.some((call) => call.startsWith("reveal ") && call.endsWith(".zip"))).toBe(
      true,
    ),
  );
});

it("components and the schedule are saved to core.backups", async () => {
  const { settings } = renderPanel();
  const library = await screen.findByRole("checkbox", { name: /Library and settings/ });
  expect((library as HTMLInputElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole("checkbox", { name: /Downloaded artwork/ }));
  await waitFor(() =>
    expect(settings.values["core.backups"]?.components).toContain("downloadedArtwork"),
  );
  fireEvent.click(screen.getByRole("radio", { name: "Daily" }));
  await waitFor(() => expect(settings.values["core.backups"]?.schedule).toBe("daily"));
});

it("the backup folder keeps fast typing and saves once, on leaving the field (#51)", async () => {
  const { settings } = renderPanel();
  const field = (await screen.findByRole("textbox", { name: "Backup folder" })) as HTMLInputElement;
  const before = settings.writes.length;
  fireEvent.focus(field);
  let typed = "";
  for (const letter of "/media/usb") {
    typed += letter;
    fireEvent.change(field, { target: { value: typed } });
  }
  expect(field.value).toBe("/media/usb");
  // Nothing saved, and so no "isn't there" for a half-typed path.
  await new Promise((resolve) => setTimeout(resolve, 700));
  expect(settings.writes.length).toBe(before);
  expect(screen.queryByText(/This folder isn't there/)).toBeNull();
  fireEvent.blur(field);
  await waitFor(() => expect(settings.values["core.backups"]?.destination).toBe("/media/usb"));
  expect(settings.writes.length).toBe(before + 1);
  expect(field.value).toBe("/media/usb");
});

it("Back up now waits, saying why, while the chosen folder isn't there (#67)", async () => {
  const { settings, api } = renderPanel();
  const button = await screen.findByRole("button", { name: "Back up now" });
  await screen.findByText(/Never included/);
  expect(button).toHaveProperty("disabled", false);
  const field = screen.getByRole("textbox", { name: "Backup folder" });
  fireEvent.focus(field);
  fireEvent.change(field, { target: { value: "/media/unplugged" } });
  fireEvent.blur(field);
  await waitFor(() =>
    expect(settings.values["core.backups"]?.destination).toBe("/media/unplugged"),
  );
  const reason = await screen.findByText("Can't back up: the backup folder isn't there.");
  expect(button).toHaveProperty("disabled", true);
  expect(button.getAttribute("aria-describedby")).toBe(reason.id);
  fireEvent.click(button);
  expect(api.calls).not.toContain("start");
});

it("Back up now waits while the folder is short of space, until Check again finds room (#67)", async () => {
  let availableBytes = 1024;
  renderPanel(undefined, undefined, () => ({ availableBytes }));
  const reason = await screen.findByText(
    "Can't back up: the backup folder doesn't have enough free space.",
  );
  const button = screen.getByRole("button", { name: "Back up now" });
  expect(button).toHaveProperty("disabled", true);
  expect(button.getAttribute("aria-describedby")).toBe(reason.id);
  availableBytes = 1024 ** 4;
  fireEvent.click(screen.getByRole("button", { name: "Check again" }));
  await waitFor(() => expect(button).toHaveProperty("disabled", false));
  expect(screen.queryByText(/Can't back up/)).toBeNull();
});

it("the default folder needn't exist yet: the backup makes it (#67)", async () => {
  renderPanel(undefined, undefined, () => ({ destinationExists: false, availableBytes: null }));
  await screen.findByText(/Never included/);
  expect(screen.getByRole("button", { name: "Back up now" })).toHaveProperty("disabled", false);
  expect(screen.queryByText(/Can't back up/)).toBeNull();
});

it("About backups says where save versions are (#67)", async () => {
  renderPanel();
  expect(await screen.findByText(/versions of each game's saves/)).toBeTruthy();
  expect(screen.getByText(/Settings → Devices → Saves/)).toBeTruthy();
});

it("a failed automatic backup says why", async () => {
  renderPanel({
    last: {
      atMs: Date.UTC(2026, 9, 29),
      automatic: true,
      path: null,
      bytes: 0,
      error: {
        code: "storageOffline",
        summary: "The backup folder “/media/usb” isn't there.",
        detail: null,
        hints: ["reconnectStorage"],
        transient: false,
        reference: "ab12",
      },
    },
  });
  expect(await screen.findByText("The automatic backup failed")).toBeTruthy();
  expect(screen.getByText("The backup folder “/media/usb” isn't there.")).toBeTruthy();
});

it("a dropped backup is checked, previewed and restored with a restart", async () => {
  const { api, drops } = renderPanel(undefined, () =>
    screen.getByRole("button", { name: "Choose a backup…" }),
  );
  await screen.findByRole("button", { name: "Choose a backup…" });
  drops.drop("/media/usb/caskade-backup-windows.zip");
  const preview = await screen.findByRole("region", { name: "Backup to restore" });
  expect(within(preview).getByText(/with 812 games/)).toBeTruthy();
  // Another machine's paths are named before anything changes.
  expect(within(preview).getByText(/Storage locations not found here: D:\\Games/)).toBeTruthy();
  expect(within(preview).getByText(/14 games' folders/)).toBeTruthy();
  fireEvent.click(within(preview).getByRole("checkbox", { name: /Plugins/ }));
  fireEvent.click(within(preview).getByRole("button", { name: "Restore and restart" }));
  await waitFor(() => expect(api.calls).toContain("restart"));
  expect(api.calls).toContain("restore /media/usb/caskade-backup-windows.zip artwork");
});

it("a staged restore can be applied or dropped, and the last one's outcome shows", async () => {
  const journal = {
    state: "staged" as const,
    archive: "caskade-backup-20261028-213000.zip",
    createdAtMs: 0,
    appVersion: "0.1.0",
    components: ["library" as const],
    inspection: (await import("../../../api/backupFixtures")).fixtureInspection("a.zip"),
  };
  const { api } = renderPanel({
    stagedRestore: journal,
    lastRestore: { journal, error: "the library didn't open", finishedAtMs: 0 },
  });
  expect(await screen.findByText(/was undone, and the library from before is back/)).toBeTruthy();
  expect(screen.getByText(/is ready to restore/)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Don't restore" }));
  await waitFor(() => expect(api.calls).toContain("discard"));
  await screen.findByRole("button", { name: "Choose a backup…" });
});
