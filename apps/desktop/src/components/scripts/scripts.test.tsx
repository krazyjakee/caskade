/** Lifecycle scripts in the app (#180): the global settings, a game's
 * Scripts tab and an emulator profile's, with deliberate tests over the
 * fixture library's pretend runner. */
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, onTestFinished } from "vitest";
import { createFixtureGames } from "../../api/fixtures";
import { fakeLayout, fakeViewport, focused, playedGames, renderApp } from "../../test/harness";

afterEach(cleanup);

const type = (element: HTMLElement, value: string) =>
  fireEvent.change(element, { target: { value } });
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

async function globalScripts() {
  renderApp("/settings?tab=scripts");
  const panel = await screen
    .findByRole("region", { name: /Global scripts/ })
    .catch(() =>
      screen.findByText("Global scripts").then((node) => node.closest("section") as HTMLElement),
    );
  return panel;
}

async function newScript(panel: HTMLElement, name: string, text: string) {
  fireEvent.click(await within(panel).findByRole("button", { name: "Add script" }));
  type(textbox(panel, "Name"), name);
  type(textbox(panel, "Script"), text);
}

it("tests a global script only when asked, then saves it and a disabled copy", async () => {
  const panel = await globalScripts();
  await newScript(panel, "Mount ISO", "echo mounted");
  // Nothing runs until Test.
  fireEvent.click(within(panel).getByRole("button", { name: "Test" }));
  const status = await within(panel).findByText(/^Finished/);
  expect(status).toBeTruthy();
  expect(within(panel).getByText("ran Mount ISO")).toBeTruthy();
  // The values it saw, as a test.
  expect(within(panel).getByText(/CASKADE_TEST/)).toBeTruthy();
  fireEvent.click(within(panel).getByRole("button", { name: "Save script" }));
  await within(panel).findByRole("option", { name: /Mount ISO/ });
  // A copy starts off, so copying never adds a running command.
  fireEvent.click(within(panel).getByRole("button", { name: "Duplicate" }));
  expect(
    (within(panel).getByRole("switch", { name: "Runs with launches" }) as HTMLInputElement).checked,
  ).toBe(false);
  fireEvent.click(within(panel).getByRole("button", { name: "Save script" }));
  await within(panel).findByRole("option", { name: /Mount ISO \(copy\)/ });
  const order = within(panel).getByRole("region", { name: "Order at launch" });
  await waitFor(() =>
    expect(
      within(order)
        .getAllByRole("listitem")
        .map((item) => [item.textContent, item.getAttribute("data-enabled")]),
    ).toEqual([
      ["Mount ISOGlobal", "true"],
      ["Mount ISO (copy)Global · Off", "false"],
    ]),
  );
});

it("shows a failure, a missing interpreter and a refused script without losing them", async () => {
  const panel = await globalScripts();
  await newScript(panel, "Check", "exit 2");
  fireEvent.click(within(panel).getByRole("button", { name: "Test" }));
  await within(panel).findByText(/Failed with exit code 2/);
  expect(within(panel).getByText("it failed")).toBeTruthy();

  await pick(panel, /Shell/, "pwsh");
  fireEvent.click(within(panel).getByRole("button", { name: "Test" }));
  await within(panel).findByText(/Didn't start/);
  expect(within(panel).getByText(/“pwsh” wasn't found/)).toBeTruthy();

  // An invalid script is refused, by Test and by Save, and stays as typed.
  type(textbox(panel, "Name"), " ");
  fireEvent.click(within(panel).getByRole("button", { name: "Test" }));
  expect((await within(panel).findByRole("alert")).textContent).toMatch(/invalid script name/);
  fireEvent.click(within(panel).getByRole("button", { name: "Save script" }));
  await waitFor(() =>
    expect(
      within(panel)
        .getAllByRole("alert")
        .some((alert) => /invalid/.test(alert.textContent ?? "")),
    ).toBe(true),
  );
  expect(textbox(panel, "Script").value).toBe("exit 2");
});

it("cancels a slow test", async () => {
  const panel = await globalScripts();
  await newScript(panel, "Slow", "sleep 600");
  fireEvent.click(within(panel).getByRole("button", { name: "Test" }));
  fireEvent.click(await within(panel).findByRole("button", { name: "Cancel test" }));
  await within(panel).findByText(/^Cancelled/);
  expect(within(panel).getByRole("button", { name: "Test" })).toBeTruthy();
});

it("shows a game's scripts after the inherited global ones, in launch order", async () => {
  const games = createFixtureGames(2);
  const app = renderApp("/games/fixture-1", { games });
  await app.library.scripts.save(
    { type: "global" },
    {
      id: "",
      scope: { type: "global" },
      name: "VPN",
      enabled: true,
      phase: "before_launch",
      command: { type: "shell", shell: "sh", script: "vpn up" },
      cwd: null,
      timeoutSecs: 60,
    },
  );
  fireEvent.click(await screen.findByRole("button", { name: "Edit details" }));
  const dialog = await screen.findByRole("dialog", { name: /^Edit / });
  fireEvent.click(within(dialog).getByRole("tab", { name: /Scripts/ }));
  // Every tab stays mounted: work in the Scripts panel.
  const tab = within(dialog)
    .getAllByRole("tabpanel")
    .find((panel) => panel.querySelector('[data-cs="scripts"]')) as HTMLElement;
  await newScript(tab, "Mods", "link-mods");
  await pick(tab, /When/, "After the game exits");
  fireEvent.click(within(tab).getByRole("button", { name: "Save script" }));
  await within(tab).findByRole("option", { name: /Mods/ });
  const order = within(tab).getByRole("region", { name: "Order at launch" });
  await waitFor(() =>
    expect(
      within(order)
        .getAllByRole("listitem")
        .map((item) => [item.textContent, item.hasAttribute("data-inherited")]),
    ).toEqual([
      ["VPNGlobal, inherited", true],
      ["ModsThis game", false],
    ]),
  );
  // A game script's test sees the game.
  fireEvent.click(within(tab).getByRole("button", { name: "Test" }));
  await within(tab).findByText(/^Finished/);
  expect(within(tab).getByText("Fixture Game 0002")).toBeTruthy();
});

it("adds, tests and saves a game's script with a pad", async () => {
  const undoLayout = fakeLayout();
  fakeViewport();
  onTestFinished(undoLayout);
  const app = renderApp("/games/fixture-4", { mode: "console", games: playedGames(5) });
  /** Presses `direction` until focus is on `name` (a button's text). */
  const walk = async (direction: "nav.down" | "nav.up", name: string) => {
    for (let i = 0; i < 60 && focused()?.textContent?.trim() !== name; i++) {
      app.pad.press(direction);
      await new Promise((resolve) => setTimeout(resolve, 0));
    }
    await waitFor(() => expect(focused()?.textContent?.trim()).toBe(name));
  };
  await waitFor(() => expect(focused()?.textContent).toBe("Play"));
  await walk("nav.down", "Edit details");
  app.pad.press("confirm");
  const dialog = await screen.findByRole("dialog", { name: /^Edit / });
  // RB along the editor's tabs to Scripts, one tab at a time: each press
  // waits for the tab it moved to, so a slow render never overshoots.
  const selected = () =>
    within(dialog)
      .getAllByRole("tab")
      .find((tab) => tab.getAttribute("aria-selected") === "true")
      ?.textContent?.trim();
  // A press before the editor's tab bindings are in place does nothing:
  // press again.
  for (let i = 0; i < 20 && !/Scripts/.test(selected() ?? ""); i++) {
    const before = selected();
    app.pad.press("tab.next");
    await waitFor(() => expect(selected()).not.toBe(before), { timeout: 250 }).catch(() => {});
  }
  expect(selected()).toMatch(/Scripts/);
  const tab = within(dialog)
    .getAllByRole("tabpanel")
    .find((panel) => panel.querySelector('[data-cs="scripts"]')) as HTMLElement;
  within(tab).getByRole("button", { name: "Add script" }).focus();
  app.pad.press("confirm");
  // The on-screen keyboard's text, as the tests elsewhere stand it in.
  type(await within(tab).findByRole("textbox", { name: "Script" }), "echo ready");
  await walk("nav.down", "Test");
  app.pad.press("confirm");
  await within(tab).findByText(/^Finished/);
  await walk("nav.up", "Save script");
  app.pad.press("confirm");
  await within(tab).findByRole("option", { name: /New script/ });
  expect(
    (await app.library.scripts.list({ type: "game", gameId: "fixture-4" }))[0]?.command,
  ).toEqual({
    type: "shell",
    shell: "sh",
    script: "echo ready",
  });
});
