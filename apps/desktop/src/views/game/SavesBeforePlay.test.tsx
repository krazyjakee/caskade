import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { createFixtureApi } from "../../api/fixtures";
import { createLibraryQueryClient, LibraryProvider } from "../../api/provider";
import type { SaveVersion } from "../../api/saves";
import { SyncProvider } from "../../api/sync";
import { createSyncFixtureApi } from "../../api/syncFixtures";
import { usePlayWithSaves } from "./SavesBeforePlay";

afterEach(cleanup);

const deck = "b".repeat(64);

function version(id: string, local: boolean, createdAtMs: number): SaveVersion {
  return {
    id,
    gameId: "fixture-1",
    accountId: "player",
    deviceId: local ? "a".repeat(64) : deck,
    sessionId: null,
    createdAtMs,
    contentHash: `sha256:${id}`,
    sizeBytes: 2048,
    fileCount: 2,
    parents: ["base"],
    reason: "session",
    local,
  };
}

function Play({ start }: { start: (entryId: string) => void }) {
  const saves = usePlayWithSaves(start);
  return (
    <>
      <button type="button" onClick={() => saves.play("entry-1")}>
        Play
      </button>
      {saves.dialog}
    </>
  );
}

function renderPlay() {
  const library = createFixtureApi();
  const start = vi.fn();
  render(
    <LibraryProvider api={library} client={createLibraryQueryClient()}>
      <SyncProvider api={createSyncFixtureApi()}>
        <Play start={start} />
      </SyncProvider>
    </LibraryProvider>,
  );
  return { saves: library.saves, start };
}

it("starts at once when there's nothing to say", async () => {
  const { start } = renderPlay();
  fireEvent.click(screen.getByRole("button", { name: "Play" }));
  await waitFor(() => expect(start).toHaveBeenCalledWith("entry-1"));
  expect(screen.queryByRole("status")).toBeNull();
});

it("says whose save the game started with, and when a device couldn't be asked", async () => {
  const { saves, start } = renderPlay();
  saves.seedCheck("entry-1", {
    check: { type: "restored", version: version("v2", false, Date.UTC(2026, 10, 2, 18)) },
    checked: false,
    warnings: [],
  });
  fireEvent.click(screen.getByRole("button", { name: "Play" }));
  await waitFor(() => expect(start).toHaveBeenCalledWith("entry-1"));
  const note = await screen.findByRole("status");
  await waitFor(() => expect(note.textContent).toContain("Started with the save from Steam Deck."));
  expect(note.textContent).toContain("Not every paired device could be reached");
});

it("asks which save to use when both devices played, then plays with the chosen one", async () => {
  const { saves, start } = renderPlay();
  const here = version("here", true, Date.UTC(2026, 10, 2, 17));
  const there = version("there", false, Date.UTC(2026, 10, 2, 18));
  saves.seedCheck("entry-1", {
    check: { type: "conflict", versions: [there, here] },
    checked: true,
    warnings: [],
  });
  fireEvent.click(screen.getByRole("button", { name: "Play" }));
  const dialog = await screen.findByRole("dialog", { name: "Choose which save to play with" });
  expect(start).not.toHaveBeenCalled();
  // This device's save is chosen to begin with; the other is named by its device.
  const radios = screen.getAllByRole("radio") as HTMLInputElement[];
  expect(radios.map((radio) => radio.checked)).toEqual([false, true]);
  await waitFor(() => expect(dialog.textContent).toContain("Steam Deck"));
  expect(dialog.textContent).toContain("This device");
  fireEvent.click(radios[0] as HTMLInputElement);
  fireEvent.click(screen.getByRole("button", { name: "Play with this save" }));
  await waitFor(() => expect(start).toHaveBeenCalledWith("entry-1"));
  expect(saves.chosen).toEqual([{ entryId: "entry-1", versionId: "there" }]);
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("cancelling the question starts nothing and chooses nothing", async () => {
  const { saves, start } = renderPlay();
  saves.seedCheck("entry-1", {
    check: {
      type: "conflict",
      versions: [version("there", false, 2), version("here", true, 1)],
    },
    checked: true,
    warnings: [],
  });
  fireEvent.click(screen.getByRole("button", { name: "Play" }));
  await screen.findByRole("dialog");
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(start).not.toHaveBeenCalled();
  expect(saves.chosen).toEqual([]);
});

it("a failed check never stops the launch", async () => {
  const { saves, start } = renderPlay();
  saves.beforePlay = vi.fn(async () => {
    throw new Error("the library isn't open");
  });
  fireEvent.click(screen.getByRole("button", { name: "Play" }));
  await waitFor(() => expect(start).toHaveBeenCalledWith("entry-1"));
});
