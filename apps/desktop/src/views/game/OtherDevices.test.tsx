/** #232, #233: a game's copies on the other devices, on its page and in
 * its menu: the device, how it's reached, the size and why a copy couldn't
 * come here; Copy to this computer where one can. */
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.setConfig({ testTimeout: 30_000 });

import { type PeerCopy, SyncProvider } from "../../api/sync";
import { createSyncFixtureApi } from "../../api/syncFixtures";
import { fakeLayout, fakeViewport, renderApp } from "../../test/harness";

let undoLayout = () => {};
beforeEach(() => {
  undoLayout = fakeLayout();
  fakeViewport();
});
afterEach(() => {
  undoLayout();
  cleanup();
});

const copies: PeerCopy[] = [
  {
    deviceId: "b".repeat(64),
    deviceName: "Steam Deck",
    kind: "handheld",
    os: "linux",
    link: "lan",
    sizeBytes: 1_200_000_000,
    location: "managed",
    blocked: null,
    throughProton: true,
    playing: false,
  },
  {
    deviceId: "c".repeat(64),
    deviceName: "Office laptop",
    kind: "laptop",
    os: "windows",
    link: "relay",
    sizeBytes: null,
    location: "launcher",
    blocked: "launcherOnly",
    throughProton: false,
    playing: false,
  },
  {
    deviceId: "d".repeat(64),
    deviceName: "Living room",
    kind: "tv",
    os: "linux",
    link: "offline",
    sizeBytes: 900_000_000,
    location: "external",
    blocked: null,
    throughProton: false,
    playing: false,
  },
];

function app(path: string) {
  const sync = createSyncFixtureApi();
  sync.setCopies("fixture-1", copies);
  renderApp(path, {
    wrap: (children: ReactNode) => <SyncProvider api={sync}>{children}</SyncProvider>,
  });
  return sync;
}

it("shows the game's copies on the other devices on its page", async () => {
  app("/games/fixture-1");
  const panel = (await screen.findByRole("heading", { name: "On your other devices" })).closest(
    '[data-part="other-devices"]',
  ) as HTMLElement;
  const rows = [...panel.querySelectorAll<HTMLElement>('[data-part="peer-copy"]')];
  expect(rows.map((row) => row.getAttribute("data-status"))).toEqual(["lan", "relay", "offline"]);
  expect(within(rows[0] as HTMLElement).getByText("On Steam Deck")).toBeTruthy();
  expect(
    within(rows[0] as HTMLElement).getByText(/GB · A Windows game: it runs here through Proton/),
  ).toBeTruthy();
  expect(within(rows[1] as HTMLElement).getByText(/Install it from the store here/)).toBeTruthy();
  expect(within(rows[2] as HTMLElement).getByText(/Living room is out of reach/)).toBeTruthy();
  // Only a copy that can come here offers to: not a launcher's, not one
  // out of reach.
  expect(
    within(panel)
      .getAllByRole("button")
      .map((b) => b.textContent),
  ).toEqual(["Copy to this computer"]);
  expect(within(rows[0] as HTMLElement).getByRole("button")).toBeTruthy();
  // The copies aren't releases here.
  expect(screen.queryByText("Another of your devices")).toBeNull();
});

it("copies a game here from another device", async () => {
  const sync = app("/games/fixture-1");
  const button = await screen.findByRole("button", { name: "Copy to this computer" });
  fireEvent.click(button);
  expect(await screen.findByText("Copying to this computer. Follow it in Activity.")).toBeTruthy();
  await waitFor(async () =>
    expect((await sync.transfers()).map((t) => [t.deviceName, t.direction])).toEqual([
      ["Steam Deck", "incoming"],
    ]),
  );
});

it("lists the devices in the game's menu, each opening its page", async () => {
  app("/games/fixture-1");
  const hero = (
    await screen.findByRole("heading", { level: 1, name: "Fixture Game 0002" })
  ).closest('[data-cs="hero"]') as HTMLElement;
  await screen.findByRole("heading", { name: "On your other devices" });
  fireEvent.contextMenu(hero);
  const menu = await screen.findByRole("menu");
  expect(within(menu).getByText("Your devices")).toBeTruthy();
  const item = within(menu).getByRole("menuitem", { name: /On Steam Deck/ });
  expect(item.textContent).toMatch(/GB · A Windows game/);
  expect(within(menu).getByRole("menuitem", { name: /On Living room/ }).textContent).toMatch(
    /out of reach/,
  );
  fireEvent.click(item);
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
  expect(screen.getByRole("heading", { name: "On your other devices" })).toBeTruthy();
});
