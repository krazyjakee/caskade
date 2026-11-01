import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { createFixtureGames } from "../../api/fixtures";
import { DropProvider } from "../../drop/DropProvider";
import { fakeDrops } from "../../drop/testing";
import { renderApp } from "../../test/harness";

afterEach(cleanup);

const LIBRARY = "/library?filter=all&sort=title";

async function open(item: string) {
  fireEvent.click(await screen.findByRole("button", { name: "Add game" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: item }));
  return screen.findByRole("dialog");
}

const field = (dialog: HTMLElement, name: string) =>
  within(dialog).getByRole("textbox", { name: new RegExp(`^${name}`) }) as HTMLInputElement;

it("keeps a title-only record and explains why it can't start", async () => {
  const app = renderApp(LIBRARY, { games: createFixtureGames(2) });
  const dialog = await open("Add a game by title…");
  const add = within(dialog).getByRole("button", { name: "Add game" });
  expect(add.hasAttribute("disabled")).toBe(true);
  fireEvent.change(field(dialog, "Title"), { target: { value: "Chrono Trigger (cartridge)" } });
  fireEvent.click(add);
  await screen.findByRole("heading", { level: 1, name: "Chrono Trigger (cartridge)" });
  expect(screen.getByText(/Caskade can't start this game/)).toBeTruthy();
  expect(app.router.state.location.pathname).toMatch(/^\/games\/fixture-manual-/);
});

it("adds a shortcut's game in place, with its arguments and folder", async () => {
  const app = renderApp(LIBRARY, { games: createFixtureGames(2) });
  const dialog = await open("Add a game on this computer…");
  const input = field(dialog, "Program, shortcut or URI");
  fireEvent.change(input, { target: { value: "/home/me/Desktop/celeste.desktop" } });
  fireEvent.blur(input);
  const summary = await within(dialog).findByText("/home/me/Desktop/celeste/celeste");
  expect(summary).toBeTruthy();
  expect(within(dialog).getByText("--windowed")).toBeTruthy();
  await waitFor(() => expect(field(dialog, "Title").value).toBe("celeste"));
  fireEvent.change(field(dialog, "Title"), { target: { value: "Celeste" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Add game" }));
  await screen.findByRole("heading", { level: 1, name: "Celeste" });
  const id = app.router.state.location.pathname.split("/").at(-1) ?? "";
  const game = await app.library.getGame(id);
  expect(game?.entries.map((entry) => [entry.source, entry.location])).toEqual([
    ["manual", "/home/me/Desktop/celeste"],
  ]);
});

it("says why a target can't be added", async () => {
  renderApp(LIBRARY, { games: createFixtureGames(2) });
  const dialog = await open("Add a game on this computer…");
  const input = field(dialog, "Program, shortcut or URI");
  for (const [value, why] of [
    ["/games/missing.exe", "isn't there"],
    ["/games/", "is a folder"],
    ["games/relative.sh", "choose the game's program"],
  ] as const) {
    fireEvent.change(input, { target: { value } });
    fireEvent.blur(input);
    expect((await within(dialog).findByRole("alert")).textContent).toContain(why);
  }
  expect(within(dialog).getByRole("button", { name: "Add game" }).hasAttribute("disabled")).toBe(
    true,
  );
});

it("offers to join a game that may be the same, or keep it apart", async () => {
  const app = renderApp(LIBRARY, { games: createFixtureGames(2) });
  let dialog = await open("Add a game on this computer…");
  const input = field(dialog, "Program, shortcut or URI");
  fireEvent.change(input, { target: { value: "/games/Fixture/fixture.sh" } });
  fireEvent.blur(input);
  await within(dialog).findByText("/games/Fixture/fixture.sh");
  fireEvent.change(field(dialog, "Title"), { target: { value: "fixture game 0001" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Add game" }));
  const duplicates = await within(dialog).findByRole("region", {
    name: "Maybe already in your library",
  });
  expect(within(duplicates).getByText("has the same title")).toBeTruthy();
  fireEvent.click(within(duplicates).getByRole("button", { name: "Add to this game" }));
  await screen.findByRole("heading", { level: 1, name: "Fixture Game 0001" });
  expect((await app.library.getGame("fixture-0"))?.entries).toHaveLength(2);

  await act(() =>
    app.router.navigate({ to: "/library", search: { filter: "all", sort: "title" } }),
  );
  dialog = await open("Add a game by title…");
  fireEvent.change(field(dialog, "Title"), { target: { value: "Fixture Game 0002" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Add game" }));
  await within(dialog).findByRole("region", { name: "Maybe already in your library" });
  // A title-only record can't join; it can be kept apart.
  expect(within(dialog).queryByRole("button", { name: "Add to this game" })).toBeNull();
  fireEvent.click(within(dialog).getByRole("button", { name: "Keep separate" }));
  await waitFor(() => expect(app.router.state.location.pathname).toMatch(/fixture-manual-/));
});

it("takes a dropped program and removes the record without touching it", async () => {
  let target: () => Element | null = () => null;
  const drops = fakeDrops(() => target());
  const app = renderApp(LIBRARY, {
    games: createFixtureGames(1),
    wrap: (children) => (
      <DropProvider source={drops.source} hitTest={drops.hitTest}>
        {children}
      </DropProvider>
    ),
  });
  const dialog = await open("Add a game on this computer…");
  const zone = within(dialog).getByRole("region", { name: "What it starts" });
  target = () => zone;
  drops.drop("/games/Hades/Hades.exe");
  await within(dialog).findByText("a Windows compatibility tool");
  fireEvent.click(within(dialog).getByRole("button", { name: "Add game" }));
  const heading = await screen.findByRole("heading", { level: 1, name: "Hades" });
  const id = app.router.state.location.pathname.split("/").at(-1) ?? "";
  fireEvent.contextMenu(heading, { clientX: 10, clientY: 10, button: 2 });
  fireEvent.click(await screen.findByRole("menuitem", { name: "Remove from library…" }));
  const confirm = await screen.findByRole("alertdialog");
  fireEvent.click(within(confirm).getByRole("button", { name: /Remove/ }));
  await waitFor(async () => expect(await app.library.getGame(id)).toBeNull());
});

it("adds a game by title with a pad", async () => {
  const app = renderApp(LIBRARY, { games: createFixtureGames(1), mode: "console" });
  const dialog = await open("Add a game by title…");
  const title = field(dialog, "Title");
  act(() => title.focus());
  app.pad.press("confirm");
  await waitFor(() => expect(document.querySelector('[data-cs="osk"]')).not.toBeNull());
  app.pad.press("confirm");
  app.pad.press("menu");
  await waitFor(() => expect(title.value).toBe("q"));
  const add = dialog.querySelector<HTMLElement>('[data-part="add"]');
  if (!add) throw new Error("no Add button");
  act(() => add.focus());
  app.pad.press("confirm");
  await waitFor(() => expect(app.router.state.location.pathname).toMatch(/fixture-manual-/));
  expect(
    (await app.library.listGames({ filter: "all", sort: "title", offset: 0, limit: 10 })).items.map(
      (g) => g.title,
    ),
  ).toContain("q");
});
