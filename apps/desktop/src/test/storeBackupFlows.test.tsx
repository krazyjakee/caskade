/** #194: store accounts, owned games and backups with a pad only. Every
 * step is a pad action; text comes from the on-screen keyboard. */
import { cleanup, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.setConfig({ testTimeout: 30_000 });

import { BackupProvider } from "../api/backup";
import { createBackupFixtureApi } from "../api/backupFixtures";
import { BrowseApiContext } from "../api/browse";
import { createBrowseFixtureApi } from "../api/browseFixtures";
import { createFixtureGames } from "../api/fixtures";
import { StoreAccountsProvider } from "../api/storeAccounts";
import { createStoreAccountsFixtureApi } from "../api/storeAccountsFixtures";
import { fakeLayout, fakeViewport, focused, renderApp, until } from "./harness";

let undoLayout = () => {};
beforeEach(() => {
  undoLayout = fakeLayout();
  fakeViewport();
});
afterEach(() => {
  undoLayout();
  cleanup();
});

type App = ReturnType<typeof renderApp>;

/** Presses down until `found` holds for the focused element. */
async function walkTo(app: App, found: (element: HTMLElement) => boolean, what: string) {
  for (let i = 0; i < 80; i++) {
    const at = focused();
    if (at && at !== document.body && found(at)) return at;
    app.pad.press("nav.down");
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  throw new Error(`never reached ${what}; focus is on ${focused()?.outerHTML.slice(0, 160)}`);
}

it("restores a backup from a drive: the browser, the preview, Restore and restart", async () => {
  const backup = createBackupFixtureApi();
  const tree = createBrowseFixtureApi([
    "/home/player/",
    "/home/player/.local/share/caskade/backups/caskade-backup-20261029-210000.zip",
    "/home/player/.local/share/caskade/backups/notes.txt",
  ]);
  const app = renderApp("/settings?tab=backup", {
    mode: "console",
    wrap: (children: ReactNode) => (
      <BrowseApiContext value={tree}>
        <BackupProvider api={backup}>{children}</BackupProvider>
      </BrowseApiContext>
    ),
  });
  await screen.findByRole("button", { name: "Choose a backup…" });
  await walkTo(app, (element) => element.textContent === "Choose a backup…", "Choose a backup…");
  app.pad.press("confirm");
  await screen.findByRole("dialog");
  // The browser opens in the backup folder, on the backup; A picks it.
  await until(
    () => focused()?.textContent === "caskade-backup-20261029-210000.zip",
    "the backup in the browser",
  );
  app.pad.press("confirm");
  const preview = await screen.findByRole("region", { name: "Backup to restore" });
  expect(backup.calls.some((call) => call.startsWith("inspect "))).toBe(true);
  expect(preview.textContent).toMatch(/games/);
  await walkTo(
    app,
    (element) => element.textContent === "Restore and restart",
    "Restore and restart",
  );
  app.pad.press("confirm");
  await waitFor(() => expect(backup.calls).toContain("restart"));
  expect(backup.calls.find((call) => call.startsWith("restore "))?.split(" ")[1]).toBe(
    "/home/player/.local/share/caskade/backups/caskade-backup-20261029-210000.zip",
  );
});

it("connects a store with an API key typed on the on-screen keyboard", async () => {
  const stores = createStoreAccountsFixtureApi();
  const app = renderApp("/settings?tab=stores", {
    mode: "console",
    wrap: (children: ReactNode) => (
      <StoreAccountsProvider api={stores}>{children}</StoreAccountsProvider>
    ),
  });
  await screen.findByText("itch.io");
  await walkTo(
    app,
    (element) =>
      element.textContent === "Connect another" &&
      Boolean(element.closest("section")?.textContent?.includes("itch.io")),
    "itch.io's Connect another",
  );
  app.pad.press("confirm");
  const signIn = await screen.findByRole("region", { name: "Sign in to itch.io" });
  const key = await walkTo(
    app,
    (element) => element === signIn.querySelector("input"),
    "the API key field",
  );
  // A opens the keyboard; it starts on q. Three of them, then B.
  app.pad.press("confirm");
  await until(() => Boolean(document.querySelector('[data-cs="osk"]')), "the keyboard");
  for (let i = 0; i < 3; i++) app.pad.press("confirm");
  app.pad.press("back");
  await until(() => !document.querySelector('[data-cs="osk"]'), "the keyboard to close");
  expect(key).toHaveProperty("value", "qqq");
  await walkTo(app, (element) => element.textContent === "Connect", "Connect");
  app.pad.press("confirm");
  await waitFor(() =>
    expect(screen.queryByRole("region", { name: "Sign in to itch.io" })).toBeNull(),
  );
  expect(await screen.findByText("qqq")).toBeTruthy();
  expect(stores.calls.some((call) => call.endsWith(" code"))).toBe(true);
});

it("RB reaches Ready to Install; A opens an owned game, and A installs it", async () => {
  const games = createFixtureGames(6).map((game) =>
    game.id === "fixture-0"
      ? {
          ...game,
          status: "not-installed" as const,
          entries: game.entries.map((entry) => ({ ...entry, source: "store:a1" })),
        }
      : game,
  );
  const stores = createStoreAccountsFixtureApi(
    [],
    [
      {
        entryId: "fixture-0-owned",
        storeAccountId: "a1",
        provider: "steam",
        providerName: "Steam",
        accountName: "Ada",
        productId: "620",
        title: "Portal 2",
        state: { type: "available", launcher: "Steam" },
        refreshedAtMs: null,
      },
    ],
  );
  const app = renderApp("/library", {
    mode: "console",
    games,
    wrap: (children: ReactNode) => (
      <StoreAccountsProvider api={stores}>{children}</StoreAccountsProvider>
    ),
  });
  await until(() => focused() !== document.body && focused() !== null, "the library");
  for (let i = 0; i < 10; i++) {
    if (app.router.state.location.search.filter === "owned") break;
    app.pad.press("tab.next");
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  expect(app.router.state.location.search).toMatchObject({ filter: "owned" });
  const card = await until(
    () => /Fixture Game 0001/.test(focused()?.textContent ?? ""),
    "the owned game's card",
  ).then(focused);
  expect(card).not.toBeNull();
  app.pad.press("confirm");
  await screen.findByRole("heading", { level: 1, name: /Fixture Game 0001/ });
  const install = await screen.findByRole("button", { name: "Install with Steam" });
  await walkTo(app, (element) => element === install, "Install with Steam");
  app.pad.press("confirm");
  await waitFor(() => expect(stores.calls).toContain("install fixture-0-owned"));
  await screen.findByText("Waiting for Steam to finish installing it.");
});
