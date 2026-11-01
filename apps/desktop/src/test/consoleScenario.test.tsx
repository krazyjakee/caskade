/** #153's scenario with the keyboard alone, through the same action layer
 * as a pad (the console keyboard map): fast, in Vitest. The pad-only run
 * against the real app is `src-tauri/tests/console_e2e.rs`. The keyboard
 * has no guide key, so the game is stopped from its page here. */
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { fakeLayout, fakeViewport, focused, playedGames, renderApp } from "./harness";

vi.setConfig({ testTimeout: 30_000 });

let undoLayout = () => {};
beforeEach(() => {
  undoLayout = fakeLayout();
  fakeViewport();
});
afterEach(() => {
  undoLayout();
  cleanup();
});

const key = (name: string) =>
  fireEvent.keyDown(focused() ?? document.body, { key: name, bubbles: true });
const label = (element: HTMLElement | null) =>
  (element?.getAttribute("aria-label") || element?.textContent || "").replace(/\s+/g, " ");

/** Presses `name` until the focused control's text contains `text`. */
async function seek(name: string, text: string) {
  for (let i = 0; i < 60; i++) {
    if (label(focused()).includes(text)) return;
    key(name);
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  throw new Error(`never reached ${text}; focus is on ${label(focused())}`);
}

it("plays, stops, searches, changes a setting and leaves console mode with keys only", async () => {
  const app = renderApp("/", { mode: "console", games: playedGames(5) });
  await waitFor(() => expect(label(focused())).toContain("Resume"));

  // Home → the shelf (PgDn and PgUp page it) → the game → Play.
  key("ArrowDown");
  await seek("ArrowDown", "Fixture Game");
  const first = focused();
  key("PageDown");
  await waitFor(() => expect(focused()).not.toBe(first));
  key("PageUp");
  await waitFor(() => expect(focused()).toBe(first));
  key("Enter");
  await screen.findByText("Play from your library");
  await waitFor(() => expect(label(focused())).toBe("Play"));
  key("Enter");
  await screen.findByText("Playing");
  // The view fades to black for the game; Escape lifts the curtain.
  const curtain = () => document.querySelector('[data-cs="game-curtain"]');
  await waitFor(() => expect(curtain()?.getAttribute("data-state")).toBe("covered"));
  key("Escape");
  await waitFor(() => expect(curtain()?.getAttribute("data-state")).toBe("open"));
  // Play became Stop, and focus went with it.
  await waitFor(() => expect(focused()).toBe(screen.getByRole("button", { name: "Stop" })));
  key("Enter");
  await screen.findByRole("button", { name: "Play" });
  await waitFor(() => expect(focused()).toBe(screen.getByRole("button", { name: "Play" })));
  const played = (await app.library.listSessions()).filter((session) => session.endedAt);
  expect(played).toHaveLength(1);
  expect(played[0]?.durationSeconds).toBeGreaterThan(0);

  // / opens the search overlay; typing counts the games; Enter opens the
  // Library on them, on the first, and Enter opens it.
  key("/");
  const search = await screen.findByRole("textbox", { name: "Search games" });
  await waitFor(() => expect(focused()).toBe(search));
  expect(document.querySelector('[data-cs="osk"]')).toBeNull();
  fireEvent.change(search, { target: { value: "0002" } });
  await waitFor(() =>
    expect(document.querySelector('[data-cs="type-search"] [data-part="count"]')?.textContent).toBe(
      "1 game",
    ),
  );
  key("Enter");
  await waitFor(() => expect(app.router.state.location.search).toMatchObject({ q: "0002" }));
  await waitFor(() => expect(label(focused())).toContain("Fixture Game 0002"));
  key("Enter");
  await screen.findByRole("heading", { level: 1, name: "Fixture Game 0002" });

  // Settings: into the sidebar (above the content in jsdom's stacked layout),
  // down to Settings (it opens on General), a switch.
  await seek("ArrowUp", "Home");
  await seek("ArrowDown", "Settings");
  key("Enter");
  await screen.findByRole("heading", { name: "Settings" });
  await waitFor(() => expect(app.router.state.location.search).toMatchObject({ tab: "general" }));
  for (let i = 0; i < 40; i++) {
    const at = focused();
    if (at instanceof HTMLInputElement && at.closest('[data-cs="setting-row"]')) break;
    key("ArrowDown");
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  const writes = app.settings.writes.length;
  key("Enter");
  await waitFor(() => expect(app.settings.writes.length).toBeGreaterThan(writes));
  expect(app.settings.writes.at(-1)).toBe("core.general");

  // V leaves console mode.
  key("v");
  await waitFor(() => expect(document.querySelector('[data-cs="app-shell"]')).not.toBeNull());
  expect(app.console.calls).toEqual(["desktop"]);
});
