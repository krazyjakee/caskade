/** #248: what else can be shared once through a link, from where it lives:
 * a save version from the game's Saves panel, a collection from its
 * menu, and a web game's offline copy or a ROM from the game's menu, each
 * as that kind of share, not the folder it's in. */
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { SharesProvider, type ShareTarget } from "../api/shares";
import { createSharesFixtureApi } from "../api/sharesFixtures";
import type { GameDetails } from "../api/types";
import { playedGames, renderApp } from "./harness";

vi.setConfig({ testTimeout: 20_000 });

afterEach(cleanup);

function open(path: string, games?: GameDetails[]) {
  const api = createSharesFixtureApi();
  const shared: ShareTarget[] = [];
  const share = api.share.bind(api);
  api.share = (target, expiry) => {
    shared.push(target);
    return share(target, expiry);
  };
  const app = renderApp(path, {
    games,
    wrap: (children) => <SharesProvider api={api}>{children}</SharesProvider>,
  });
  return { app, shared };
}

/** Makes the link in the open share dialog, titled `title`. */
async function makeLink(title: string) {
  const dialog = await screen.findByRole("dialog", { name: title });
  fireEvent.click(within(dialog).getByRole("button", { name: "Make a link" }));
  await within(dialog).findByRole("button", { name: /Copy/ });
}

it("shares a save version from the game's Saves panel", async () => {
  const { app, shared } = open("/games/fixture-1");
  app.library.saves.seedHistory("fixture-1", {
    accountId: "",
    syncing: true,
    conflict: [],
    versions: [
      {
        current: true,
        complete: true,
        version: {
          id: "v1",
          gameId: "fixture-1",
          accountId: "",
          deviceId: "local",
          sessionId: null,
          createdAtMs: Date.UTC(2026, 10, 2, 20),
          contentHash: "sha256:00",
          sizeBytes: 4096,
          fileCount: 2,
          parents: [],
          reason: "session",
          local: true,
        },
      },
    ],
  });
  fireEvent.click(await screen.findByRole("button", { name: "Share this save with someone…" }));
  await makeLink("Share a save of Fixture Game 0002");
  expect(shared).toEqual([
    {
      kind: "item",
      item: { kind: "save", versionId: "v1" },
      title: "a save of Fixture Game 0002",
    },
  ]);
});

it("shares a collection as a list from its menu", async () => {
  const { shared } = open("/collections/collection-favourites");
  fireEvent.click(await screen.findByRole("button", { name: "More" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "Share with someone…" }));
  const dialog = await screen.findByRole("dialog", { name: "Share Favourites" });
  expect(within(dialog).getByText(/Nothing is downloaded/)).toBeTruthy();
  await makeLink("Share Favourites");
  expect(shared).toEqual([
    {
      kind: "item",
      item: { kind: "collection", collectionId: "collection-favourites" },
      title: "Favourites",
    },
  ]);
});

/** A game whose only release is `entry`, installed in a folder. */
function withEntry(entry: Partial<GameDetails["entries"][number]>): GameDetails[] {
  return playedGames(2).map((game, index) =>
    index === 1
      ? {
          ...game,
          entries: [
            {
              id: "entry-x",
              source: "manual",
              state: "installed",
              location: "/Games/Thing",
              canLaunch: true,
              emulated: false,
              ...entry,
            },
          ],
        }
      : game,
  );
}

async function shareFromMenu() {
  const hero = (await screen.findByRole("heading", { level: 1 })).closest(
    '[data-cs="hero"]',
  ) as HTMLElement;
  fireEvent.contextMenu(hero);
  const menu = await screen.findByRole("menu");
  fireEvent.click(within(menu).getByRole("menuitem", { name: /Share with someone…/ }));
}

it("a web game's menu shares its offline copy, and an emulated game's its ROM", async () => {
  let { shared } = open("/games/fixture-1", withEntry({ source: "web" }));
  await shareFromMenu();
  await waitFor(() => screen.getByRole("dialog"));
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Make a link" }));
  await waitFor(() => expect(shared).toHaveLength(1));
  expect(shared[0]).toMatchObject({ kind: "item", item: { kind: "web", gameId: "fixture-1" } });
  cleanup();

  ({ shared } = open("/games/fixture-1", withEntry({ emulated: true })));
  await shareFromMenu();
  await waitFor(() => screen.getByRole("dialog"));
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Make a link" }));
  await waitFor(() => expect(shared).toHaveLength(1));
  expect(shared[0]).toMatchObject({ kind: "item", item: { kind: "rom", entryId: "entry-x" } });
  cleanup();

  // Any other game shares its folder, as before.
  ({ shared } = open("/games/fixture-1", withEntry({})));
  await shareFromMenu();
  await waitFor(() => screen.getByRole("dialog"));
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Make a link" }));
  await waitFor(() => expect(shared).toHaveLength(1));
  expect(shared[0]).toMatchObject({ kind: "game", entryId: "entry-x" });
});
