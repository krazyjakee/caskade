/** Desktop mode's game summary panel (#196): a click on a game opens its
 * quick look beside the view instead of its page; console mode keeps the
 * page and the pad's X. */
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { fakeViewport, renderApp } from "../../test/harness";

beforeEach(() => {
  fakeViewport();
  vi.stubGlobal("scrollTo", vi.fn());
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

async function card(name: RegExp) {
  const [link] = await screen.findAllByRole("link", { name });
  if (!link) throw new Error("card missing");
  return link;
}

const focusedCard = () =>
  document.activeElement?.closest('[data-cs="game-card"]')?.querySelector('[data-part="title"]')
    ?.textContent;

it("opens a card's summary beside the library, one panel at a time, and gives focus back", async () => {
  const app = renderApp("/library?filter=all&sort=title");
  fireEvent.click(await card(/Fixture Game 0002/));
  const panel = await screen.findByRole("dialog", { name: "Fixture Game 0002" });
  expect(panel.dataset.cs).toBe("quick-look");
  // The library stays; nothing starts.
  expect(app.router.state.location.pathname).toBe("/library");
  expect(screen.getByRole("heading", { level: 1, name: "Library" })).toBeTruthy();
  const actions = panel.querySelector('[data-part="actions"]');
  expect(Array.from(actions?.querySelectorAll("button") ?? [], (b) => b.textContent)).toEqual([
    "Play",
    "View more",
  ]);
  expect((await app.library.listSessions()).filter((s) => s.endedAt === null)).toHaveLength(0);

  // Another game shows in the same panel.
  fireEvent.click(await card(/Fixture Game 0003/));
  await screen.findByRole("dialog", { name: "Fixture Game 0003" });
  expect(screen.getAllByRole("dialog")).toHaveLength(1);

  // Escape closes it, back on the card.
  fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(focusedCard()).toBe("Fixture Game 0003");

  // So does Close.
  fireEvent.click(await card(/Fixture Game 0002/));
  const again = await screen.findByRole("dialog", { name: "Fixture Game 0002" });
  fireEvent.click(within(again).getByRole("button", { name: "Close" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(focusedCard()).toBe("Fixture Game 0002");
});

it("closes on a click elsewhere in the view, but not on another game", async () => {
  renderApp("/library?filter=all&sort=title");
  const press = (at: Element) => {
    fireEvent.pointerDown(at, { button: 0 });
    fireEvent.click(at);
  };
  fireEvent.click(await card(/Fixture Game 0002/));
  const panel = await screen.findByRole("dialog", { name: "Fixture Game 0002" });

  // Inside the panel it stays.
  press(within(panel).getByRole("heading", { name: "Fixture Game 0002" }));
  await new Promise((resolve) => setTimeout(resolve, 10));
  expect(screen.getByRole("dialog", { name: "Fixture Game 0002" })).toBeTruthy();

  // Another game shows in it.
  press(await card(/Fixture Game 0003/));
  await screen.findByRole("dialog", { name: "Fixture Game 0003" });
  await new Promise((resolve) => setTimeout(resolve, 10));
  expect(screen.getByRole("dialog", { name: "Fixture Game 0003" })).toBeTruthy();

  // Anywhere else closes it, leaving focus alone.
  const heading = screen.getByRole("heading", { level: 1, name: "Library" });
  press(heading);
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(focusedCard()).toBeUndefined();
});

it("plays once from the panel", async () => {
  const app = renderApp("/library?filter=all&sort=title");
  fireEvent.click(await card(/Fixture Game 0002/));
  const panel = await screen.findByRole("dialog", { name: "Fixture Game 0002" });
  fireEvent.click(within(panel).getByRole("button", { name: "Play" }));
  await waitFor(async () => {
    const running = (await app.library.listSessions()).filter((s) => s.endedAt === null);
    expect(running.map((s) => s.entryId)).toEqual(["entry-1"]);
  });
  expect(screen.queryByRole("dialog", { name: "Fixture Game 0002" })).toBeNull();
});

it("View more opens the page, whose Back returns to the library", async () => {
  const app = renderApp("/library?filter=all&sort=title");
  fireEvent.click(await card(/Fixture Game 0004/));
  const panel = await screen.findByRole("dialog", { name: "Fixture Game 0004" });
  fireEvent.click(within(panel).getByRole("button", { name: "View more" }));
  await screen.findByRole("heading", { level: 1, name: "Fixture Game 0004" });
  expect(screen.queryByRole("dialog")).toBeNull();
  fireEvent.click(await screen.findByRole("link", { name: "Back to Library" }));
  await screen.findByRole("heading", { level: 1, name: "Library" });
  expect(app.router.state.location.search).toMatchObject({ filter: "all", sort: "title" });
});

// The pad's X and its modal quick look: test/padFlows.test.tsx.
it("opens the page in console mode", async () => {
  const app = renderApp("/library?filter=all&sort=title", { mode: "console" });
  fireEvent.click(await card(/Fixture Game 0002/));
  await screen.findByRole("heading", { level: 1, name: "Fixture Game 0002" });
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(app.router.state.location.search).toMatchObject({
    from: expect.stringContaining("/library"),
  });
});
