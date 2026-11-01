/** Age limits on play (#209): every Play asks for an admin's PIN when the
 * signed-in account's age limit holds the game back, then launches once. */
import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { createFixtureApi } from "../../api/fixtures";
import type { AgeRestriction, LaunchChoice } from "../../api/types";
import { fakeLayout, fakeViewport, focused, renderApp } from "../../test/harness";

afterEach(cleanup);

const PIN = "1111";
const TITLE = /An admin PIN is needed to play Fixture Game 0002, rated PEGI 18/;

/** Every game is rated PEGI 18 over a limit of 12; like the core, an
 * approval lets one launch of its game through. */
function ageLimit(library: ReturnType<typeof createFixtureApi>) {
  const approved = new Set<string>();
  const always = new Set<string>();
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
  library.ageRestriction = vi.fn(async (gameId) =>
    always.has(gameId) ? null : restriction(gameId),
  );
  library.approvePlay = vi.fn(
    async (gameId: string, _accountId: string, pin: string, permanent: boolean) => {
      if (pin === "9999") return { type: "locked" as const, untilMs: Date.now() + 30_000 };
      if (pin !== PIN) return { type: "wrong" as const };
      if (permanent) always.add(gameId);
      else approved.add(gameId);
      return { type: "matched" as const };
    },
  );
  library.launchGame = vi.fn(async (entryId: string, choice?: LaunchChoice) => {
    const gameId = entryId.replace("entry-", "fixture-");
    if (!always.has(gameId) && !approved.delete(gameId))
      return { type: "age_restricted" as const, restriction: await restriction(gameId) };
    return launch(entryId, choice);
  });
  // As switching accounts does: the views ask again.
  act(() => library.emit({ type: "account.changed" }));
}

const running = async (library: ReturnType<typeof createFixtureApi>) =>
  (await library.listSessions()).filter((session) => session.endedAt === null);

async function enter(pin: string) {
  const dialog = await screen.findByRole("dialog", { name: TITLE });
  fireEvent.change(within(dialog).getByLabelText(TITLE), { target: { value: pin } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Confirm" }));
  return dialog;
}

it("the game page shows the lock, and Play asks for the PIN each time", async () => {
  const app = renderApp("/games/fixture-1");
  await screen.findByRole("heading", { level: 1, name: "Fixture Game 0002" });
  ageLimit(app.library);
  await screen.findByText("Admin PIN needed");

  fireEvent.click(screen.getByRole("button", { name: "Play" }));
  await enter("4321");
  await screen.findByText("That PIN isn't right.");
  expect(app.library.launchGame).not.toHaveBeenCalled();
  expect(await running(app.library)).toHaveLength(0);

  await enter(PIN);
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  await waitFor(async () =>
    expect((await running(app.library)).map((s) => s.entryId)).toEqual(["entry-1"]),
  );
  expect(app.library.approvePlay).toHaveBeenLastCalledWith("fixture-1", "player", PIN, false);
});

it("a locked admin PIN waits, and cancelling starts nothing", async () => {
  const app = renderApp("/games/fixture-1");
  await screen.findByRole("heading", { level: 1, name: "Fixture Game 0002" });
  ageLimit(app.library);
  fireEvent.click(await screen.findByRole("button", { name: "Play" }));
  const dialog = await enter("9999");
  await screen.findByText(/Too many wrong PINs\. Try again in/);
  fireEvent.keyDown(within(dialog).getByLabelText(TITLE), { key: "Escape" });
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(app.library.launchGame).not.toHaveBeenCalled();
});

it("the game menu's Play asks too, and a launch the core refuses asks again", async () => {
  const app = renderApp("/games/fixture-1");
  const heading = await screen.findByRole("heading", { level: 1, name: "Fixture Game 0002" });
  ageLimit(app.library);
  // Play skipped the check (the core still refuses).
  app.library.ageRestriction = vi.fn(async () => null);
  fireEvent.contextMenu(heading, { clientX: 120, clientY: 80, button: 2 });
  fireEvent.click(
    await within(await screen.findByRole("menu")).findByRole("menuitem", { name: "Play" }),
  );
  await enter(PIN);
  await waitFor(async () =>
    expect((await running(app.library)).map((s) => s.entryId)).toEqual(["entry-1"]),
  );
  expect(app.library.launchGame).toHaveBeenCalledTimes(2);
});

it.each([false, true])("a pad chooses and enters the PIN (permanent: %s)", async (permanent) => {
  const undoLayout = fakeLayout();
  fakeViewport();
  try {
    const app = renderApp("/library?filter=installed", { mode: "console" });
    await waitFor(() => expect(focused()?.closest('[data-cs="game-card"]')).toBeTruthy());
    ageLimit(app.library);
    app.pad.press("details");
    await screen.findByRole("dialog");
    app.pad.press("details");
    const pad = await screen.findByRole("dialog", { name: /An admin PIN is needed to play/ });
    await waitFor(() => expect(focused()?.textContent).toBe("1"));
    if (permanent) {
      for (
        let i = 0;
        i < 8 && !focused()?.closest("label")?.textContent?.includes("Always allow");
        i++
      )
        app.pad.press("nav.up");
      expect(focused()?.closest("label")?.textContent).toContain("Always allow");
      app.pad.press("confirm");
      for (let i = 0; i < 8 && focused()?.textContent !== "1"; i++) app.pad.press("nav.down");
      expect(focused()?.textContent).toBe("1");
    }
    for (let i = 0; i < PIN.length; i++) app.pad.press("confirm");
    await waitFor(() =>
      expect((within(pad).getByLabelText(/An admin PIN/) as HTMLInputElement).value).toBe(PIN),
    );
    expect(await running(app.library)).toHaveLength(0);
    app.pad.press("menu");
    await waitFor(async () => expect(await running(app.library)).toHaveLength(1));
    expect(app.library.approvePlay).toHaveBeenLastCalledWith(
      expect.any(String),
      "player",
      PIN,
      permanent,
    );
  } finally {
    undoLayout();
  }
});

it("names the account and game and requires a valid PIN for a permanent approval", async () => {
  const app = renderApp("/games/fixture-1");
  await screen.findByRole("heading", { level: 1, name: "Fixture Game 0002" });
  ageLimit(app.library);
  fireEvent.click(screen.getByRole("button", { name: "Play" }));
  const dialog = await screen.findByRole("dialog", { name: TITLE });
  expect(within(dialog).getByText("Approve Fixture Game 0002 for Player.")).toBeTruthy();
  expect(
    (within(dialog).getByRole("radio", { name: "Unlock this time" }) as HTMLInputElement).checked,
  ).toBe(true);
  fireEvent.click(
    within(dialog).getByRole("radio", { name: "Always allow this game for this account" }),
  );
  await enter("4321");
  await screen.findByText("That PIN isn't right.");
  expect(await running(app.library)).toHaveLength(0);
  await enter(PIN);
  await waitFor(async () => expect(await running(app.library)).toHaveLength(1));
  expect(app.library.approvePlay).toHaveBeenLastCalledWith("fixture-1", "player", PIN, true);
  expect(await app.library.ageRestriction?.("fixture-1")).toBeNull();
  expect(await app.library.ageRestriction?.("fixture-2")).not.toBeNull();
});

it.each(["grid", "list"])(
  "marks locked Library search results in the %s layout",
  async (layout) => {
    fakeViewport();
    const app = renderApp(`/library?filter=all&q=0002&layout=${layout}`);
    await screen.findByText("Fixture Game 0002");
    const list = app.library.listGames.bind(app.library);
    app.library.listGames = async (query) => {
      const page = await list(query);
      return { ...page, items: page.items.map((game) => ({ ...game, adminPinNeeded: true })) };
    };
    act(() => app.library.emit({ type: "account.changed" }));
    const lock = await screen.findByText("Admin PIN needed");
    expect(
      lock.closest(layout === "grid" ? '[data-cs="game-card"]' : '[data-cs="game-row"]'),
    ).toBeTruthy();
    expect(await running(app.library)).toHaveLength(0);
  },
);

it("marks locked search suggestions without approving them", async () => {
  const app = renderApp("/library");
  await screen.findByRole("combobox", { name: "Search games" });
  const list = app.library.listGames.bind(app.library);
  app.library.listGames = async (query) => {
    const page = await list(query);
    return { ...page, items: page.items.map((game) => ({ ...game, adminPinNeeded: true })) };
  };
  act(() => screen.getByRole("combobox", { name: "Search games" }).focus());
  fireEvent.change(screen.getByRole("combobox", { name: "Search games" }), {
    target: { value: "0002" },
  });
  fireEvent.keyDown(screen.getByRole("combobox", { name: "Search games" }), { key: "ArrowDown" });
  const option = await screen.findByRole("option", { name: /Fixture Game 0002/ });
  expect(within(option).getByText("Admin PIN needed")).toBeTruthy();
  expect(await running(app.library)).toHaveLength(0);
});
