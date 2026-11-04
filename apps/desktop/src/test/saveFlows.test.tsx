/** #241: a game's saves with a pad in console mode (restore an older
 * version, choose between two devices' saves) and Settings → Devices →
 * Saves; Show in folder in desktop mode. */
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.setConfig({ testTimeout: 30_000 });

import type { SaveState, SaveVersion, VersionState } from "../api/saves";
import { fakeLayout, fakeViewport, focused, renderApp } from "./harness";

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

async function walkTo(app: App, found: (element: HTMLElement) => boolean, limit = 120) {
  for (let i = 0; i < limit; i++) {
    const at = focused();
    if (at && at !== document.body && found(at)) return at;
    app.pad.press("nav.down");
    await new Promise((resolve) => setTimeout(resolve, 0));
    if (focused() === at && i > 0) return null;
  }
  return null;
}

const named = (name: string) => (element: HTMLElement) => element.textContent?.trim() === name;

function version(id: string, at: number, extra: Partial<SaveVersion> = {}): SaveVersion {
  return {
    id,
    gameId: "fixture-1",
    accountId: "",
    deviceId: "here",
    sessionId: null,
    createdAtMs: Date.UTC(2026, 10, 2, at),
    contentHash: `sha256:${id}`,
    sizeBytes: 4096,
    fileCount: 1,
    parents: [],
    reason: "session",
    local: true,
    ...extra,
  };
}

const state = (each: SaveVersion, current = false): VersionState => ({
  version: each,
  current,
  complete: true,
});

const locations: SaveState = {
  entryId: "entry-1",
  revision: 0,
  preferences: { manifestTitle: null, locations: null },
  matched: { type: "unknown" },
  automatic: [],
  locations: [
    { spec: { kind: "folder", path: "/home/player/Saves/Fixture" }, tags: [], fileCount: 1 },
  ],
  warnings: [],
};

const openSaves = () => screen.findByRole("heading", { name: "Saves" });

it("restores an older save with a pad, and the save it replaced stays a version", async () => {
  const app = renderApp("/games/fixture-1", { mode: "console" });
  app.library.saves.seedHistory("fixture-1", {
    accountId: "",
    syncing: true,
    conflict: [],
    versions: [state(version("newer", 18), true), state(version("older", 12))],
  });
  await openSaves();
  const restore = await walkTo(app, named("Restore"));
  expect(restore).not.toBeNull();
  app.pad.press("confirm");
  await screen.findByText("Restored. The save that was in use is kept as a version.");
  expect(app.library.saves.restored).toEqual([{ entryId: "entry-1", versionId: "older" }]);
  const list = document.querySelector<HTMLElement>('[data-part="versions"]') as HTMLElement;
  await waitFor(() => expect(within(list).getAllByRole("listitem")).toHaveLength(3));
  // The restored copy is in use; the newer save it replaced is kept.
  const rows = within(list).getAllByRole("listitem");
  expect(rows[0]?.dataset.status).toBe("current");
  expect(rows.map((row) => row.dataset.status)).toEqual(["current", "kept", "kept"]);
  // No Show in folder in console mode.
  expect(screen.queryByRole("button", { name: /Show in folder/ })).toBeNull();
});

it("chooses between two devices' saves with a pad", async () => {
  const app = renderApp("/games/fixture-1", { mode: "console" });
  const here = version("here", 17);
  const there = version("there", 18, { local: false, deviceId: "deck" });
  app.library.saves.seedHistory("fixture-1", {
    accountId: "",
    syncing: true,
    conflict: ["here", "there"],
    versions: [state(there), state(here, true)],
  });
  app.library.saves.seedCheck("entry-1", {
    check: { type: "conflict", versions: [there, here] },
    checked: true,
    warnings: [],
  });
  await openSaves();
  await walkTo(app, named("Choose a save…"));
  app.pad.press("confirm");
  const dialog = await screen.findByRole("dialog", { name: "Choose which save to play with" });
  // This device's save is chosen first; a pad starts on the newest, the
  // other device's, and A picks it.
  const radios = within(dialog).getAllByRole("radio") as HTMLInputElement[];
  expect(radios.map((radio) => radio.checked)).toEqual([false, true]);
  await waitFor(() => expect(focused()).toBe(radios[0]));
  expect(radios[0]?.closest("label")?.textContent).toContain("Another device");
  app.pad.press("confirm");
  await waitFor(() => expect(radios[0]?.checked).toBe(true));
  await walkTo(app, named("Use this save"));
  app.pad.press("confirm");
  await waitFor(() =>
    expect(app.library.saves.chosen).toEqual([{ entryId: "entry-1", versionId: "there" }]),
  );
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

it("shows where the saves are, with Show in folder in desktop mode", async () => {
  const app = renderApp("/games/fixture-1");
  app.library.saves.seed(locations);
  await openSaves();
  expect(screen.getByText("/home/player/Saves/Fixture")).toBeTruthy();
  expect(
    screen.getByText("No saves kept yet. A version is kept each time you finish playing."),
  ).toBeTruthy();
  fireEvent.click(
    screen.getByRole("button", { name: "Show in folder: /home/player/Saves/Fixture" }),
  );
  await waitFor(() =>
    expect(app.library.saves.revealed).toEqual([{ entryId: "entry-1", location: 0 }]),
  );
});

it("Settings → Devices → Saves with a pad: sync, versions to keep, Update now", async () => {
  const app = renderApp("/settings?tab=devices", { mode: "console" });
  app.library.saves.seedManifest({ available: true, checkedAtMs: Date.UTC(2026, 10, 1, 9) });
  await screen.findByRole("heading", { name: "Saves" });
  // Whether saves sync is What syncs' Saves row (#54).
  const toggle = await walkTo(app, (element) => {
    const row = element.closest('[data-cs="setting-row"]');
    return (
      element.getAttribute("role") === "radio" &&
      element.textContent === "This device only" &&
      row?.querySelector('[data-part="label"]')?.textContent === "Saves"
    );
  });
  expect(toggle).not.toBeNull();
  app.pad.press("confirm");
  await waitFor(() =>
    expect(
      (app.settings.values["core.sync"] as { categories: { saves: string } }).categories.saves,
    ).toBe("thisDeviceOnly"),
  );
  const keep = await walkTo(app, (element) =>
    Boolean(element.closest('[data-cs="setting-row"]')?.textContent?.includes("Versions to keep")),
  );
  expect(keep?.textContent).toContain("20 versions");
  const row = () =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-cs="setting-row"]')).find((each) =>
      each.textContent?.includes("Save locations list"),
    );
  const before = row()?.textContent;
  const update = await walkTo(app, named("Update now"));
  expect(update).not.toBeNull();
  app.pad.press("confirm");
  await waitFor(() => expect(row()?.textContent).not.toBe(before));
  expect(row()?.textContent).toContain("Last checked");
});
