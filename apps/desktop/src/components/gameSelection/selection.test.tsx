import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createFixtureGames } from "../../api/fixtures";
import { fakeViewport, renderApp } from "../../test/harness";

beforeEach(() => {
  fakeViewport();
  vi.stubGlobal("scrollTo", vi.fn());
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const LIBRARY = "/library?filter=all&sort=title&layout=grid";

const card = (title: string) => screen.getByRole("link", { name: new RegExp(`^${title}`) });
const chosen = (title: string) =>
  card(title).closest('[data-cs="game-card"]')?.getAttribute("data-selected") === "true";
const bar = () => screen.getByRole("toolbar", { name: "Selected games" });
const count = () => within(bar()).getByRole("status").textContent;

it("selects by click and range across a virtualized 1,000-game library, and all of it", async () => {
  const app = renderApp(LIBRARY, { games: createFixtureGames(1000) });
  await screen.findAllByText("Fixture Game 0001");
  fireEvent.click(screen.getByRole("button", { name: "Select" }));
  fireEvent.click(card("Fixture Game 0001"));
  fireEvent.click(card("Fixture Game 0004"), { shiftKey: true });
  expect(count()).toBe("4 games selected");
  expect(chosen("Fixture Game 0003")).toBe(true);
  // A click toggles; selection mode never opens the game.
  fireEvent.click(card("Fixture Game 0003"));
  expect(count()).toBe("3 games selected");
  expect(app.router.state.location.pathname).toBe("/library");

  // Scrolled far away and back, the chosen games stay chosen by id.
  const viewport = document.querySelector('[data-cs="virtual-library"]');
  if (!viewport) throw new Error("no viewport");
  Object.defineProperty(viewport, "scrollTop", { configurable: true, value: 40_000 });
  fireEvent.scroll(viewport);
  await waitFor(() => expect(screen.queryAllByText("Fixture Game 0001")).toHaveLength(0));
  expect(count()).toBe("3 games selected");
  Object.defineProperty(viewport, "scrollTop", { configurable: true, value: 0 });
  fireEvent.scroll(viewport);
  await screen.findAllByText("Fixture Game 0001");
  expect(chosen("Fixture Game 0001")).toBe(true);
  expect(chosen("Fixture Game 0003")).toBe(false);

  // The loaded games are fewer than the whole result; select all says so.
  fireEvent.click(within(bar()).getByRole("button", { name: "Clear" }));
  fireEvent.click(within(bar()).getByRole("button", { name: "Select loaded" }));
  const loaded = Number(count()?.split(" ")[0]);
  expect(loaded).toBeGreaterThan(0);
  expect(loaded).toBeLessThan(1000);
  fireEvent.click(within(bar()).getByRole("button", { name: "Select all 1,000 games" }));
  await waitFor(() => expect(count()).toBe("1,000 games selected"));
  fireEvent.click(within(bar()).getByRole("button", { name: "Done" }));
  expect(screen.queryByRole("toolbar", { name: "Selected games" })).toBeNull();
});

async function selectThree() {
  await screen.findAllByText("Fixture Game 0001");
  fireEvent.click(screen.getByRole("button", { name: "Select" }));
  for (const title of ["Fixture Game 0001", "Fixture Game 0002", "Fixture Game 0003"])
    fireEvent.click(card(title));
  fireEvent.click(within(bar()).getByRole("button", { name: "Edit selected…" }));
  return screen.findByRole("dialog", { name: "Edit 3 games" });
}

const addTag = (dialog: HTMLElement, tag: string) => {
  fireEvent.click(within(dialog).getByRole("checkbox", { name: "Change Tags" }));
  const field = within(dialog).getByRole("textbox", { name: "Add to Tags" });
  fireEvent.change(field, { target: { value: tag } });
  fireEvent.keyDown(field, { key: "Enter" });
};

it("previews a bulk edit, applies only the ticked field and cancels cleanly", async () => {
  const games = createFixtureGames(4);
  games[1] = { ...games[1], tags: ["Fixture", "co-op"] } as (typeof games)[number];
  games.forEach((game, i) => {
    game.description = `Description ${i}`;
  });
  const app = renderApp(LIBRARY, { games });
  let dialog = await selectThree();
  // Nothing ticked: nothing to preview.
  expect(within(dialog).getByRole("button", { name: "Preview" }).hasAttribute("disabled")).toBe(
    true,
  );
  addTag(dialog, "co-op");
  fireEvent.click(within(dialog).getByRole("button", { name: "Preview" }));
  const preview = await within(dialog).findByRole("region", { name: "What changes" });
  expect(within(dialog).getByText("2 games change")).toBeTruthy();
  expect(within(dialog).getByText("1 already has it")).toBeTruthy();
  expect(
    within(preview)
      .getAllByRole("listitem")
      .map((li) => li.querySelector("p")?.textContent),
  ).toEqual(["Fixture Game 0001", "Fixture Game 0003"]);
  // Back, then Cancel: nothing saved.
  fireEvent.click(within(dialog).getByRole("button", { name: "Back" }));
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect((await app.library.getGame("fixture-0"))?.tags).toEqual(["Fixture"]);

  fireEvent.click(within(bar()).getByRole("button", { name: "Edit selected…" }));
  dialog = await screen.findByRole("dialog", { name: "Edit 3 games" });
  addTag(dialog, "co-op");
  fireEvent.click(within(dialog).getByRole("button", { name: "Preview" }));
  await within(dialog).findByRole("region", { name: "What changes" });
  fireEvent.click(within(dialog).getByRole("button", { name: "Apply" }));
  await within(dialog).findByText("2 games saved");
  fireEvent.click(within(dialog).getByRole("button", { name: "Done" }));
  for (const id of ["fixture-0", "fixture-1", "fixture-2"])
    expect((await app.library.getGame(id))?.tags).toContain("co-op");
  // The rest of each game stays its own.
  expect((await app.library.getGame("fixture-1"))?.description).toBe("Description 1");
  expect((await app.library.getGame("fixture-3"))?.tags).toEqual(["Fixture"]);
});

it("reports invalid games and a failed apply, and a retry adds nothing twice", async () => {
  const app = renderApp(LIBRARY, { games: createFixtureGames(4) });
  const bulk = vi.spyOn(app.library.edits, "bulkEdit");
  let dialog = await selectThree();
  fireEvent.click(within(dialog).getByRole("checkbox", { name: "Change Release date" }));
  fireEvent.change(within(dialog).getByRole("textbox", { name: "Release date" }), {
    target: { value: "someday" },
  });
  fireEvent.click(within(dialog).getByRole("button", { name: "Preview" }));
  const rejected = await within(dialog).findByRole("region", { name: "Not saved" });
  expect(within(rejected).getAllByRole("listitem")).toHaveLength(3);
  expect(within(dialog).getByText("3 can't take it")).toBeTruthy();
  fireEvent.click(within(dialog).getByRole("button", { name: "Back" }));
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

  fireEvent.click(within(bar()).getByRole("button", { name: "Edit selected…" }));
  dialog = await screen.findByRole("dialog", { name: "Edit 3 games" });
  addTag(dialog, "co-op");
  fireEvent.click(within(dialog).getByRole("button", { name: "Preview" }));
  await within(dialog).findByRole("region", { name: "What changes" });
  bulk.mockRejectedValueOnce(new Error("the library is busy"));
  fireEvent.click(within(dialog).getByRole("button", { name: "Apply" }));
  await within(dialog).findByText(/That didn't finish: the library is busy/);
  fireEvent.click(within(dialog).getByRole("button", { name: "Try again" }));
  await within(dialog).findByText("3 games saved");
  expect((await app.library.getGame("fixture-0"))?.tags).toEqual(["Fixture", "co-op"]);
});

it("selects and edits by pad alone", async () => {
  const app = renderApp(LIBRARY, { games: createFixtureGames(6), mode: "console" });
  await screen.findAllByText("Fixture Game 0001");
  const focusOn = (element: HTMLElement) => act(() => element.focus());
  focusOn(screen.getByRole("button", { name: "Select" }));
  app.pad.press("confirm");
  for (const title of ["Fixture Game 0002", "Fixture Game 0005"]) {
    focusOn(card(title));
    app.pad.press("confirm");
  }
  expect(count()).toBe("2 games selected");
  // Start on a chosen game opens the bulk editor.
  app.pad.press("menu");
  const dialog = await screen.findByRole("dialog", { name: "Edit 2 games" });
  focusOn(within(dialog).getByRole("checkbox", { name: "Change Your rating" }));
  app.pad.press("confirm");
  focusOn(within(dialog).getByRole("checkbox", { name: "Rated" }));
  app.pad.press("confirm");
  // Start: Preview, Apply, Done.
  app.pad.press("menu");
  await within(dialog).findByRole("region", { name: "What changes" });
  app.pad.press("menu");
  await within(dialog).findByText("2 games saved");
  app.pad.press("menu");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect((await app.library.edits.state("fixture-1"))?.game.rating).toBe(50);
  expect((await app.library.edits.state("fixture-0"))?.game.rating).toBeNull();
  // B leaves selection mode.
  focusOn(card("Fixture Game 0001"));
  app.pad.press("back");
  await waitFor(() => expect(screen.queryByRole("toolbar", { name: "Selected games" })).toBeNull());
});

it("selects a manual collection's games", async () => {
  renderApp("/collections/collection-weekend", { games: createFixtureGames(6) });
  await screen.findAllByText("Fixture Game 0002");
  fireEvent.click(screen.getByRole("button", { name: "Select" }));
  fireEvent.click(within(bar()).getByRole("button", { name: "Select all 3 games" }));
  await waitFor(() => expect(count()).toBe("3 games selected"));
});
