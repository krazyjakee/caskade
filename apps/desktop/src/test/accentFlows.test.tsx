/** #61: the accent colour with a pad only. */
import { cleanup, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.setConfig({ testTimeout: 30_000 });

import { fakeLayout, fakeViewport, focused, renderApp } from "./harness";

let undoLayout = () => {};
beforeEach(() => {
  undoLayout = fakeLayout();
  fakeViewport();
  // The theme's surfaces, which an accent has to stay readable on.
  document.documentElement.style.setProperty("--cs-color-surface-base", "#060914");
  document.documentElement.style.setProperty("--cs-color-surface-raised", "#0d1224d9");
});
afterEach(() => {
  undoLayout();
  cleanup();
  document.documentElement.removeAttribute("style");
});

/** Focus once a press has moved it, or null when it stayed put for 250 ms. */
async function moved(from: Element | null) {
  const until = Date.now() + 250;
  do {
    await new Promise((resolve) => setTimeout(resolve, 0));
    if (focused() !== from) return focused();
  } while (Date.now() < until);
  return null;
}

it("reaches a swatch with a pad and sets the accent, with no native picker", async () => {
  const app = renderApp("/settings?tab=appearance", { mode: "console" });
  const red = await screen.findByRole("radio", { name: "Red" });
  expect(screen.queryByLabelText("Custom accent colour")).toBeNull();

  const panel = document.querySelector('[data-settings="appearance"]');
  panel?.querySelector<HTMLElement>('[role="radio"]')?.focus();
  while (focused() !== red) {
    const at = focused();
    app.pad.press("nav.down");
    expect(await moved(at), "focus stopped before the swatch").not.toBeNull();
  }

  app.pad.press("confirm");
  await waitFor(() =>
    expect(app.settings.values["core.appearance"]).toMatchObject({ accent: "#ef4444" }),
  );
  await waitFor(() => expect(red.getAttribute("aria-checked")).toBe("true"));
});
