import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { renderApp } from "../test/harness";

// The collection's games are a virtualized grid: give it a size.
beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(900);
  vi.stubGlobal("scrollTo", vi.fn());
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe(target: Element) {
        this.callback([
          {
            target,
            contentRect: { width: 900, height: 600 },
            borderBoxSize: [{ inlineSize: 900, blockSize: 600 }],
          },
        ]);
      }
      unobserve() {}
      disconnect() {}
      constructor(private callback: (entries: unknown[]) => void) {}
    },
  );
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const cardTitles = () =>
  Array.from(document.querySelectorAll('[data-cs="collection-card"] [data-part="title"]')).map(
    (title) => title.textContent,
  );

it("lists collections pinned first, and opens one with its games", async () => {
  const app = renderApp("/collections");
  await waitFor(() => expect(cardTitles()).toEqual(["Favourites", "Weekend Picks"]));
  const favourites = document.querySelector('[data-cs="collection-card"][data-kind="smart"]');
  expect(favourites?.querySelector('[data-part="pin"]')).not.toBeNull();

  fireEvent.click(screen.getByRole("tab", { name: /Manual/ }));
  await waitFor(() => expect(cardTitles()).toEqual(["Weekend Picks"]));

  fireEvent.click(screen.getByRole("link", { name: /Weekend Picks/ }));
  await screen.findByRole("heading", { name: "Weekend Picks", level: 1 });
  expect(app.router.state.location.pathname).toBe("/collections/collection-weekend");
  await screen.findByRole("heading", { name: "Games (3)" });
  await screen.findAllByText("Fixture Game 0002");
});

it("makes a manual collection and adds games to it", async () => {
  const app = renderApp("/collections");
  await waitFor(() => expect(cardTitles()).toHaveLength(2));
  fireEvent.click(screen.getAllByRole("button", { name: /New Collection/ })[0] as HTMLElement);
  const dialog = await screen.findByRole("dialog");
  fireEvent.change(within(dialog).getByRole("textbox", { name: "Name" }), {
    target: { value: "Couch Co-op" },
  });
  fireEvent.click(within(dialog).getByRole("button", { name: "Create" }));

  await screen.findByRole("heading", { name: "Couch Co-op", level: 1 });
  expect(app.router.state.location.pathname).toMatch(/^\/collections\/collection-/);
  await screen.findByText(/This collection is empty/);

  fireEvent.click(screen.getByRole("button", { name: /Add Games/ }));
  const picker = await screen.findByRole("dialog");
  fireEvent.click(await within(picker).findByRole("checkbox", { name: /Fixture Game 0001/ }));
  await waitFor(() =>
    expect(within(picker).getByRole("checkbox", { name: /Fixture Game 0001/ })).toHaveProperty(
      "checked",
      true,
    ),
  );
  fireEvent.click(within(picker).getByRole("button", { name: "Done" }));
  await screen.findByRole("heading", { name: "Games (1)" });
});

it("saves the library's view as a smart collection", async () => {
  // Six of the eight fixture games are installed.
  renderApp("/library?filter=installed&sort=title&layout=grid");
  fireEvent.click(await screen.findByRole("button", { name: /Save as collection/ }));
  const dialog = await screen.findByRole("dialog");
  await within(dialog).findByText("6 games match now");
  fireEvent.change(within(dialog).getByRole("textbox", { name: "Name" }), {
    target: { value: "Ready to Play" },
  });
  fireEvent.click(within(dialog).getByRole("button", { name: "Create" }));

  await screen.findByRole("heading", { name: "Ready to Play", level: 1 });
  const rules = document.querySelector('[data-part="rules"]') as HTMLElement;
  expect(within(rules).getByText("Installed")).toBeTruthy();
  await screen.findByRole("heading", { name: "Games (6)" });
});

it("puts a game in a collection from its menu", async () => {
  const app = renderApp("/library?filter=all&sort=title&layout=grid");
  const [card] = await screen.findAllByRole("link", { name: /Fixture Game 0001/ });
  fireEvent.contextMenu(card as HTMLElement, { clientX: 10, clientY: 10 });
  fireEvent.click(await screen.findByRole("menuitem", { name: /Add to collection/ }));
  const picker = await screen.findByRole("dialog");
  fireEvent.click(await within(picker).findByRole("checkbox", { name: "Weekend Picks" }));
  await waitFor(async () =>
    expect(await app.library.collections?.ofGame("fixture-0")).toEqual(["collection-weekend"]),
  );

  // A new collection from the picker starts with the game in it.
  fireEvent.change(within(picker).getByRole("textbox", { name: "New collection" }), {
    target: { value: "Short" },
  });
  fireEvent.click(within(picker).getByRole("button", { name: "Create" }));
  await within(picker).findByRole("checkbox", { name: "Short" });
  await waitFor(async () =>
    expect(await app.library.collections?.ofGame("fixture-0")).toHaveLength(2),
  );
});

it("deletes a collection from its menu", async () => {
  const app = renderApp("/collections/collection-weekend");
  await screen.findByRole("heading", { name: "Weekend Picks", level: 1 });
  fireEvent.click(screen.getByRole("button", { name: "More" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: /Delete/ }));
  const confirm = await screen.findByRole("alertdialog");
  fireEvent.click(within(confirm).getByRole("button", { name: "Delete" }));
  await waitFor(() => expect(app.router.state.location.pathname).toBe("/collections"));
  await waitFor(() => expect(cardTitles()).toEqual(["Favourites"]));
});

it("plays a random installed game from the collection", async () => {
  const app = renderApp("/collections/collection-weekend");
  await screen.findByRole("heading", { name: "Games (3)" });
  fireEvent.click(screen.getByRole("button", { name: /Play Random/ }));
  await waitFor(async () => {
    const sessions = await app.library.listSessions();
    expect(sessions).toHaveLength(1);
    expect(["fixture-1", "fixture-2", "fixture-3"]).toContain(sessions[0]?.gameId);
  });
});
