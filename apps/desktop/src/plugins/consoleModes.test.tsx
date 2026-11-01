/** #152: plugin views in console mode. */

import {
  FocusSection,
  type InputAction,
  type PluginContext,
  useFocusable,
  useInputAction,
  useMode,
} from "@caskade/sdk";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, expect, expectTypeOf, it } from "vitest";
import type { Action } from "../api/bindings";
import { InputProvider } from "../input";
import { fakeLayout, fakePad, fakeViewport, focused, renderApp } from "../test/harness";
import { PluginBoundary } from "./PluginBoundary";
import { PluginRegistry } from "./registry";
import type { UiPlugin } from "./types";

let undoLayout = () => {};
beforeEach(() => {
  undoLayout = fakeLayout();
  fakeViewport();
});
afterEach(() => {
  undoLayout();
  cleanup();
});

it("the SDK's pad actions are the core's", () => {
  expectTypeOf<InputAction>().toEqualTypeOf<Action>();
});

/** A plugin's own widget: a focusable tile, a pad binding and the mode. */
function Couch() {
  const [count, setCount] = useState(0);
  const tile = useFocusable<HTMLDivElement>({ focusKey: "couch-tile" });
  const mode = useMode();
  useInputAction("details", () => setCount((n) => n + 10), { label: "Ten more" });
  return (
    <FocusSection id="couch">
      <p>Mode: {mode}</p>
      <button type="button" onClick={() => setCount((n) => n + 1)}>
        Add one
      </button>
      <div ref={tile.ref} role="note" data-focused={tile.focused || undefined}>
        Count {count}
      </div>
    </FocusSection>
  );
}

function registry() {
  const plugin: UiPlugin = {
    id: "dev.fixture.couch",
    name: "Couch",
    version: "0.1.0",
    entry: "caskade-plugin://dev.fixture.couch/ui.js",
    contributes: [
      { slot: "views", id: "couch", title: "Couch", modes: ["console"] },
      { slot: "views", id: "desk", title: "Desk", modes: ["desktop"] },
      { slot: "views", id: "both", title: "Both" },
      { slot: "topbar.items", id: "clock" },
    ],
  };
  const context = { plugin: { id: plugin.id, name: plugin.name } } as unknown as PluginContext;
  const registry = new PluginRegistry();
  registry.register(plugin, "views", { id: "couch", title: "Couch", component: Couch }, context);
  registry.register(plugin, "views", { id: "desk", title: "Desk", component: () => <p>Desk</p> });
  registry.register(plugin, "views", { id: "both", title: "Both", component: () => <p>Both</p> });
  registry.register(plugin, "topbar.items", { id: "clock", component: () => <span>12:00</span> });
  return registry;
}

it("a console view shows in the console sidebar and works by pad; a desktop view doesn't", async () => {
  const app = renderApp("/settings", { mode: "console", registry: registry() });
  const nav = await screen.findByRole("navigation", { name: "Plugins" });
  expect(
    within(nav)
      .getAllByRole("link")
      .map((link) => link.textContent),
  ).toEqual(["Couch", "Both"]);
  // Desktop-only slots stay out of console mode.
  expect(screen.queryByText("12:00")).toBeNull();

  act(() => within(nav).getByRole("link", { name: "Couch" }).focus());
  app.pad.press("confirm");
  await screen.findByText("Mode: console");
  // Into the view: its button, pressed by confirm.
  app.pad.press("nav.right");
  await waitFor(() => expect(focused()?.textContent).toBe("Add one"));
  app.pad.press("confirm");
  await screen.findByText("Count 1");
  // Its own focusable tile, and its binding (labelled in the hint bar).
  app.pad.press("nav.down");
  await waitFor(() => expect(focused()?.dataset.focusKey).toBe("couch-tile"));
  expect(focused()?.dataset.focused).toBe("true");
  expect(document.querySelector('[data-cs="hint-bar"]')?.textContent).toContain("Ten more");
  app.pad.press("details");
  await screen.findByText("Count 11");

  // A link into the desktop-only view says why it's empty.
  await act(() =>
    app.router.navigate({
      to: "/plugins/$pluginId/$viewId",
      params: { pluginId: "dev.fixture.couch", viewId: "desk" },
    }),
  );
  await screen.findByText("Desk isn't available in console mode.");
});

it("on the desktop, the console view is hidden and the desktop one shows", async () => {
  renderApp("/settings", { registry: registry() });
  const nav = await screen.findByRole("navigation", { name: "Plugins" });
  expect(
    within(nav)
      .getAllByRole("link")
      .map((link) => link.textContent),
  ).toEqual(["Desk", "Both"]);
  expect(screen.getByText("12:00")).toBeTruthy();
});

/** Throws on the render after its button is pressed. */
function Fragile() {
  const [broken, setBroken] = useState(false);
  if (broken) throw new Error("boom");
  return (
    <button type="button" onClick={() => setBroken(true)}>
      Break
    </button>
  );
}

it("a plugin that crashes under focus leaves it on its placeholder, and a pad moves on", async () => {
  const pad = fakePad();
  render(
    <InputProvider console subscribe={pad.subscribe}>
      <button type="button">Before</button>
      <PluginBoundary plugin={{ id: "dev.fixture.fragile", name: "Fragile" }}>
        <Fragile />
      </PluginBoundary>
    </InputProvider>,
  );
  const button = screen.getByRole("button", { name: "Break" });
  act(() => button.focus());
  act(() => button.click());
  const placeholder = await screen.findByText("Fragile couldn't show this part.");
  expect(focused()).toBe(placeholder);
  pad.press("nav.up");
  await waitFor(() => expect(focused()?.textContent).toBe("Before"));
});
