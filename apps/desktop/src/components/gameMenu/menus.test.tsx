import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { createFixtureGames } from "../../api/fixtures";
import type { GameDetails } from "../../api/types";
import { emptyStorage, renderApp } from "../../test/harness";

afterEach(cleanup);

/** Right-clicks `element` at a point, as a mouse does. */
const rightClick = (element: Element) =>
  fireEvent.contextMenu(element, { clientX: 120, clientY: 80, button: 2 });

const menu = () => screen.findByRole("menu");
const item = async (name: string | RegExp) => within(await menu()).findByRole("menuitem", { name });

/** Two games; the first is a Caskade install. */
function games(): GameDetails[] {
  const list = createFixtureGames(2);
  const [first] = list;
  if (!first) throw new Error("fixture missing");
  first.status = "installed";
  first.favorite = false;
  first.entries = [
    {
      id: "managed",
      source: "caskade-storage",
      state: "installed",
      location: "/games/Fixture",
      canLaunch: true,
      emulated: false,
    },
  ];
  return list;
}

it("plays a game, then stops it, from the game page's menu", async () => {
  const app = renderApp("/games/fixture-0", { games: games() });
  rightClick(await screen.findByRole("heading", { level: 1, name: "Fixture Game 0001" }));
  fireEvent.click(await item("Play"));
  await waitFor(async () =>
    expect((await app.library.listSessions("fixture-0")).some((s) => s.endedAt === null)).toBe(
      true,
    ),
  );
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
  rightClick(screen.getByRole("heading", { level: 1 }));
  fireEvent.click(await item("Stop"));
  await waitFor(async () =>
    expect((await app.library.listSessions("fixture-0")).every((s) => s.endedAt !== null)).toBe(
      true,
    ),
  );
});

it("marks a favourite and opens the folder from a game's menu", async () => {
  const app = renderApp("/games/fixture-0", { games: games() });
  const reveal = vi.spyOn(app.library, "revealEntry");
  const hero = await screen.findByRole("heading", { level: 1, name: "Fixture Game 0001" });
  rightClick(hero);
  fireEvent.click(await item("Add to favourites"));
  await waitFor(async () => expect((await app.library.getGame("fixture-0"))?.favorite).toBe(true));
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
  rightClick(hero);
  await item("Remove from favourites");
  fireEvent.click(await item("Show in folder"));
  await waitFor(() => expect(reveal).toHaveBeenCalledWith("managed"));
});

it("shows a folder two entries share once", async () => {
  const list = games();
  const [first] = list;
  if (!first) throw new Error("fixture missing");
  const shared = { state: "installed", canLaunch: true, emulated: false } as const;
  first.entries = [
    { ...shared, id: "lutris", source: "lutris", location: "/steam/common/Fixture" },
    { ...shared, id: "steam", source: "steam", location: "/steam/common/Fixture" },
  ];
  const app = renderApp("/games/fixture-0", { games: list });
  const reveal = vi.spyOn(app.library, "revealEntry");
  rightClick(await screen.findByRole("heading", { level: 1, name: "Fixture Game 0001" }));
  const folders = within(await menu()).getAllByRole("menuitem", { name: /Show in folder/ });
  expect(folders).toHaveLength(1);
  expect(folders[0]?.textContent).toBe("Show in folder");
  fireEvent.click(folders[0] as Element);
  await waitFor(() => expect(reveal).toHaveBeenCalledWith("lutris"));
});

it("scopes an entry's menu to the entry, and asks before uninstalling it", async () => {
  const uninstall = vi.fn(async () => {});
  renderApp("/games/fixture-0", { games: games(), storage: { ...emptyStorage, uninstall } });
  rightClick(await screen.findByText("/games/Fixture"));
  const names = (await within(await menu()).findAllByRole("menuitem")).map(
    (each) => each.textContent,
  );
  expect(names).toEqual(["Play", "Show in folder", "Move installed files…", "Uninstall…"]);
  fireEvent.click(await item("Uninstall…"));
  const confirm = await screen.findByRole("alertdialog", { name: "Uninstall Fixture Game 0001?" });
  expect(uninstall).not.toHaveBeenCalled();
  fireEvent.click(within(confirm).getByRole("button", { name: "Uninstall" }));
  await waitFor(() => expect(uninstall).toHaveBeenCalledWith("managed"));
  await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
});

it("says why a game from the menu didn't start", async () => {
  const list = games();
  const app = renderApp("/games/fixture-0", { games: list });
  vi.spyOn(app.library, "launchGame").mockRejectedValue(new Error("The emulator is missing"));
  rightClick(await screen.findByRole("heading", { level: 1, name: "Fixture Game 0001" }));
  fireEvent.click(await item("Play"));
  const alert = await screen.findByRole("alertdialog", { name: "Fixture Game 0001 didn't start" });
  expect(alert.textContent).toContain("The emulator is missing");
  fireEvent.click(within(alert).getByRole("button", { name: "OK" }));
  await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
});

it("offers a job's buttons on a right click", async () => {
  const app = renderApp("/activity");
  rightClick(await screen.findByRole("heading", { name: "Fixture installation" }));
  const names = (await within(await menu()).findAllByRole("menuitem")).map(
    (each) => each.textContent,
  );
  expect(names).toEqual(["Cancel job"]);
  const cancel = vi.spyOn(app.installs, "cancel");
  fireEvent.click(await item("Cancel job"));
  await waitFor(() => expect(cancel).toHaveBeenCalledWith("fixture-job"));
});

it("offers the window's menu on the title bar, and leaves the search box its own", async () => {
  const app = renderApp("/");
  const search = await screen.findByRole("combobox", { name: "Search games" });
  rightClick(search);
  expect(screen.queryByRole("menu")).toBeNull();
  const bar = document.querySelector('[data-cs="topbar"]') as Element;
  rightClick(bar);
  const names = (await within(await menu()).findAllByRole("menuitem")).map(
    (each) => each.textContent,
  );
  expect(names).toEqual(["Minimize", "Maximize", "Close"]);
  fireEvent.click(await item("Maximize"));
  await waitFor(() => expect(app.window.calls).toContain("toggleMaximize"));
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
  rightClick(bar);
  await item("Restore");
});

it("turns Play into Stop when the game starts while its menu is open", async () => {
  const app = renderApp("/games/fixture-0", { games: games() });
  rightClick(await screen.findByRole("heading", { level: 1, name: "Fixture Game 0001" }));
  await item("Play");
  // Started elsewhere, or the sessions only now loaded: the same item slot.
  await act(async () => {
    await app.library.launchGame("managed");
  });
  expect(await item("Stop")).toBeTruthy();
  expect(screen.queryByText("Caskade ran into a problem")).toBeNull();
});

it("removes a game Caskade installed, and only hides one a scan found", async () => {
  const app = renderApp("/games/fixture-0", { games: games() });
  const remove = vi.spyOn(app.library, "removeGame");
  rightClick(await screen.findByRole("heading", { level: 1, name: "Fixture Game 0001" }));
  fireEvent.click(await item("Remove from library…"));
  const confirm = await screen.findByRole("alertdialog", {
    name: "Remove Fixture Game 0001 from your library?",
  });
  expect(confirm.textContent).toContain("The files Caskade installed for it are deleted");
  expect(remove).not.toHaveBeenCalled();
  fireEvent.click(within(confirm).getByRole("button", { name: "Remove" }));
  await waitFor(() => expect(remove).toHaveBeenCalledWith("fixture-0"));
  await waitFor(async () => expect(await app.library.getGame("fixture-0")).toBeFalsy());
  await waitFor(() => expect(app.router.state.location.pathname).toBe("/library"));
  cleanup();

  renderApp("/games/fixture-1", { games: games() });
  rightClick(await screen.findByRole("heading", { level: 1, name: "Fixture Game 0002" }));
  await item("Hide from library");
  expect(within(await menu()).queryByRole("menuitem", { name: "Remove from library…" })).toBeNull();
});
