import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { createFixtureGames } from "../../api/fixtures";
import { DropProvider } from "../../drop/DropProvider";
import { fakeDrops } from "../../drop/testing";
import { renderApp } from "../../test/harness";

const dialogOpen = vi.hoisted(() => vi.fn<() => Promise<string | null>>());
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: dialogOpen }));

afterEach(() => {
  cleanup();
  dialogOpen.mockReset();
});

async function openSaves() {
  fireEvent.click(await screen.findByRole("button", { name: "Edit details" }));
  const dialog = await screen.findByRole("dialog", { name: "Edit Fixture Game 0001" });
  fireEvent.click(within(dialog).getByRole("tab", { name: "Saves" }));
  const tab = dialog.querySelector<HTMLElement>('[data-cs="game-saves"]');
  if (!tab) throw new Error("no Saves tab");
  await within(tab).findByText(/Using automatic locations/);
  return { dialog, tab };
}

const paths = (tab: HTMLElement) =>
  [...tab.querySelectorAll('[data-part="location"] [data-part="path"]')].map(
    (path) => path.textContent,
  );

it("says a release the manifest doesn't know has no locations until the user adds them", async () => {
  let target: () => Element | null = () => null;
  const drops = fakeDrops(() => target());
  const app = renderApp("/games/fixture-1", {
    games: createFixtureGames(2),
    wrap: (children) => (
      <DropProvider source={drops.source} hitTest={drops.hitTest}>
        {children}
      </DropProvider>
    ),
  });
  const edit = vi.spyOn(app.library.saves, "edit");
  // The second fixture game is installed; its entry is entry-1.
  fireEvent.click(await screen.findByRole("button", { name: "Edit details" }));
  const dialog = await screen.findByRole("dialog", { name: "Edit Fixture Game 0002" });
  fireEvent.click(within(dialog).getByRole("tab", { name: "Saves" }));
  const tab = dialog.querySelector<HTMLElement>('[data-cs="game-saves"]') as HTMLElement;
  expect(
    (await within(tab).findByText(/No automatic save locations are known/)).dataset.match,
  ).toBe("unknown");
  expect(paths(tab)).toEqual([]);
  expect(within(tab).getByText(/No save files or folders/)).toBeTruthy();
  expect(tab.dataset.choice).toBe("automatic");
  expect(
    within(tab).getByRole("button", { name: "Reset to automatic" }).hasAttribute("disabled"),
  ).toBe(true);

  // A folder from the picker and a file dropped on the tab.
  dialogOpen.mockResolvedValueOnce("/home/me/Saves");
  fireEvent.click(within(tab).getByRole("button", { name: "Add folder" }));
  await waitFor(() => expect(paths(tab)).toEqual(["/home/me/Saves"]));
  expect(dialogOpen).toHaveBeenCalledWith(expect.objectContaining({ directory: true }));
  target = () => tab;
  drops.drop("/home/me/profile.sav");
  await waitFor(() => expect(paths(tab)).toEqual(["/home/me/Saves", "/home/me/profile.sav"]));
  // The same path twice is one location.
  drops.drop("/home/me/Saves");
  expect(paths(tab)).toHaveLength(2);
  expect(
    [...tab.querySelectorAll<HTMLElement>('[data-part="location"]')].map((row) => row.dataset.kind),
  ).toEqual(["folder", "file"]);
  expect(tab.dataset.choice).toBe("chosen");

  fireEvent.click(within(tab).getByRole("button", { name: "Save locations" }));
  await within(tab).findByText("Save locations saved.");
  expect(edit).toHaveBeenCalledWith(
    "entry-1",
    {
      manifestTitle: null,
      locations: [
        { kind: "folder", path: "/home/me/Saves" },
        { kind: "file", path: "/home/me/profile.sav" },
      ],
    },
    0,
  );
  expect((await app.library.saves.state("entry-1")).preferences.locations).toHaveLength(2);
});

it("shows automatic locations, removes one, resets to automatic and refuses a stale edit", async () => {
  const app = renderApp("/games/fixture-0", { games: createFixtureGames(2) });
  app.library.saves.seed({
    entryId: "entry-0",
    revision: 3,
    preferences: { manifestTitle: null, locations: null },
    matched: { type: "matched", title: "Fixture Game", method: "steam" },
    automatic: [
      {
        spec: { kind: "folder", path: "/home/me/.local/share/fixture" },
        tags: ["save"],
        fileCount: 2,
      },
      { spec: { kind: "file", path: "/home/me/fixture.cfg" }, tags: ["config"], fileCount: 1 },
    ],
    locations: [
      {
        spec: { kind: "folder", path: "/home/me/.local/share/fixture" },
        tags: ["save"],
        fileCount: 2,
      },
      { spec: { kind: "file", path: "/home/me/fixture.cfg" }, tags: ["config"], fileCount: 1 },
    ],
    warnings: ["The registry isn't available on this device."],
  });
  const { tab } = await openSaves();
  expect(within(tab).getByText("Save locations from Fixture Game.")).toBeTruthy();
  expect(paths(tab)).toEqual(["/home/me/.local/share/fixture", "/home/me/fixture.cfg"]);
  expect(within(tab).getByText("Folder · 2 save files")).toBeTruthy();
  expect(within(tab).getByText("File · 1 save file")).toBeTruthy();
  expect(within(tab).getByText("The registry isn't available on this device.")).toBeTruthy();

  fireEvent.click(within(tab).getByRole("button", { name: "Remove /home/me/fixture.cfg" }));
  expect(paths(tab)).toEqual(["/home/me/.local/share/fixture"]);
  fireEvent.click(within(tab).getByRole("button", { name: "Save locations" }));
  await within(tab).findByText("Save locations saved.");
  await within(tab).findByText("Using your chosen locations.");
  expect((await app.library.saves.state("entry-0")).revision).toBe(4);

  // Reset brings the automatic locations back.
  fireEvent.click(within(tab).getByRole("button", { name: "Reset to automatic" }));
  await waitFor(() =>
    expect(paths(tab)).toEqual(["/home/me/.local/share/fixture", "/home/me/fixture.cfg"]),
  );
  fireEvent.click(within(tab).getByRole("button", { name: "Save locations" }));
  await within(tab).findByText("Using automatic locations.");
  expect((await app.library.saves.state("entry-0")).preferences).toEqual({
    manifestTitle: null,
    locations: null,
  });

  // Another editor changed them meanwhile: the save is refused, nothing lost.
  fireEvent.change(within(tab).getByRole("textbox", { name: "Manifest game name" }), {
    target: { value: "Fixture Game: Remastered" },
  });
  const current = await app.library.saves.state("entry-0");
  await app.library.saves.edit("entry-0", current.preferences, current.revision);
  fireEvent.click(within(tab).getByRole("button", { name: "Save locations" }));
  expect((await within(tab).findByRole("alert")).textContent).toContain("changed elsewhere");
  expect(
    (within(tab).getByRole("textbox", { name: "Manifest game name" }) as HTMLInputElement).value,
  ).toBe("Fixture Game: Remastered");
});
