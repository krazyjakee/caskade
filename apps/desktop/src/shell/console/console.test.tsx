import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { Gamepad } from "lucide-react";
import { Button } from "react-aria-components";
import { afterEach, beforeEach, expect, it } from "vitest";
import { ShelfRow } from "../../components/ShelfRow";
import { InputProvider } from "../../input";
import { fakeLayout, fakePad, playedGames, renderApp } from "../../test/harness";
import { glyph } from "./glyphs";
import { ModeProvider } from "./mode";

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

const shell = () =>
  document.querySelector("[data-cs]:is([data-cs='app-shell'], [data-cs='console-shell'])");

it("the toggle switches the shell and remembers it; View and the core switch it too", async () => {
  const app = renderApp("/");
  await screen.findByText("Welcome back");
  expect(shell()?.getAttribute("data-cs")).toBe("app-shell");
  expect(document.documentElement.dataset.mode).toBe("desktop");

  // The topbar button, next to maximize.
  const controls = document.querySelector('[data-cs="window-controls"]') as HTMLElement;
  const parts = Array.from(controls.querySelectorAll("[data-part]")).map((part) =>
    part.getAttribute("data-part"),
  );
  expect(parts).toEqual(["minimize", "console", "maximize", "close"]);
  fireEvent.click(within(controls).getByRole("button", { name: "Console mode" }));
  await waitFor(() => expect(shell()?.getAttribute("data-cs")).toBe("console-shell"));
  expect(document.documentElement.dataset.mode).toBe("console");
  expect(app.console.calls).toEqual(["console"]);
  // The same view, in the console shell.
  expect(screen.getByText("Welcome back")).toBeTruthy();
  expect(document.querySelector('[data-cs="window-controls"]')).toBeNull();

  // View (Select) leaves.
  app.pad.press("view");
  await waitFor(() => expect(shell()?.getAttribute("data-cs")).toBe("app-shell"));
  expect(app.console.calls).toEqual(["console", "desktop"]);

  // `caskade --console` while Caskade runs: the core announces it.
  app.console.announce("console");
  await waitFor(() => expect(shell()?.getAttribute("data-cs")).toBe("console-shell"));
  expect(app.console.calls).toEqual(["console", "desktop"]);

  // The sidebar's way back.
  fireEvent.click(screen.getByRole("button", { name: "Desktop mode" }));
  await waitFor(() => expect(shell()?.getAttribute("data-cs")).toBe("app-shell"));
  expect(app.console.calls).toEqual(["console", "desktop", "desktop"]);
});

const hints = () =>
  Array.from(document.querySelectorAll('[data-cs="hint-bar"] [data-part="hint"]')).map(
    (hint) =>
      `${hint.querySelector('[data-part="glyph"]')?.textContent} ${hint.querySelector('[data-part="label"]')?.textContent}`,
  );

it("the hint bar shows what the buttons do where focus is, in the pad's glyphs", async () => {
  const app = renderApp("/", { mode: "console", games: playedGames() });
  await screen.findByText("Welcome back");
  // Home opens on the hero's action once it has loaded (#150).
  await waitFor(() => expect(document.activeElement?.textContent).toMatch(/Resume/));
  // No pad yet: keys.
  expect(hints()).toEqual([
    "Enter Select",
    "Esc Back",
    "/ Search",
    "PgUp Previous page",
    "PgDn Next page",
    "V Desktop mode",
  ]);
  expect(document.querySelector('[data-cs="hint-bar"]')?.getAttribute("data-glyphs")).toBe(
    "keyboard",
  );
  // The search names the console's key, not the desktop's Ctrl K.
  const searchKey = () =>
    document.querySelector('[data-cs="search"] [data-part="shortcut"]')?.textContent;
  expect(searchKey()).toBe("/");

  // Focus on a Recently Played card: the shelf pages with LB/RB.
  await waitFor(() =>
    expect(document.querySelector('[data-cs="shelf-row"] [data-part="items"] a')).not.toBeNull(),
  );
  const card = [
    document.querySelector('[data-cs="shelf-row"] [data-part="items"] a') as HTMLElement,
  ];
  act(() => (card[0] as HTMLElement).focus());
  app.pad.press("nav.left", "xbox");
  act(() => (card[0] as HTMLElement).focus());
  await waitFor(() =>
    expect(hints()).toEqual([
      "A Select",
      "B Back",
      "X Details",
      "Y Search",
      "LT Previous page",
      "RT Next page",
      "Menu Options",
      "View Desktop mode",
      "Guide Quick menu",
    ]),
  );
  expect(searchKey()).toBe("Y");

  // Another controller in hand: its glyphs.
  app.pad.press("nav.left", "playstation");
  act(() => (card[0] as HTMLElement).focus());
  await waitFor(() => expect(hints()[0]).toBe("✕ Select"));
  expect(searchKey()).toBe("△");
  expect(hints()).toContain("L2 Previous page");
  app.pad.press("nav.left", "nintendo");
  act(() => (card[0] as HTMLElement).focus());
  await waitFor(() => expect(hints()).toContain("− Desktop mode"));
  expect(document.querySelector('[data-cs="hint-bar"]')?.getAttribute("data-glyphs")).toBe(
    "nintendo",
  );

  // Out of the shelf, LT/RT page the view, and Home has no tabs for LB/RB.
  act(() => screen.getByRole("link", { name: "Home" }).focus());
  await waitFor(() => expect(hints()).toContain("ZR Next page"));
  expect(hints().some((hint) => hint.includes("tab"))).toBe(false);
});

it("glyphs follow each family's layout", () => {
  expect(glyph("confirm", "nintendo")).toBe("A");
  expect(glyph("details", "nintendo")).toBe("Y");
  expect(glyph("back", "playstation")).toBe("○");
  expect(glyph("page.next", "generic")).toBe("RT");
  expect(glyph("guide", "keyboard")).toBeNull();
});

it("LT/RT page a shelf by what fits, and focus follows", async () => {
  const pad = fakePad();
  render(
    <ModeProvider initial="console">
      <InputProvider console subscribe={pad.subscribe}>
        <ShelfRow id="test" title="Shelf" icon={Gamepad}>
          {Array.from({ length: 8 }, (_, i) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: fixed list
            <div key={i} data-rect={`${i * 200} 0 200 100`}>
              <Button data-rect={`${i * 200} 0 200 100`}>Card {i}</Button>
            </div>
          ))}
        </ShelfRow>
      </InputProvider>
    </ModeProvider>,
  );
  const items = document.querySelector('[data-part="items"]') as HTMLElement;
  items.setAttribute("data-rect", "0 0 600 100");
  const card = (i: number) => screen.getByRole("button", { name: `Card ${i}` });
  act(() => card(0).focus());
  pad.press("page.next");
  expect(document.activeElement).toBe(card(3));
  pad.press("page.next");
  expect(document.activeElement).toBe(card(6));
  pad.press("page.next");
  expect(document.activeElement).toBe(card(7));
  pad.press("page.prev");
  expect(document.activeElement).toBe(card(4));
  // The pager shows in console mode.
  expect(
    Array.from(document.querySelectorAll('[data-part="pager"] kbd')).map((key) => key.textContent),
  ).toEqual(["LT", "RT"]);
});
