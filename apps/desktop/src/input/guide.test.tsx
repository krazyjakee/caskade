import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Action } from "./actions";
import { type GuideApi, GuideMenu, type RunningGame } from "./GuideMenu";
import { InputProvider, type PadAction } from "./InputProvider";

const original = Element.prototype.getBoundingClientRect;
beforeEach(() => {
  // jsdom has no layout: the dialog and its buttons get boxes.
  Element.prototype.getBoundingClientRect = function (this: Element) {
    const visible = this.matches('[role="dialog"], button');
    const index = Array.from(document.querySelectorAll("button")).indexOf(
      this as HTMLButtonElement,
    );
    const top = visible ? 100 + Math.max(index, 0) * 50 : 0;
    return {
      x: 0,
      y: top,
      left: 0,
      top,
      width: visible ? 200 : 0,
      height: visible ? 40 : 0,
      right: 200,
      bottom: top + 40,
    } as DOMRect;
  };
});
afterEach(() => {
  Element.prototype.getBoundingClientRect = original;
  cleanup();
});

function setup(running: RunningGame[]) {
  let send: (action: PadAction) => void = () => {};
  const api: GuideApi = {
    running: vi.fn(async () => running),
    returnToGame: vi.fn(async () => {}),
    stop: vi.fn(async () => {}),
  };
  const openDetails = vi.fn();
  render(
    <InputProvider
      console
      onBack={() => {}}
      subscribe={async (listener) => {
        send = listener;
        return () => {};
      }}
    >
      <GuideMenu api={api} openDetails={openDetails} />
    </InputProvider>,
  );
  const press = (action: Action) =>
    act(() => {
      send({ action, glyphs: "xbox" });
    });
  return { api, openDetails, press };
}

const celeste: RunningGame = { sessionId: "s1", gameId: "g1", title: "Celeste" };

it("guide opens the quick menu over a running game; return goes back to it", async () => {
  const { api, press } = setup([celeste]);
  press("guide");
  const menu = await screen.findByRole("dialog", { name: "Quick menu" });
  expect(menu.textContent).toContain("Celeste");
  await waitFor(() => expect(document.activeElement?.textContent).toBe("Return to game"));
  press("confirm");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(api.returnToGame).toHaveBeenCalledTimes(1);
});

it("stopping asks first, by pad, then stops the session", async () => {
  const { api, press } = setup([celeste]);
  press("guide");
  await screen.findByRole("dialog", { name: "Quick menu" });
  await waitFor(() => expect(document.activeElement?.textContent).toBe("Return to game"));
  press("nav.down");
  expect(document.activeElement?.textContent).toBe("Stop game");
  press("confirm");
  await waitFor(() => expect(document.activeElement?.textContent).toBe("Stop Celeste"));
  expect(api.stop).not.toHaveBeenCalled();
  press("confirm");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(api.stop).toHaveBeenCalledWith("s1");
});

it("opens the game's details, and back closes the menu", async () => {
  const { openDetails, press } = setup([celeste]);
  press("guide");
  await screen.findByRole("dialog", { name: "Quick menu" });
  press("back");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  press("guide");
  await screen.findByRole("dialog", { name: "Quick menu" });
  act(() => screen.getByText("Game details").click());
  expect(openDetails).toHaveBeenCalledWith("g1");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

it("guide again closes the menu", async () => {
  const { press } = setup([celeste]);
  press("guide");
  await screen.findByRole("dialog", { name: "Quick menu" });
  press("guide");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

it("shows no menu when nothing is running", async () => {
  const { api, press } = setup([]);
  press("guide");
  await waitFor(() => expect(api.running).toHaveBeenCalled());
  expect(screen.queryByRole("dialog")).toBeNull();
});
