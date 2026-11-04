import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import type { SettingsApi, SettingsSnapshot } from "../../api/settings";
import { AppearanceSettings } from "./AppearanceSettings";
import { SettingsProvider } from "./useSettings";

afterEach(() => {
  cleanup();
  document.documentElement.removeAttribute("style");
});

const defaults = {
  theme: "caskade.default",
  density: "comfortable",
  cardSize: 176,
  uiScale: 100,
  showArtTitles: true,
  motion: "system",
  reduceEffects: false,
  accent: null,
};

function show(surfaces: [string, string]) {
  const root = document.documentElement;
  root.style.setProperty("--cs-color-surface-base", surfaces[0]);
  root.style.setProperty("--cs-color-surface-raised", surfaces[1]);
  let snapshot: SettingsSnapshot = {
    namespace: "core.appearance",
    version: 1,
    revision: 0,
    value: defaults,
  };
  const saved: unknown[] = [];
  const api: SettingsApi = {
    sections: async () => [],
    get: async () => structuredClone(snapshot),
    set: async (_namespace, value, revision) => {
      saved.push(value.accent);
      snapshot = { ...snapshot, revision: revision + 1, value };
      return structuredClone(snapshot);
    },
    reset: async () => snapshot,
    subscribe: async () => () => {},
  };
  render(
    <QueryClientProvider client={new QueryClient()}>
      <SettingsProvider api={api}>
        <AppearanceSettings />
      </SettingsProvider>
    </QueryClientProvider>,
  );
  return saved;
}

it("saves a readable accent, adjusting one that would vanish on the theme", async () => {
  const saved = show(["#060914", "#0d1224d9"]);
  const input = await screen.findByLabelText("Custom accent colour");
  fireEvent.change(input, { target: { value: "#e0457b" } });
  await waitFor(() => expect(saved).toEqual(["#e0457b"]));
  fireEvent.change(input, { target: { value: "#1a1f4a" } });
  await screen.findByText(/#1a1f4a was lightened to #[0-9a-f]{6} to stay readable/);
  await waitFor(() => expect(saved).toHaveLength(2));
  expect(saved[1]).not.toBe("#1a1f4a");
  fireEvent.click(screen.getByRole("button", { name: "Theme accent" }));
  await waitFor(() => expect(saved.at(-1)).toBeNull());
});

it("refuses an accent no adjustment can make readable, with a message", async () => {
  const saved = show(["#3a3a3a", "#c8c8c8"]);
  fireEvent.change(await screen.findByLabelText("Custom accent colour"), {
    target: { value: "#e0457b" },
  });
  expect((await screen.findByRole("alert")).textContent).toMatch(/can't be made readable/);
  expect(saved).toEqual([]);
});

it("offers named swatches beside the picker, and marks one adjusted for the theme", async () => {
  const saved = show(["#ffffff", "#f4f4f4"]);
  const group = await screen.findByRole("radiogroup", { name: "Accent colour" });
  const amber = within(group).getByRole("radio", { name: "Amber" });
  expect(within(group).getAllByRole("radio")).toHaveLength(8);
  expect(screen.getByLabelText("Custom accent colour")).toBeTruthy();
  fireEvent.click(amber);
  await screen.findByText(/#f59e0b was darkened to #[0-9a-f]{6} to stay readable/);
  await waitFor(() => expect(saved).toHaveLength(1));
  expect(saved[0]).not.toBe("#f59e0b");
  await waitFor(() => expect(amber.getAttribute("aria-checked")).toBe("true"));
  fireEvent.click(screen.getByRole("button", { name: "Theme accent" }));
  await waitFor(() => expect(amber.getAttribute("aria-checked")).toBe("false"));
});

it("says why a refused swatch wasn't saved", async () => {
  const saved = show(["#3a3a3a", "#c8c8c8"]);
  fireEvent.click(await screen.findByRole("radio", { name: "Red" }));
  expect((await screen.findByRole("alert")).textContent).toMatch(/can't be made readable/);
  expect(saved).toEqual([]);
});
