import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createFixtureApi, createFixtureGames } from "../../api/fixtures";
import { createInstallFixtureApi } from "../../api/installFixtures";
import { createLibraryQueryClient, LibraryProvider } from "../../api/provider";
import type { LibraryLayout, LibrarySort } from "../../api/types";
import { ActivityProvider } from "../activity/ActivityProvider";
import { LibraryView } from "./LibraryView";

let scrollTo: ReturnType<typeof vi.fn>;
beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(900);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(900);
  // Room to scroll: the virtualizer clamps jumps to it.
  vi.spyOn(Element.prototype, "scrollHeight", "get").mockReturnValue(100_000);
  scrollTo = vi.fn();
  vi.stubGlobal("scrollTo", vi.fn());
  Element.prototype.scrollTo = scrollTo as unknown as typeof Element.prototype.scrollTo;
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

/** Four games under `#`, then twenty for each of A to L. */
function titledGames(count = 244) {
  return createFixtureGames(count).map((game, i) => ({
    ...game,
    title:
      i < 4
        ? `${1940 + i} Fixture`
        : `${"ABCDEFGHIJKL"[Math.floor((i - 4) / 20)]} Fixture ${String(i).padStart(3, "0")}`,
  }));
}

function renderLibrary({
  sort = "title",
  layout = "grid",
  games = titledGames(),
}: {
  sort?: LibrarySort;
  layout?: LibraryLayout;
  games?: ReturnType<typeof titledGames>;
} = {}) {
  const root = createRootRoute({ component: Outlet });
  const route = createRoute({
    getParentRoute: () => root,
    path: "/library",
    component: () => (
      <ActivityProvider api={createInstallFixtureApi()}>
        <LibraryView filter="all" sort={sort} layout={layout} />
      </ActivityProvider>
    ),
  });
  const router = createRouter({
    routeTree: root.addChildren([route]),
    history: createMemoryHistory({ initialEntries: ["/library"] }),
  });
  return render(
    <LibraryProvider api={createFixtureApi(games)} client={createLibraryQueryClient()}>
      <RouterProvider router={router} />
    </LibraryProvider>,
  );
}

it("disables letters no title starts with, and jumps to a letter's first row", async () => {
  renderLibrary();
  await screen.findByRole("toolbar", { name: "Jump to letter" });
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "C" })).toHaveProperty("disabled", false),
  );
  expect(screen.getByRole("button", { name: "Numbers and symbols" })).toHaveProperty(
    "disabled",
    false,
  );
  expect(screen.getByRole("button", { name: "M" })).toHaveProperty("disabled", true);
  expect(
    screen.getByRole("button", { name: "Numbers and symbols" }).getAttribute("aria-current"),
  ).toBe("true");
  fireEvent.click(screen.getByRole("button", { name: "C" }));
  // C starts at game 44: row 11 of a four-column grid of 312px rows.
  await waitFor(() =>
    expect(scrollTo).toHaveBeenCalledWith(expect.objectContaining({ top: 11 * 312 })),
  );
});

it("puts a jump in the list below its sticky header, and follows the scrolling", async () => {
  const result = renderLibrary({ layout: "list" });
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "C" })).toHaveProperty("disabled", false),
  );
  fireEvent.click(screen.getByRole("button", { name: "C" }));
  // Row 44 of 44px rows, under the 32px header.
  await waitFor(() =>
    expect(scrollTo).toHaveBeenCalledWith(expect.objectContaining({ top: 44 * 44 })),
  );
  const viewport = result.container.querySelector('[data-cs="virtual-library"]');
  if (!viewport) throw new Error("no viewport");
  Object.defineProperty(viewport, "scrollTop", { configurable: true, value: 44 * 44 });
  fireEvent.scroll(viewport);
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "C" }).getAttribute("aria-current")).toBe("true"),
  );
});

it("only shows for a long library sorted by title", async () => {
  const first = renderLibrary({ sort: "recent" });
  await screen.findAllByText("1940 Fixture");
  expect(screen.queryByRole("toolbar", { name: "Jump to letter" })).toBeNull();
  first.unmount();
  renderLibrary({ games: titledGames(30) });
  await screen.findAllByText("1940 Fixture");
  expect(screen.queryByRole("toolbar", { name: "Jump to letter" })).toBeNull();
});
