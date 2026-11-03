/** #243: Settings → Backup keeps backups on a paired device too, and
 * restores one a paired device keeps, fetched here first, through the
 * usual preview and staged restore. */
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { BackupProvider } from "../../../api/backup";
import { createBackupFixtureApi } from "../../../api/backupFixtures";
import { SyncProvider } from "../../../api/sync";
import { createSyncFixtureApi } from "../../../api/syncFixtures";
import { DropProvider } from "../../../drop/DropProvider";
import { fakeDrops } from "../../../drop/testing";
import { createSettingsFixture } from "../../../test/settingsFixture";
import { SettingsProvider } from "../useSettings";
import { BackupSettings } from "./BackupSettings";

afterEach(cleanup);

const deck = "b".repeat(64);

function renderPanel({ devices = true } = {}) {
  const api = createBackupFixtureApi();
  const settings = createSettingsFixture();
  const drops = fakeDrops(() => null);
  const panel = (
    <BackupProvider api={api}>
      <BackupSettings />
    </BackupProvider>
  );
  render(
    <DropProvider source={drops.source} hitTest={drops.hitTest}>
      <QueryClientProvider client={new QueryClient()}>
        <SettingsProvider api={settings.api}>
          {devices ? <SyncProvider api={createSyncFixtureApi()}>{panel}</SyncProvider> : panel}
        </SettingsProvider>
      </QueryClientProvider>
    </DropProvider>,
  );
  return { api, settings };
}

it("keeps backups on a paired device as well, or only here", async () => {
  const { settings } = renderPanel();
  const row = (await screen.findByText("Also keep backups on")).closest(
    '[data-cs="setting-row"]',
  ) as HTMLElement;
  fireEvent.click(within(row).getByRole("button"));
  fireEvent.click(await screen.findByRole("option", { name: "Steam Deck" }));
  await waitFor(() =>
    expect((settings.values["core.backups"] as { keepOnDevice?: string }).keepOnDevice).toBe(deck),
  );
  fireEvent.click(within(row).getByRole("button"));
  fireEvent.click(await screen.findByRole("option", { name: "Only this device" }));
  await waitFor(() =>
    expect((settings.values["core.backups"] as { keepOnDevice?: string }).keepOnDevice).toBe(null),
  );
});

it("restores a backup another device keeps: fetched, previewed, then staged", async () => {
  const { api } = renderPanel();
  api.setOnDevices([
    {
      deviceId: deck,
      deviceName: "Steam Deck",
      backups: [
        {
          origin: "c".repeat(64),
          originName: "Old desktop",
          hash: `sha256:${"e".repeat(64)}`,
          name: "caskade-backup-20261102T2000Z.zip",
          size: 52_428_800,
          createdMs: Date.UTC(2026, 10, 2, 20),
        },
      ],
    },
  ]);
  const kept = await screen.findByRole("region", { name: "Kept on Steam Deck" });
  expect(kept.textContent).toContain("Old desktop");
  fireEvent.click(within(kept).getByRole("button", { name: /^Restore the backup of Old desktop/ }));
  const preview = await screen.findByRole("region", { name: "Backup to restore" });
  expect(api.calls).toContain(`fetch sha256:${"e".repeat(64)}`);
  fireEvent.click(within(preview).getByRole("button", { name: "Restore and restart" }));
  await waitFor(() => expect(api.calls).toContain("restart"));
  expect(
    api.calls.some((call) => call.startsWith("restore /fetched/caskade-backup-20261102T2000Z.zip")),
  ).toBe(true);
});

it("without paired devices, Backup is as before", async () => {
  renderPanel({ devices: false });
  await screen.findByRole("button", { name: "Choose a backup…" });
  expect(screen.queryByText("Also keep backups on")).toBeNull();
  expect(screen.queryByText("On your other devices")).toBeNull();
});
