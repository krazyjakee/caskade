import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { createFixtureGames } from "../../api/fixtures";
import { renderApp } from "../../test/harness";

afterEach(cleanup);

const games = () => {
  const list = createFixtureGames(2);
  const [first] = list;
  if (first) {
    first.description = "From the launcher.";
    first.tags = ["Fixture"];
  }
  return list;
};

const rightClick = (element: Element) =>
  fireEvent.contextMenu(element, { clientX: 120, clientY: 80, button: 2 });

async function openEditor() {
  const hero = await screen.findByRole("heading", { level: 1, name: "Fixture Game 0001" });
  rightClick(hero);
  fireEvent.click(
    await within(await screen.findByRole("menu")).findByRole("menuitem", { name: "Edit…" }),
  );
  return screen.findByRole("dialog", { name: "Edit Fixture Game 0001" });
}

const field = (dialog: HTMLElement, name: string) =>
  within(dialog).getByRole("textbox", { name }) as HTMLInputElement;

const type = (element: HTMLElement, value: string) =>
  fireEvent.change(element, { target: { value } });

const frame = (dialog: HTMLElement, name: string) =>
  dialog.querySelector(`[data-part="field"][data-field="${name}"]`) as HTMLElement;

it("edits a game's details from its menu and shows them at once", async () => {
  const app = renderApp("/games/fixture-0", { games: games() });
  const dialog = await openEditor();
  await waitFor(() => expect(field(dialog, "Title").value).toBe("Fixture Game 0001"));
  expect(field(dialog, "Description").value).toBe("From the launcher.");
  expect(within(dialog).getByRole("button", { name: "Close" })).toBeTruthy();

  type(field(dialog, "Title"), "My Fixture");
  // The sort title followed the title.
  expect(field(dialog, "Sort title").value).toBe("My Fixture");
  type(field(dialog, "Add to Tags"), "co-op, puzzle");
  fireEvent.keyDown(field(dialog, "Add to Tags"), { key: "Enter" });
  expect(
    within(frame(dialog, "tags"))
      .getAllByRole("listitem")
      .map((li) => li.textContent),
  ).toEqual(["Fixture", "co-op", "puzzle"]);
  fireEvent.click(within(frame(dialog, "tags")).getByRole("button", { name: "Remove Fixture" }));
  fireEvent.click(within(dialog).getByRole("checkbox", { name: "Rated" }));
  type(field(dialog, "Notes"), "Finish the co-op campaign.\nThen the DLC.");
  fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  await screen.findByRole("heading", { level: 1, name: "My Fixture" });
  const state = await app.library.edits.state("fixture-0");
  expect(state?.game.tags).toEqual(["co-op", "puzzle"]);
  expect(state?.game.rating).toBe(50);
  expect(state?.game.notes).toBe("Finish the co-op campaign.\nThen the DLC.");
  expect(state?.manual).toEqual(["title", "sortTitle", "tags"]);
  // Opened again, it shows what was saved and that the title is the user's.
  rightClick(await screen.findByRole("heading", { level: 1, name: "My Fixture" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "Edit…" }));
  const again = await screen.findByRole("dialog", { name: "Edit My Fixture" });
  await waitFor(() => expect(field(again, "Title").value).toBe("My Fixture"));
  expect(frame(again, "title").dataset.manual).toBe("true");
  expect(field(again, "Notes").value).toBe("Finish the co-op campaign.\nThen the DLC.");
});

it("shows what is wrong beside each field and saves nothing", async () => {
  const app = renderApp("/games/fixture-0", { games: games() });
  const save = vi.spyOn(app.library.edits, "save");
  const dialog = await openEditor();
  await waitFor(() => expect(field(dialog, "Title").value).toBe("Fixture Game 0001"));
  type(field(dialog, "Title"), "  ");
  type(field(dialog, "Release date"), "2024-02-30");
  type(field(dialog, "Description"), "New.");
  fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
  await waitFor(() => expect(within(frame(dialog, "releaseDate")).getByRole("alert")).toBeTruthy());
  expect(within(frame(dialog, "title")).getByRole("alert").textContent).toContain("title");
  expect(dialog.querySelector('[data-part="status"]')?.textContent).toContain("Nothing was saved");
  expect((await app.library.getGame("fixture-0"))?.description).toBe("From the launcher.");
  // Fixing them saves it all.
  type(field(dialog, "Title"), "Fixed");
  type(field(dialog, "Release date"), "2024-02-29");
  fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(save).toHaveBeenCalledTimes(2);
  expect((await app.library.getGame("fixture-0"))?.description).toBe("New.");
});

it("asks before closing with unsaved changes; Cancel leaves the game alone", async () => {
  const app = renderApp("/games/fixture-0", { games: games() });
  const dialog = await openEditor();
  await waitFor(() => expect(field(dialog, "Title").value).toBe("Fixture Game 0001"));
  type(field(dialog, "Notes"), "Unsaved");
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  const confirm = await screen.findByRole("alertdialog", { name: "Discard your changes?" });
  fireEvent.click(within(confirm).getByRole("button", { name: "Keep editing" }));
  await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
  expect(field(dialog, "Notes").value).toBe("Unsaved");
  fireEvent.keyDown(field(dialog, "Notes"), { key: "Escape" });
  fireEvent.click(
    within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Discard" }),
  );
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect((await app.library.edits.state("fixture-0"))?.game.notes).toBe("");
});

it("reports a field changed meanwhile and keeps the user's choice", async () => {
  const app = renderApp("/games/fixture-0", { games: games() });
  const dialog = await openEditor();
  await waitFor(() => expect(field(dialog, "Title").value).toBe("Fixture Game 0001"));
  type(field(dialog, "Description"), "Mine.");
  type(field(dialog, "Title"), "Renamed");
  // A refresh writes the description while the editor is open.
  act(() =>
    app.library.edits.refresh("fixture-0", (game) => {
      game.description = "From IGDB.";
    }),
  );
  fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
  const conflict = await within(frame(dialog, "description")).findByRole("alert");
  expect(conflict.textContent).toContain("From IGDB.");
  // Nothing saved: the title waits too.
  expect((await app.library.getGame("fixture-0"))?.title).toBe("Fixture Game 0001");
  fireEvent.click(within(conflict).getByRole("button", { name: "Keep mine" }));
  fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  const saved = await app.library.getGame("fixture-0");
  expect([saved?.title, saved?.description]).toEqual(["Renamed", "Mine."]);
});

it("takes the value saved meanwhile instead", async () => {
  const app = renderApp("/games/fixture-0", { games: games() });
  const dialog = await openEditor();
  await waitFor(() => expect(field(dialog, "Title").value).toBe("Fixture Game 0001"));
  type(field(dialog, "Description"), "Mine.");
  act(() =>
    app.library.edits.refresh("fixture-0", (game) => {
      game.description = "From IGDB.";
    }),
  );
  fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
  const conflict = await within(frame(dialog, "description")).findByRole("alert");
  fireEvent.click(within(conflict).getByRole("button", { name: "Use theirs" }));
  await waitFor(() => expect(field(dialog, "Description").value).toBe("From IGDB."));
  // Nothing left to save.
  expect(within(dialog).getByRole("button", { name: "Save" }).hasAttribute("disabled")).toBe(true);
});

it("resets a field the user set to its automatic value", async () => {
  const app = renderApp("/games/fixture-0", { games: games() });
  await app.library.edits.save("fixture-0", { description: { op: "set", value: "" } }, null);
  act(() =>
    app.library.edits.refresh("fixture-0", (game) => {
      game.description = "From IGDB.";
    }),
  );
  const dialog = await openEditor();
  await waitFor(() => expect(frame(dialog, "description").dataset.manual).toBe("true"));
  expect(field(dialog, "Description").value).toBe("");
  // Only fields with an automatic value can be reset.
  expect(within(frame(dialog, "notes")).queryByRole("button", { name: /Reset/ })).toBeNull();
  fireEvent.click(within(frame(dialog, "description")).getByRole("button", { name: /Reset/ }));
  expect(field(dialog, "Description").value).toBe("From IGDB.");
  expect(frame(dialog, "description").dataset.manual).toBeUndefined();
  fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  const state = await app.library.edits.state("fixture-0");
  expect(state?.manual).toEqual([]);
  expect(state?.game.description).toBe("From IGDB.");
});

it("keeps the changes after a failed save, to try again", async () => {
  const app = renderApp("/games/fixture-0", { games: games() });
  const save = vi
    .spyOn(app.library.edits, "save")
    .mockRejectedValueOnce(new Error("The library is busy"));
  const dialog = await openEditor();
  await waitFor(() => expect(field(dialog, "Title").value).toBe("Fixture Game 0001"));
  type(field(dialog, "Notes"), "Keep me");
  fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
  await within(dialog).findByText("The library is busy");
  expect(field(dialog, "Notes").value).toBe("Keep me");
  fireEvent.click(within(dialog).getByRole("button", { name: "Try again" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(save).toHaveBeenCalledTimes(2);
  expect((await app.library.edits.state("fixture-0"))?.game.notes).toBe("Keep me");
});

it("edits by pad alone in console mode", async () => {
  const app = renderApp("/games/fixture-0", { games: games(), mode: "console" });
  const edit = await screen.findByRole("button", { name: "Edit details" });
  act(() => edit.focus());
  app.pad.press("confirm");
  const dialog = await screen.findByRole("dialog", { name: "Edit Fixture Game 0001" });
  await waitFor(() => expect(field(dialog, "Notes").value).toBe(""));
  // RB/LB switch the editor's tabs.
  app.pad.press("tab.next");
  expect(within(dialog).getByRole("tab", { name: "Artwork" }).getAttribute("aria-selected")).toBe(
    "true",
  );
  app.pad.press("tab.prev");
  // A on the notes opens the keyboard; the pad types into them.
  const notes = field(dialog, "Notes");
  act(() => notes.focus());
  app.pad.press("confirm");
  await waitFor(() => expect(document.querySelector('[data-cs="osk"]')).not.toBeNull());
  app.pad.press("confirm");
  app.pad.press("nav.right");
  app.pad.press("confirm");
  // Start is the keyboard's Done, then the editor's Save.
  app.pad.press("menu");
  await waitFor(() => expect(document.querySelector('[data-cs="osk"]')).toBeNull());
  app.pad.press("menu");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect((await app.library.edits.state("fixture-0"))?.game.notes).toBe("qw");
});
