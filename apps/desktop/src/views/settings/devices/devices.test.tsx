/** Settings → Devices (#231) over the fixture API: pairing by code and by
 * link, forgetting, what syncs, the read-only view of a standard account,
 * pairings and opened links over any view, the status bar's entry, and the
 * same with a pad only in console mode. */
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.setConfig({ testTimeout: 30_000 });

import { SyncProvider } from "../../../api/sync";
import { createSyncFixtureApi, type SyncFixtureApi } from "../../../api/syncFixtures";
import { fakeLayout, fakeViewport, focused, renderApp, until } from "../../../test/harness";

let undoLayout = () => {};
beforeEach(() => {
  undoLayout = fakeLayout();
  fakeViewport();
});
afterEach(() => {
  undoLayout();
  cleanup();
});

const family = [
  { id: "ada", name: "Ada", role: "admin" as const, pin: "2468" },
  { id: "kid", name: "Kid", avatar: "avatar:moon" },
];

function devices(
  path: string,
  sync: SyncFixtureApi,
  options: Parameters<typeof renderApp>[1] = {},
) {
  return renderApp(path, {
    ...options,
    wrap: (children: ReactNode) => <SyncProvider api={sync}>{children}</SyncProvider>,
  });
}

it("shows this device, paired devices with how each is reached, and syncs one now", async () => {
  const sync = createSyncFixtureApi();
  devices("/settings?tab=devices", sync);
  expect(await screen.findByDisplayValue("Den-PC")).toBeTruthy();
  expect(screen.getByText(/192\.168\.1\.20/)).toBeTruthy();
  const rows = document.querySelectorAll('[data-cs="paired-device"]');
  expect([...rows].map((row) => row.getAttribute("data-status"))).toEqual([
    "lan",
    "relay",
    "direct",
    "offline",
  ]);
  // The last try didn't reach the TV: said quietly, never as an error.
  expect(within(rows[3] as HTMLElement).getByText(/Last try didn't reach it/)).toBeTruthy();
  const deck = rows[0] as HTMLElement;
  expect(within(deck).getByText("Local network")).toBeTruthy();
  fireEvent.click(within(deck).getByRole("button", { name: "Sync now" }));
  await waitFor(() => expect(within(deck).getByText(/Synced/)).toBeTruthy());
  // Waking (#234) and copies (#233) aren't offered yet.
  expect(screen.queryByRole("button", { name: /Wake/ })).toBeNull();
  expect(screen.queryByText("Copies and shares")).toBeNull();
});

it("pairs a nearby device once both codes match", async () => {
  const sync = createSyncFixtureApi();
  devices("/settings?tab=devices", sync);
  fireEvent.click(await screen.findByRole("button", { name: "Pair…" }));
  const dialog = await screen.findByRole("dialog");
  expect(dialog.querySelector('[data-part="code"]')?.textContent).toBe("482 913");
  fireEvent.click(within(dialog).getByRole("button", { name: "They match: pair" }));
  await screen.findByRole("heading", { name: "Paired with Kids' PC" });
  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(sync.state().devices.map((device) => device.name)).toContain("Kids' PC");
  expect(sync.state().pairing).toBeNull();
});

it("pairs with a pasted link, makes one to copy, and forgets a device", async () => {
  const sync = createSyncFixtureApi();
  devices("/settings?tab=devices", sync);
  fireEvent.click(await screen.findByRole("button", { name: "Copy a pairing link" }));
  await waitFor(() =>
    expect(document.querySelector('[data-part="pairing-link"]')?.textContent).toMatch(
      /^caskade:\/\/pair\?t=/,
    ),
  );
  const field = screen.getByLabelText("Paste a pairing link from another device");
  fireEvent.change(field, { target: { value: "  hello  " } });
  fireEvent.click(screen.getByRole("button", { name: "Pair" }));
  expect(await screen.findByText("that isn't a Caskade pairing link")).toBeTruthy();
  fireEvent.change(field, { target: { value: "caskade://pair?t=caskadepairabc" } });
  fireEvent.click(screen.getByRole("button", { name: "Pair" }));
  expect(await screen.findByText("Paired with Office desktop.")).toBeTruthy();

  fireEvent.click(screen.getByRole("button", { name: "Forget Office laptop" }));
  fireEvent.click(await screen.findByRole("button", { name: "Forget Office laptop" }));
  await waitFor(() =>
    expect(sync.state().devices.map((device) => device.name)).not.toContain("Office laptop"),
  );
});

it("changes what syncs and Never use relays", async () => {
  const sync = createSyncFixtureApi();
  const app = devices("/settings?tab=devices", sync);
  const row = (await screen.findByText("Play history")).closest('[data-cs="setting-row"]');
  fireEvent.click(within(row as HTMLElement).getByRole("radio", { name: "This device only" }));
  await waitFor(async () => {
    const value = app.settings.values["core.sync"] as {
      categories: Record<string, string>;
    };
    expect(value.categories.playSessions).toBe("thisDeviceOnly");
  });
  fireEvent.click(screen.getByRole("switch", { name: "Never use relays" }));
  await waitFor(async () => {
    const value = app.settings.values["core.sync"] as { neverRelay: boolean };
    expect(value.neverRelay).toBe(true);
  });
});

it("shows a standard account the devices without the changes", async () => {
  const sync = createSyncFixtureApi();
  devices("/settings?tab=devices", sync, { accounts: family, activeAccount: "kid" });
  expect(
    await screen.findByText("Only an admin can pair devices, forget them or change what syncs."),
  ).toBeTruthy();
  await waitFor(() =>
    expect(document.querySelectorAll('[data-cs="paired-device"]')).toHaveLength(4),
  );
  expect(screen.queryByText("Pair a device")).toBeNull();
  expect(screen.queryByRole("button", { name: /Forget/ })).toBeNull();
  expect(screen.getByRole("switch", { name: "Never use relays" })).toHaveProperty("disabled", true);
  for (const radio of screen.getAllByRole("radio", { name: "This device only" })) {
    expect(radio).toHaveProperty("disabled", true);
  }
  expect(screen.getByLabelText("Device name")).toHaveProperty("readOnly", true);
});

it("shows another device's pairing and an opened link over any view", async () => {
  const sync = createSyncFixtureApi();
  devices("/library", sync);
  await screen.findByRole("heading", { name: "Library" });
  sync.incomingPairing("MacBook", "105277");
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByRole("heading").textContent).toBe("MacBook wants to pair");
  expect(dialog.querySelector('[data-part="code"]')?.textContent).toBe("105 277");
  fireEvent.click(within(dialog).getByRole("button", { name: "They don't match" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  await waitFor(() => expect(sync.state().pairing).toBeNull());

  sync.openLink({ deviceId: "7".repeat(64), expiresAtMs: Date.now() + 60_000, expired: false });
  const link = await screen.findByRole("dialog");
  expect(within(link).getByText(/starts with 77777777/)).toBeTruthy();
  fireEvent.click(within(link).getByRole("button", { name: "Pair" }));
  await waitFor(() =>
    expect(sync.state().devices.map((device) => device.id)).toContain("7".repeat(64)),
  );
});

it("puts devices in reach in the status bar, linked to Settings → Devices", async () => {
  const sync = createSyncFixtureApi();
  devices("/library", sync);
  const entry = await waitFor(() => {
    const found = document.querySelector<HTMLAnchorElement>(
      '[data-cs="status-bar"] [data-part="sync"]',
    );
    expect(found).toBeTruthy();
    return found as HTMLAnchorElement;
  });
  expect(entry.textContent).toMatch(/3 devices in reach/);
  expect(entry.getAttribute("data-status")).toBe("lan");
  expect(entry.getAttribute("href")).toMatch(/tab=devices/);
});

it("pairs with a pad only in console mode, the code big enough for the sofa", async () => {
  const sync = createSyncFixtureApi();
  const app = devices("/settings?tab=devices", sync, { mode: "console" });
  await screen.findByRole("button", { name: "Pair…" });
  for (let i = 0; i < 80 && focused()?.textContent !== "Pair…"; i++) {
    app.pad.press("nav.down");
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  expect(focused()?.textContent).toBe("Pair…");
  app.pad.press("confirm");
  const dialog = await screen.findByRole("dialog");
  expect(dialog.querySelector('[data-part="code"]')?.className).toMatch(/text-7xl/);
  await until(() => focused()?.textContent === "They match: pair", "the confirm button");
  app.pad.press("confirm");
  await screen.findByRole("heading", { name: "Paired with Kids' PC" });

  // A pairing link typed with the on-screen keyboard.
  await until(() => focused()?.textContent === "Done", "Done");
  app.pad.press("confirm");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  const field = screen.getByLabelText("Paste a pairing link from another device");
  for (let i = 0; i < 80 && focused() !== field; i++) {
    app.pad.press("nav.down");
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  expect(focused()).toBe(field);
  app.pad.press("confirm");
  await until(() => Boolean(document.querySelector('[data-cs="osk"]')), "the keyboard");
  app.pad.press("confirm");
  app.pad.press("back");
  await until(() => !document.querySelector('[data-cs="osk"]'), "the keyboard to close");
  expect((field as HTMLInputElement).value.length).toBe(1);
});
