/** The Settings tabs the settings tests leave out, in the pseudo-locale (#9):
 * Scripts, Store accounts, Streaming and Backup. */
import { waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { expect, it } from "vitest";
import { BackupProvider } from "../api/backup";
import { createBackupFixtureApi } from "../api/backupFixtures";
import { StoreAccountsProvider } from "../api/storeAccounts";
import {
  createStoreAccountsFixtureApi,
  fixtureAccounts,
  fixtureProviders,
} from "../api/storeAccountsFixtures";
import { StreamingApiProvider } from "../api/streaming";
import { createStreamingFixtureApi, fixtureStreamHosts } from "../api/streamingFixtures";
import { devices, one, open, paths, ready, shown, strings } from "./pseudoHarness";

// Settings tabs the settings tests leave out.

const settings = () => ['[data-view="settings"]', '[role="tabpanel"]'];

it("shows Settings → Scripts in the pseudo-locale", async () => {
  await open("/settings?tab=scripts");
  await ready('[role="tabpanel"] [data-cs="scripts"] [data-part="order"]');
  // Placeholders and variables are written as code.
  expect(shown(settings(), [/\{\w+\}/g, /CASKADE_\w+/g], "code")).toEqual([]);
});

it("shows Settings → Store accounts in the pseudo-locale", async () => {
  await open("/settings?tab=stores", {
    wrap: (children: ReactNode) => (
      <StoreAccountsProvider api={createStoreAccountsFixtureApi()}>
        {children}
      </StoreAccountsProvider>
    ),
  });
  await waitFor(() => expect(one('[role="tabpanel"]')?.textContent).toContain("Old Origin"));
  await waitFor(() => expect(one('[role="tabpanel"]')?.textContent).toContain("Ada Epic"));
  // Stores, accounts and the core's words for what went wrong (#214).
  const data = [
    ...strings([
      ...fixtureProviders.map((provider) => provider.name),
      ...fixtureAccounts().flatMap((summary) => [
        summary.account.displayName,
        summary.account.sync.error?.summary,
      ]),
    ]),
    /\borigin\b/g,
    /Steam|Epic Games|GOG|Lutris|itch\.io/g,
  ];
  expect(shown(settings(), data)).toEqual([]);
});

it("shows Settings → Streaming while hosting in the pseudo-locale", async () => {
  const streaming = createStreamingFixtureApi({
    hosting: { state: "ready", version: "2026.914.233613", problem: null, warning: null },
    requests: [{ id: "r1", name: "Steam Deck", address: "192.168.1.40" }],
    paired: [
      { uuid: "u1", name: "Deck" },
      { uuid: "u2", name: "Bedroom TV" },
    ],
  });
  await open("/settings?tab=streaming", {
    wrap: (children: ReactNode) => (
      <StreamingApiProvider api={streaming}>{children}</StreamingApiProvider>
    ),
  });
  await ready('[role="tabpanel"] [data-cs="streaming-settings"]');
  await waitFor(() => {
    const text = one('[role="tabpanel"]')?.textContent ?? "";
    for (const name of ["Living room", "Study", "Bedroom TV", "Steam Deck"])
      expect(text).toContain(name);
  });
  const data = [
    ...strings(fixtureStreamHosts().flatMap((host) => [host.name, host.address])),
    devices,
    /\d+\.\d+\.\d+\.\d+(?::\d+)?/g,
    /Sunshine|Moonlight/g,
  ];
  expect(shown(settings(), data)).toEqual([]);
});

it("shows Settings → Backup in the pseudo-locale", async () => {
  const destination = "/home/player/.local/share/caskade/backups";
  const backup = createBackupFixtureApi({
    last: {
      atMs: Date.UTC(2026, 9, 30, 9, 30),
      automatic: true,
      path: `${destination}/caskade-backup-20261030-093000.zip`,
      bytes: 71 * 1024 * 1024,
      error: null,
    },
  });
  await open("/settings?tab=backup", {
    wrap: (children: ReactNode) => <BackupProvider api={backup}>{children}</BackupProvider>,
  });
  await ready(`[role="tabpanel"] [placeholder="${destination}"]`);
  // The last backup's outcome, with its size.
  await waitFor(() => expect(one('[role="tabpanel"]')?.textContent).toContain("71.0 MB"));
  expect(shown(settings(), [...paths, devices])).toEqual([]);
});
