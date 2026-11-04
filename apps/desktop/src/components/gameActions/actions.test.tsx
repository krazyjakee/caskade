/** Launch actions in the app (#178): the editor's Actions tab, Play's
 * choice and the game menu's play actions and tools, over the fixture
 * library's actions (the core's rules). */
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, onTestFinished, vi } from "vitest";
import { createFixtureGames } from "../../api/fixtures";
import type { GameDetails } from "../../api/types";
import {
  fakeLayout,
  fakeViewport,
  focused,
  playedGames,
  renderApp,
  until,
} from "../../test/harness";

afterEach(cleanup);

const rightClick = (element: Element) =>
  fireEvent.contextMenu(element, { clientX: 120, clientY: 80, button: 2 });
const type = (element: HTMLElement, value: string) =>
  fireEvent.change(element, { target: { value } });

/** A game with two releases: a store's and one Caskade installed. */
function games(): GameDetails[] {
  const list = createFixtureGames(2);
  const [first] = list;
  if (!first) throw new Error("fixture missing");
  first.status = "installed";
  first.entries = [
    {
      id: "steam",
      source: "steam",
      state: "installed",
      location: "/games/Steam/Fixture",
      canLaunch: true,
      emulated: false,
    },
    {
      id: "managed",
      source: "caskade-storage",
      state: "installed",
      location: "/games/Fixture",
      canLaunch: true,
      emulated: false,
    },
  ];
  first.defaultEntryId = "steam";
  return list;
}

async function openActions() {
  rightClick(await screen.findByRole("heading", { level: 1, name: "Fixture Game 0001" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "Edit…" }));
  const dialog = await screen.findByRole("dialog", { name: "Edit Fixture Game 0001" });
  fireEvent.click(within(dialog).getByRole("tab", { name: /Actions/ }));
  await within(dialog).findByRole("listbox", { name: "Launch actions" });
  return dialog;
}

const textbox = (scope: HTMLElement, name: string | RegExp) =>
  within(scope).getByRole("textbox", { name }) as HTMLInputElement;

async function pick(scope: HTMLElement, label: RegExp, option: string | RegExp) {
  const trigger = within(scope)
    .getAllByRole("button", { name: label })
    .find((button) => button.getAttribute("aria-haspopup") === "listbox");
  if (!trigger) throw new Error(`no ${label} list`);
  fireEvent.click(trigger);
  fireEvent.click(await screen.findByRole("option", { name: option }));
}

// Many steps, each through the whole app: more than the default 5 s on a busy machine.
it("adds, edits, orders, defaults and deletes a release's actions", {
  timeout: 30_000,
}, async () => {
  const app = renderApp("/games/fixture-0", { games: games() });
  const dialog = await openActions();
  // The release's own launch is its built-in action: its target is the
  // store's, so only its name and tracking change.
  expect(within(dialog).getByRole("option", { name: /Play/ })).toBeTruthy();
  expect(textbox(dialog, "Program").disabled).toBe(true);
  expect(within(dialog).getByText(/Duplicate it to change what it starts/)).toBeTruthy();

  // A mod manager that starts the game, tracked by the game's name.
  fireEvent.click(within(dialog).getByRole("button", { name: "Add play action" }));
  type(textbox(dialog, "Name"), "Modded");
  type(textbox(dialog, "Program"), "/games/Fixture/ModOrganizer.exe");
  fireEvent.click(within(dialog).getByRole("button", { name: "Add argument" }));
  type(textbox(dialog, "Argument 1"), "-p Default profile");
  fireEvent.click(within(dialog).getByRole("button", { name: "Add variable" }));
  type(textbox(dialog, "Variable 1 name"), "MODS");
  type(textbox(dialog, "Variable 1 value"), "on");
  await pick(dialog, /Which programs are the game/, "Programs by name");
  // Unsaved work is marked on the tab.
  await waitFor(() =>
    expect(
      within(dialog)
        .getByRole("tab", { name: /Actions/ })
        .getAttribute("data-dirty"),
    ).not.toBeNull(),
  );
  // No names yet: the core's reason shows beside the form, nothing is lost.
  fireEvent.click(within(dialog).getByRole("button", { name: "Save action" }));
  expect((await within(dialog).findByRole("alert")).textContent).toMatch(/process names/);
  expect(textbox(dialog, "Name").value).toBe("Modded");
  fireEvent.click(within(dialog).getByRole("button", { name: "Add program name" }));
  type(textbox(dialog, "Program name 1"), "Fixture.exe");
  fireEvent.click(within(dialog).getByRole("button", { name: "Save action" }));
  await within(dialog).findByRole("option", { name: /Modded/ });
  const [steam] = await app.library.actions.list("fixture-0");
  const modded = steam?.actions.find((action) => action.name === "Modded");
  expect(modded?.target).toEqual({
    type: "executable",
    path: "/games/Fixture/ModOrganizer.exe",
    args: ["-p Default profile"],
    cwd: null,
    env: { MODS: "on" },
  });
  expect(modded?.tracking).toEqual({ mode: "process_name", folder: null, names: ["Fixture.exe"] });

  // Two tools, reordered.
  for (const name of ["Configure", "Mod setup"]) {
    fireEvent.click(within(dialog).getByRole("button", { name: "Add tool" }));
    type(textbox(dialog, "Name"), name);
    type(textbox(dialog, "Program"), `/games/Fixture/${name}.exe`);
    fireEvent.click(within(dialog).getByRole("button", { name: "Save action" }));
    await within(dialog).findByRole("option", { name: new RegExp(name) });
  }
  fireEvent.click(within(dialog).getByRole("button", { name: "Move up" }));
  await waitFor(async () =>
    expect(
      (await app.library.actions.list("fixture-0"))[0]?.actions.map((action) => action.name),
    ).toEqual(["Play", "Modded", "Mod setup", "Configure"]),
  );

  // Play asks which action to start.
  await pick(dialog, /Play starts/, "Ask each time");
  await waitFor(async () =>
    expect((await app.library.actions.list("fixture-0"))[0]?.settings.ask).toBe(true),
  );

  // A rename is reverted, then the tool is deleted after a confirmation.
  fireEvent.click(within(dialog).getByRole("option", { name: /Configure/ }));
  type(textbox(dialog, "Name"), "Renamed");
  fireEvent.click(within(dialog).getByRole("button", { name: "Revert" }));
  expect(textbox(dialog, "Name").value).toBe("Configure");
  fireEvent.click(within(dialog).getByRole("button", { name: "Delete…" }));
  fireEvent.click(within(dialog).getByRole("button", { name: "Delete “Configure”?" }));
  await waitFor(() =>
    expect(within(dialog).queryByRole("option", { name: /Configure/ })).toBeNull(),
  );

  // The other release has its own actions.
  await pick(dialog, /Release/, /Fixture · caskade-storage/);
  await waitFor(() =>
    expect(
      within(dialog)
        .getAllByRole("option")
        .map((option) => option.textContent),
    ).toEqual(["Play"]),
  );
});

it("duplicates the built-in action into one whose target can change", async () => {
  const app = renderApp("/games/fixture-0", { games: games() });
  const dialog = await openActions();
  fireEvent.click(within(dialog).getByRole("button", { name: "Duplicate" }));
  expect(textbox(dialog, "Name").value).toBe("Play (copy)");
  expect(textbox(dialog, "Program").disabled).toBe(false);
  type(textbox(dialog, "Program"), "/games/Steam/Fixture/game-dx11");
  fireEvent.click(within(dialog).getByRole("button", { name: "Save action" }));
  await within(dialog).findByRole("option", { name: /Play \(copy\)/ });
  const [steam] = await app.library.actions.list("fixture-0");
  expect(steam?.actions).toHaveLength(2);
});

it("asks which action Play starts and launches the one chosen", async () => {
  const app = renderApp("/games/fixture-0", { games: games() });
  const [steam] = await app.library.actions.list("fixture-0");
  if (!steam) throw new Error("no release");
  const modded = await app.library.actions.save("fixture-0", {
    ...steam.actions[0],
    id: "",
    name: "Modded",
  } as never);
  await app.library.actions.saveSettings("fixture-0", "steam", {
    defaultAction: modded.id,
    ask: true,
    builtIn: { name: null, tracking: null },
  });
  const launch = vi.spyOn(app.library, "launchGame");
  rightClick(await screen.findByRole("heading", { level: 1, name: "Fixture Game 0001" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "Play" }));
  const choice = await screen.findByRole("dialog", { name: "Choose how to play" });
  // The default is chosen already; the other can be.
  expect((within(choice).getByRole("radio", { name: /Modded/ }) as HTMLInputElement).checked).toBe(
    true,
  );
  fireEvent.click(within(choice).getByRole("radio", { name: /^Play/ }));
  fireEvent.click(within(choice).getByRole("button", { name: "Play" }));
  await waitFor(() =>
    expect(launch).toHaveBeenLastCalledWith(
      "steam",
      expect.objectContaining({ action: "source:steam" }),
    ),
  );
  await waitFor(async () =>
    expect((await app.library.listSessions("fixture-0")).some((s) => s.endedAt === null)).toBe(
      true,
    ),
  );
});

it("plays with an action and runs a tool from the game's menu", async () => {
  const app = renderApp("/games/fixture-0", { games: games() });
  const [steam] = await app.library.actions.list("fixture-0");
  if (!steam) throw new Error("no release");
  const base = steam.actions[0];
  if (!base) throw new Error("no built-in action");
  const modded = await app.library.actions.save("fixture-0", { ...base, id: "", name: "Modded" });
  const tool = await app.library.actions.save("fixture-0", {
    ...base,
    id: "",
    name: "Configure",
    kind: "utility",
  });
  const launch = vi.spyOn(app.library, "launchGame");
  // The game page offers the tool beside Play.
  expect(await screen.findByRole("button", { name: "Configure" })).toBeTruthy();
  rightClick(await screen.findByRole("heading", { level: 1, name: "Fixture Game 0001" }));
  const menu = await screen.findByRole("menu");
  expect(within(menu).getByRole("menuitem", { name: /Configure/ })).toBeTruthy();
  fireEvent.click(within(menu).getByRole("menuitem", { name: "Play with Modded" }));
  await waitFor(() =>
    expect(launch).toHaveBeenLastCalledWith(
      "steam",
      expect.objectContaining({ action: modded.id }),
    ),
  );
  // A tool starts no session.
  rightClick(await screen.findByRole("heading", { level: 1, name: "Fixture Game 0001" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: /Configure/ }));
  await waitFor(() => expect(app.library.actions.ran).toEqual([tool.id]));
});

it("chooses a play action in Play's question with a pad", async () => {
  const undoLayout = fakeLayout();
  fakeViewport();
  onTestFinished(undoLayout);
  const list = playedGames(5);
  const app = renderApp("/games/fixture-4", { mode: "console", games: list });
  const [release] = await app.library.actions.list("fixture-4");
  const own = release?.actions[0];
  if (!release || !own) throw new Error("no launch");
  const modded = await app.library.actions.save("fixture-4", { ...own, id: "", name: "Modded" });
  await app.library.actions.saveSettings("fixture-4", release.entryId, {
    ...release.settings,
    ask: true,
  });
  const launch = vi.spyOn(app.library, "launchGame");
  await until(() => focused()?.textContent === "Play", "Play");
  app.pad.press("confirm");
  await screen.findByRole("dialog", { name: "Choose how to play" });
  // It opens on the default, the release's own launch.
  await until(() => focused()?.closest("label")?.textContent?.startsWith("Play") ?? false, "Play");
  app.pad.press("nav.down");
  await until(() => focused()?.closest("label")?.textContent === "Modded", "Modded");
  app.pad.press("confirm");
  // Down to the buttons, on to Play, A.
  app.pad.press("nav.down");
  await until(() => focused()?.tagName === "BUTTON", "the buttons");
  // The test layout stacks controls in document order: Play is below Cancel.
  if (focused()?.textContent !== "Play") app.pad.press("nav.down");
  await until(() => focused()?.textContent === "Play", "the dialog's Play");
  app.pad.press("confirm");
  await waitFor(() =>
    expect(launch).toHaveBeenLastCalledWith(
      release.entryId,
      expect.objectContaining({ action: modded.id }),
    ),
  );
});
