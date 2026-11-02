/** #240: a game running on another device shows on its page, its console
 * card and the device's row, and Play warns before starting it; presence
 * never stops a launch; a save on its way is waited for, with Skip. */
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.setConfig({ testTimeout: 30_000 });

import type { PlayingElsewhere, SaveVersion } from "../api/saves";
import { fakeLayout, fakeViewport, playedGames, renderApp } from "./harness";

let undoLayout = () => {};
beforeEach(() => {
  undoLayout = fakeLayout();
  fakeViewport();
});
afterEach(() => {
  undoLayout();
  cleanup();
});

const deck = "b".repeat(64);
const onDeck: PlayingElsewhere = {
  deviceId: deck,
  deviceName: "Steam Deck",
  gameId: "fixture-1",
  title: "Fixture Game 0002",
  accountId: "",
  startedAtMs: Date.UTC(2026, 10, 2, 19),
};

const play = () =>
  fireEvent.click(screen.getAllByRole("button", { name: /^Play\b/ })[0] as HTMLElement);

it("shows where the game is running and warns before playing it here too", async () => {
  const app = renderApp("/games/fixture-1");
  const launch = vi.spyOn(app.library, "launchGame");
  app.library.saves.setPlaying([onDeck]);
  await screen.findByText("Playing on Steam Deck");

  play();
  const dialog = await screen.findByRole("dialog", {
    name: "Fixture Game 0002 is running on Steam Deck",
  });
  expect(launch).not.toHaveBeenCalled();
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(launch).not.toHaveBeenCalled();

  play();
  fireEvent.click(await screen.findByRole("button", { name: "Play anyway" }));
  await waitFor(() => expect(launch).toHaveBeenCalledWith("entry-1", undefined));

  // It ended there: the page says nothing more.
  app.library.saves.setPlaying([]);
  await waitFor(() => expect(screen.queryByText("Playing on Steam Deck")).toBeNull());
});

it("never stops a launch when presence can't be read", async () => {
  const app = renderApp("/games/fixture-1");
  const launch = vi.spyOn(app.library, "launchGame");
  app.library.saves.playingElsewhere = vi.fn(async () => {
    throw new Error("the library isn't open");
  });
  await screen.findByRole("heading", { name: "Fixture Game 0002" });
  play();
  await waitFor(() => expect(launch).toHaveBeenCalledWith("entry-1", undefined));
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("waits for a save on its way from the other device, and Skip plays with the one here", async () => {
  const app = renderApp("/games/fixture-1");
  const launch = vi.spyOn(app.library, "launchGame");
  const version: SaveVersion = {
    id: "from-deck",
    gameId: "fixture-1",
    accountId: "",
    deviceId: deck,
    sessionId: null,
    createdAtMs: Date.UTC(2026, 10, 2, 20),
    contentHash: "sha256:deck",
    sizeBytes: 1024,
    fileCount: 1,
    parents: [],
    reason: "session",
    local: false,
  };
  app.library.saves.seedCheck("entry-1", {
    check: { type: "notArrived", version },
    checked: true,
    warnings: [],
  });
  await screen.findByRole("heading", { name: "Fixture Game 0002" });
  play();
  const waiting = await screen.findByRole("dialog", { name: /^Waiting for the save from/ });
  expect(launch).not.toHaveBeenCalled();
  fireEvent.click(within(waiting).getByRole("button", { name: "Skip" }));
  await waitFor(() => expect(launch).toHaveBeenCalledWith("entry-1", undefined));
});

it("starts once the save on its way has arrived", async () => {
  const app = renderApp("/games/fixture-1");
  const launch = vi.spyOn(app.library, "launchGame");
  const version: SaveVersion = {
    id: "from-deck",
    gameId: "fixture-1",
    accountId: "",
    deviceId: deck,
    sessionId: null,
    createdAtMs: Date.UTC(2026, 10, 2, 20),
    contentHash: "sha256:deck",
    sizeBytes: 1024,
    fileCount: 1,
    parents: [],
    reason: "session",
    local: false,
  };
  app.library.saves.seedCheck("entry-1", {
    check: { type: "notArrived", version },
    checked: true,
    warnings: [],
  });
  await screen.findByRole("heading", { name: "Fixture Game 0002" });
  play();
  await screen.findByRole("dialog", { name: /^Waiting for the save from/ });
  // The files arrive; the next look restores them and plays.
  app.library.saves.seedCheck("entry-1", {
    check: { type: "restored", version },
    checked: true,
    warnings: [],
  });
  await waitFor(() => expect(launch).toHaveBeenCalledWith("entry-1", undefined), {
    timeout: 5_000,
  });
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("marks the card in console mode and the device's row in Settings → Devices", async () => {
  const library = renderApp("/library", { mode: "console", games: playedGames(8) });
  library.library.saves.setPlaying([onDeck]);
  const card = await waitFor(
    () => {
      const found = document.querySelector<HTMLElement>('[data-focus-key="game-card:fixture-1"]');
      if (!found) throw new Error("no card yet");
      return found;
    },
    { timeout: 5_000 },
  );
  await waitFor(() => expect(card.textContent).toContain("Playing on Steam Deck"));
  cleanup();

  const settings = renderApp("/settings?tab=devices");
  settings.library.saves.setPlaying([onDeck]);
  const row = await waitFor(() => {
    const found = Array.from(
      document.querySelectorAll<HTMLElement>('[data-part="device"], [data-cs="paired-device"]'),
    ).find((each) => each.textContent?.includes("Steam Deck"));
    if (!found) throw new Error("no row yet");
    return found;
  });
  await waitFor(() => expect(row.textContent).toContain("Playing Fixture Game 0002"));
});
