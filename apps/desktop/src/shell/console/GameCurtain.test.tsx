import { act, cleanup, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it } from "vitest";
import { fakeLayout, playedGames, renderApp } from "../../test/harness";

let undoLayout = () => {};
beforeEach(() => {
  undoLayout = fakeLayout();
  Element.prototype.scrollIntoView = () => {};
});
afterEach(() => {
  undoLayout();
  cleanup();
  delete document.documentElement.dataset.mode;
});

const curtain = () => document.querySelector('[data-cs="game-curtain"]');
const state = () => curtain()?.getAttribute("data-state");

async function play(app: ReturnType<typeof renderApp>) {
  const reply = await act(() => app.library.launchGame("entry-1"));
  if (reply.type !== "started") throw new Error("the fixture game didn't start");
  await waitFor(() => expect(state()).toBe("covered"));
  return reply.ticket.sessionId;
}

it("fades to black when a game starts in console mode, and back when it exits", async () => {
  const app = renderApp("/", { mode: "console", games: playedGames(3) });
  await screen.findByText("Select");
  expect(state()).toBe("open");

  const session = await play(app);
  await waitFor(() =>
    expect(curtain()?.querySelector('[data-part="title"]')?.textContent).toBe("Fixture Game 0002"),
  );
  // It takes the pad: focus is on the curtain, not the view behind it.
  expect(screen.getByRole("dialog", { name: "Playing" })).toBeTruthy();
  // Caskade never minimizes for it.
  expect(app.window.calls).not.toContain("minimize");

  await act(() => app.library.stopGame?.(session));
  await waitFor(() => expect(state()).toBe("open"));
  expect(screen.queryByRole("dialog", { name: "Playing" })).toBeNull();
});

it("lifts when Caskade has focus again after the game took it", async () => {
  const app = renderApp("/", { mode: "console", games: playedGames(3) });
  await screen.findByText("Select");
  await play(app);
  // Focus that never left (the game's window isn't up yet) changes nothing.
  act(() => app.window.focus(true));
  expect(state()).toBe("covered");
  act(() => app.window.focus(false));
  expect(state()).toBe("covered");
  // Guide, or switching back to Caskade.
  act(() => app.window.focus(true));
  await waitFor(() => expect(state()).toBe("open"));
});

it("lifts on back while the game runs", async () => {
  const app = renderApp("/", { mode: "console", games: playedGames(3) });
  await screen.findByText("Select");
  await play(app);
  await waitFor(() => expect(document.activeElement?.closest('[role="dialog"]')).not.toBeNull());
  app.pad.press("back");
  await waitFor(() => expect(state()).toBe("open"));
});

it("has no curtain in desktop mode", async () => {
  const app = renderApp("/", { games: playedGames(3) });
  await screen.findByText("Welcome back");
  await act(() => app.library.launchGame("entry-1"));
  expect(curtain()).toBeNull();
  expect(screen.queryByRole("dialog", { name: "Playing" })).toBeNull();
});
