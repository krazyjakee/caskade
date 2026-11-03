/** #234: a game another device asks to start here is played as Play here
 * would: over the signed-in account's age limit, the admin PIN is asked for
 * here, and the device that asked hears only once it started. */
import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, expect, it, vi } from "vitest";
import type { createFixtureApi } from "../../api/fixtures";
import { SyncProvider } from "../../api/sync";
import { createSyncFixtureApi } from "../../api/syncFixtures";
import type { AgeRestriction, LaunchChoice } from "../../api/types";
import { renderApp } from "../../test/harness";

afterEach(cleanup);

const PIN = "1111";
const TITLE = /An admin PIN is needed to play Fixture Game 0002, rated PEGI 18/;

/** Every game is rated PEGI 18 over a limit of 12; an approval lets one
 * launch through, as the core does. */
function ageLimit(library: ReturnType<typeof createFixtureApi>) {
  const approved = new Set<string>();
  const launch = library.launchGame.bind(library);
  const restriction = async (gameId: string): Promise<AgeRestriction> => ({
    accountId: "player",
    accountName: "Player",
    gameId,
    title: (await library.getGame(gameId))?.title ?? gameId,
    ratings: [{ board: "pegi", rating: "18" }],
    minAge: 18,
    maxAge: 12,
  });
  library.ageRestriction = vi.fn(async (gameId) => restriction(gameId));
  library.approvePlay = vi.fn(async (gameId: string, _account: string, pin: string) => {
    if (pin !== PIN) return { type: "wrong" as const };
    approved.add(gameId);
    return { type: "matched" as const };
  });
  library.launchGame = vi.fn(async (entryId: string, choice?: LaunchChoice) => {
    const gameId = entryId.replace("entry-", "fixture-");
    if (!approved.delete(gameId))
      return { type: "age_restricted" as const, restriction: await restriction(gameId) };
    return launch(entryId, choice);
  });
  act(() => library.emit({ type: "account.changed" }));
}

it("asks for the admin PIN here before starting a game over the age limit", async () => {
  const sync = createSyncFixtureApi();
  const app = renderApp("/library", {
    wrap: (children: ReactNode) => <SyncProvider api={sync}>{children}</SyncProvider>,
  });
  await screen.findByRole("heading", { name: "Library" });
  ageLimit(app.library);

  sync.requestPlay({ requestId: "r1", gameId: "fixture-1", deviceName: "Steam Deck" });
  const dialog = await screen.findByRole("dialog", { name: TITLE });
  // Not started, and not answered, until the PIN is given here.
  expect(sync.played().finished).toEqual([]);
  fireEvent.change(within(dialog).getByLabelText(TITLE), { target: { value: PIN } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Confirm" }));
  await waitFor(() => expect(sync.played().finished).toEqual([["r1", true, undefined]]));
  expect(app.library.approvePlay).toHaveBeenCalledWith("fixture-1", "player", PIN, false);
});
