/** #150: Home, Library and game details with a pad only. Every step is a
 * pad action; the only other input is the text an on-screen keyboard would
 * type into the search. */
import { cleanup, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it } from "vitest";
import { fakeLayout, fakeViewport, focused, playedGames, renderApp, until } from "./harness";

let undoLayout = () => {};
beforeEach(() => {
  undoLayout = fakeLayout();
  fakeViewport();
});
afterEach(() => {
  undoLayout();
  cleanup();
});

it("Home → a shelf → game details → Play starts the game", async () => {
  // Every fifth fixture game isn't installed; the second newest here, the
  // shelf's first (the newest is the hero's), is.
  const app = renderApp("/", { mode: "console", games: playedGames(6) });
  await until(() => /Resume/.test(focused()?.textContent ?? ""), "the hero");

  // Down into Recently Played, past its View all, onto the first card.
  app.pad.press("nav.down");
  await until(() => focused()?.dataset.part === "view-all", "View all");
  app.pad.press("nav.down");
  await until(() => Boolean(focused()?.closest('[data-cs="game-card"]')), "a card");
  // RT pages along the shelf; the first card is the most recently played after the hero's.
  const first = focused();
  app.pad.press("page.next");
  await until(
    () => focused() !== first && Boolean(focused()?.closest('[data-cs="shelf-row"]')),
    "the next page",
  );
  app.pad.press("page.prev");
  await until(() => focused() === first, "the first card again");

  app.pad.press("confirm");
  await screen.findByRole("heading", { level: 1, name: "Fixture Game 0005" });
  // The page opens on Play.
  await until(() => focused()?.textContent === "Play", "Play");
  app.pad.press("confirm");
  await screen.findByText("Playing");
  const sessions = await app.library.listSessions("fixture-4");
  expect(sessions).toHaveLength(1);
  expect(sessions[0]?.endedAt).toBeNull();
  // And Stop is where Play was.
  await until(() => focused()?.textContent?.includes("Stop") ?? false, "Stop");
});

it("Library → RB to a filter → the grid → X's quick look → the game → back to the card", async () => {
  const app = renderApp("/library", { mode: "console", games: playedGames(8) });
  await screen.findByRole("heading", { name: "Library" });
  // The library opens on its first game.
  await until(() => Boolean(focused()?.closest('[data-cs="game-card"]')), "the first game");

  // RB: the next filter, in place of this one.
  const entries = app.router.history.length;
  app.pad.press("tab.next");
  await waitFor(() =>
    expect(app.router.state.location.search).toMatchObject({ filter: "installed" }),
  );
  expect(app.router.history.length).toBe(entries);

  // Down into the grid; the scroll box isn't a stop.
  for (let i = 0; i < 12 && !focused()?.closest('[data-cs="game-card"]'); i++) {
    expect(focused()?.dataset.cs).not.toBe("virtual-library");
    app.pad.press("nav.down");
    await waitFor(() => expect(focused()).not.toBeNull());
  }
  const card = focused()?.closest<HTMLElement>('[data-cs="game-card"]');
  expect(card).toBeTruthy();
  const title = card?.textContent ?? "";

  // X: the quick look, on Play; B closes it, back on the card.
  app.pad.press("details");
  const look = await screen.findByRole("dialog");
  expect(look.dataset.cs).toBe("quick-look");
  await until(() => focused()?.textContent === "Play", "Play");
  app.pad.press("back");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  await until(() => focused()?.closest('[data-cs="game-card"]')?.textContent === title, "the card");

  // A opens the game; B comes back to the same card.
  app.pad.press("confirm");
  await waitFor(() => expect(app.router.state.location.pathname).toMatch(/^\/games\//));
  const heading = await screen.findByRole("heading", { level: 1, name: /Fixture Game/ });
  expect(title).toContain(heading.textContent ?? "-");
  app.pad.press("back");
  await screen.findByRole("heading", { name: "Library" });
  await until(
    () => focused()?.closest('[data-cs="game-card"]')?.textContent === title,
    "the card it came from",
  );
});

it("X twice on a card plays the game", async () => {
  const app = renderApp("/library?filter=installed", { mode: "console", games: playedGames(8) });
  await until(() => Boolean(focused()?.closest('[data-cs="game-card"]')), "the first game");
  app.pad.press("details");
  await screen.findByRole("dialog");
  app.pad.press("details");
  await screen.findByText("Playing", undefined, { timeout: 3000 }).catch(() => null);
  await waitFor(async () => {
    const running = (await app.library.listSessions()).filter((s) => s.endedAt === null);
    expect(running).toHaveLength(1);
  });
});

it("Y goes to the search; a suggestion opens its game", async () => {
  const app = renderApp("/", { mode: "console", games: playedGames(6) });
  await until(() => /Resume/.test(focused()?.textContent ?? ""), "the hero");
  app.pad.press("search");
  const search = screen.getByRole("combobox", { name: "Search games" });
  await until(() => focused() === search, "the search box");
  // The on-screen keyboard opens and types: up from q is 1, along to 0,
  // three times, then back to 3. B closes it.
  await until(() => Boolean(document.querySelector('[data-cs="osk"]')), "the keyboard");
  app.pad.press("nav.up");
  for (let i = 0; i < 9; i++) app.pad.press("nav.right");
  for (let i = 0; i < 3; i++) app.pad.press("confirm");
  for (let i = 0; i < 7; i++) app.pad.press("nav.left");
  app.pad.press("confirm");
  expect(search).toHaveProperty("value", "0003");
  app.pad.press("back");
  expect(document.querySelector('[data-cs="osk"]')).toBeNull();
  // Down opens the suggestions and moves through them; focus stays in the box.
  const highlighted = () => {
    const active = search.getAttribute("aria-activedescendant");
    return (active && document.getElementById(active)?.textContent) || "";
  };
  // Suggestions load once the typing pauses.
  await new Promise((resolve) => setTimeout(resolve, 250));
  const seen: string[] = [];
  for (let i = 0; i < 4 && !/Fixture Game 0003/.test(highlighted()); i++) {
    const before = highlighted();
    app.pad.press("nav.down");
    await waitFor(() => expect(highlighted()).not.toBe(before));
    seen.push(highlighted());
  }
  expect(highlighted(), seen.join(" | ")).toMatch(/Fixture Game 0003/);
  expect(focused()).toBe(search);
  app.pad.press("confirm");
  await screen.findByRole("heading", { level: 1, name: "Fixture Game 0003" });
});

it("in French the search's keyboard is AZERTY, and a pad types é and ç", async () => {
  const app = renderApp("/", { mode: "console", games: playedGames(3), locale: "fr" });
  await until(() => /Resume/.test(focused()?.textContent ?? ""), "the hero");
  app.pad.press("search");
  const search = screen.getByRole("combobox", { name: "Search games" });
  await until(() => Boolean(document.querySelector('[data-cs="osk"]')), "the keyboard");
  const row = document.querySelectorAll('[data-cs="osk"] [data-part="row"]')[1];
  const keys = Array.from(row?.querySelectorAll('[data-part="key"]') ?? [], (key) =>
    key.getAttribute("data-key"),
  );
  expect(keys.join("")).toBe("azertyuiopéè");
  // From a, left wraps round to è, then é; down to à, right to ç.
  app.pad.press("nav.left");
  app.pad.press("nav.left");
  app.pad.press("confirm");
  app.pad.press("nav.down");
  app.pad.press("nav.right");
  app.pad.press("confirm");
  expect(search).toHaveProperty("value", "éç");
});
