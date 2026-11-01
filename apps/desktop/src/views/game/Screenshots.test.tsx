import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Screenshot } from "../../api/types";
import { InputProvider } from "../../input";
import { fakeLayout, fakePad, fakeViewport } from "../../test/harness";
import { Screenshots } from "./Screenshots";

let undoLayout = () => {};
beforeEach(() => {
  undoLayout = fakeLayout();
  fakeViewport();
});
afterEach(() => {
  undoLayout();
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const shots: Screenshot[] = [1, 2, 3].map((n) => ({
  thumb: `asset://shot-${n}/thumb-320.webp`,
  full: `asset://shot-${n}/original.webp`,
}));

const shown = () =>
  document.querySelector<HTMLImageElement>('[data-cs="screenshot-viewer"] [data-part="image"]')
    ?.src;
const counter = () =>
  document.querySelector('[data-cs="screenshot-viewer"] [data-part="counter"]')?.textContent;

function renderShots(options: { console?: boolean; screenshots?: Screenshot[] } = {}) {
  const pad = fakePad();
  render(
    <InputProvider console={options.console} subscribe={pad.subscribe}>
      <Screenshots title="Fixture" screenshots={options.screenshots ?? shots} />
    </InputProvider>,
  );
  return pad;
}

it("opens a thumbnail full size; the keys, the buttons and the strip step through them", async () => {
  renderShots();
  fireEvent.click(screen.getByRole("button", { name: "Fixture, screenshot 2" }));
  const dialog = await screen.findByRole("dialog", { name: "Fixture, screenshots" });
  expect(shown()).toBe(shots[1]?.full);
  expect(counter()).toBe("2 / 3");

  fireEvent.keyDown(dialog, { key: "ArrowRight" });
  expect(shown()).toBe(shots[2]?.full);
  // Round from the last to the first, and back.
  fireEvent.keyDown(dialog, { key: "ArrowRight" });
  expect(shown()).toBe(shots[0]?.full);
  fireEvent.keyDown(dialog, { key: "ArrowLeft" });
  expect(counter()).toBe("3 / 3");
  fireEvent.keyDown(dialog, { key: "Home" });
  expect(counter()).toBe("1 / 3");

  fireEvent.click(within(dialog).getByRole("button", { name: "Next screenshot" }));
  expect(counter()).toBe("2 / 3");
  fireEvent.click(within(dialog).getByRole("button", { name: "Previous screenshot" }));
  expect(counter()).toBe("1 / 3");
  const third = within(dialog).getByRole("button", { name: "Screenshot 3" });
  fireEvent.click(third);
  expect(counter()).toBe("3 / 3");
  expect(third.getAttribute("aria-current")).toBe("true");

  fireEvent.keyDown(dialog, { key: "Escape" });
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

it("follows a swipe, but not a mouse drag", async () => {
  renderShots();
  fireEvent.click(screen.getByRole("button", { name: "Fixture, screenshot 1" }));
  await screen.findByRole("dialog");
  const stage = document.querySelector('[data-part="stage"]') as HTMLElement;
  const drag = (pointerType: string, from: number, to: number) => {
    fireEvent.pointerDown(stage, { pointerType, clientX: from, clientY: 300 });
    fireEvent.pointerUp(stage, { pointerType, clientX: to, clientY: 310 });
  };
  drag("touch", 600, 200);
  expect(counter()).toBe("2 / 3");
  drag("touch", 200, 600);
  expect(counter()).toBe("1 / 3");
  // A short one is a tap.
  drag("touch", 300, 280);
  drag("mouse", 600, 200);
  expect(counter()).toBe("1 / 3");
});

it("a pad steps with left/right and LB/RB, focus follows on the strip, and B closes", async () => {
  const pad = renderShots({ console: true });
  const open = screen.getByRole("button", { name: "Fixture, screenshot 1" });
  open.focus();
  pad.press("confirm");
  await screen.findByRole("dialog");
  // The pad starts on the open screenshot's thumbnail.
  await waitFor(() =>
    expect(document.activeElement?.getAttribute("aria-label")).toBe("Screenshot 1"),
  );
  pad.press("nav.right");
  expect(counter()).toBe("2 / 3");
  expect(document.activeElement?.getAttribute("aria-label")).toBe("Screenshot 2");
  pad.press("tab.next");
  expect(counter()).toBe("3 / 3");
  pad.press("tab.prev");
  pad.press("nav.left");
  expect(counter()).toBe("1 / 3");
  expect(document.activeElement?.getAttribute("aria-label")).toBe("Screenshot 1");

  pad.press("back");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

it("keeps its four slots while loading and with no screenshots, and pads a short row", () => {
  const slots = () =>
    Array.from(document.querySelectorAll('[data-part="screenshot-slot"]'), (slot) =>
      slot.getAttribute("data-status"),
    );
  const { rerender } = render(<Screenshots title="Fixture" screenshots={[]} loading />);
  expect(slots()).toEqual(["loading", "loading", "loading", "loading"]);
  expect(screen.queryByText("No screenshots")).toBeNull();
  rerender(<Screenshots title="Fixture" screenshots={[]} />);
  expect(slots()).toEqual(["empty", "empty", "empty", "empty"]);
  screen.getByText("No screenshots");
  rerender(<Screenshots title="Fixture" screenshots={shots.slice(0, 1)} />);
  expect(slots()).toEqual(["empty", "empty", "empty"]);
  expect(screen.queryByText("No screenshots")).toBeNull();
});

it("a single screenshot has nothing to step to", async () => {
  renderShots({ screenshots: shots.slice(0, 1) });
  fireEvent.click(screen.getByRole("button", { name: "Fixture, screenshot 1" }));
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).queryByRole("button", { name: "Next screenshot" })).toBeNull();
  expect(document.querySelector('[data-part="strip"]')).toBeNull();
  fireEvent.keyDown(dialog, { key: "ArrowRight" });
  expect(counter()).toBe("1 / 1");
});
