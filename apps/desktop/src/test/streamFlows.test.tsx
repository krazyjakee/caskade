/** #264: Stream from another device, from the game page's Play, the game
 * menu and with a pad in console mode; a busy device asks before its game
 * is stopped; a stream ends from the game page and the guide menu. */
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.setConfig({ testTimeout: 30_000 });

import { createFixtureGames } from "../api/fixtures";
import { type StreamHost, StreamingApiProvider } from "../api/streaming";
import { createStreamingFixtureApi } from "../api/streamingFixtures";
import type { GameDetails } from "../api/types";
import type { Action } from "../input/actions";
import { type GuideApi, GuideMenu } from "../input/GuideMenu";
import { InputProvider, type PadAction } from "../input/InputProvider";
import { fakeLayout, fakeViewport, focused, renderApp } from "./harness";

let undoLayout = () => {};
beforeEach(() => {
  undoLayout = fakeLayout();
  fakeViewport();
});
afterEach(() => {
  undoLayout();
  cleanup();
});

const den: StreamHost = {
  id: "den-host",
  name: "Living room",
  address: "192.168.1.20",
  reach: "ready",
  paired: true,
  games: 2,
};

/** fixture-1 is installed here and offered by the living room's device;
 * fixture-2 only streams from it. */
function games(): GameDetails[] {
  const all = createFixtureGames(4);
  const stream = (id: string) => ({
    id,
    source: "stream:den-host",
    state: "partial",
    location: null,
    canLaunch: true,
    emulated: false,
  });
  const both = all[1] as GameDetails;
  both.entries = [...both.entries, stream("stream-1")];
  const only = all[2] as GameDetails;
  only.entries = [stream("stream-2")];
  return all;
}

function open(path: string, mode: "desktop" | "console" = "desktop", host: StreamHost = den) {
  return renderApp(path, {
    mode,
    games: games(),
    wrap: (app) => (
      <StreamingApiProvider api={createStreamingFixtureApi({ hosts: [host] })}>
        {app}
      </StreamingApiProvider>
    ),
  });
}

async function running(app: ReturnType<typeof open>, gameId: string) {
  return (await app.library.listSessions(gameId)).filter((session) => session.endedAt === null);
}

it("Play streams a game only another device has, and the stream ends from the page", async () => {
  const app = open("/games/fixture-2");
  const play = await screen.findByRole("button", { name: "Stream from Living room" });
  fireEvent.click(play);
  const end = await screen.findByRole("button", { name: "End stream" });
  expect(screen.getByText("Streaming")).toBeTruthy();
  expect((await running(app, "fixture-2")).map((session) => session.entryId)).toEqual(["stream-2"]);
  fireEvent.click(end);
  await waitFor(async () => expect(await running(app, "fixture-2")).toEqual([]));
});

it("a busy device asks before its game is stopped, and Cancel streams nothing", async () => {
  const app = open("/games/fixture-2");
  app.library.busyHosts.set("stream-2", { device: "Living room", playing: "Hades" });
  fireEvent.click(await screen.findByRole("button", { name: "Stream from Living room" }));
  let dialog = await screen.findByRole("dialog", { name: "Stop the other game?" });
  expect(dialog.textContent).toContain(
    "Living room is playing Hades. Stop it and stream Fixture Game 0003?",
  );
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(await running(app, "fixture-2")).toEqual([]);

  fireEvent.click(screen.getByRole("button", { name: "Stream from Living room" }));
  dialog = await screen.findByRole("dialog", { name: "Stop the other game?" });
  fireEvent.click(within(dialog).getByRole("button", { name: "Stop and stream" }));
  await screen.findByRole("button", { name: "End stream" });
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect((await running(app, "fixture-2")).map((session) => session.entryId)).toEqual(["stream-2"]);
});

it("the game menu offers Stream from beside Play", async () => {
  const app = open("/games/fixture-1");
  const hero = (
    await screen.findByRole("heading", { level: 1, name: "Fixture Game 0002" })
  ).closest('[data-cs="hero"]') as HTMLElement;
  fireEvent.contextMenu(hero);
  const menu = await screen.findByRole("menu");
  const items = within(menu)
    .getAllByRole("menuitem")
    .map((item) => item.textContent ?? "");
  expect(items.some((item) => item.startsWith("Play"))).toBe(true);
  fireEvent.click(within(menu).getByRole("menuitem", { name: /Stream from Living room/ }));
  await waitFor(async () =>
    expect((await running(app, "fixture-1")).map((session) => session.entryId)).toEqual([
      "stream-1",
    ]),
  );
  // Playing, the menu ends the stream.
  fireEvent.contextMenu(hero);
  expect(
    await within(await screen.findByRole("menu")).findByRole("menuitem", { name: /End stream/ }),
  ).toBeTruthy();
});

it("a pad streams from the game page and answers the busy device", async () => {
  const app = open("/games/fixture-2", "console");
  app.library.busyHosts.set("stream-2", { device: "Living room", playing: null });
  const play = await screen.findByRole("button", { name: "Stream from Living room" });
  await waitFor(() => expect(focused()).toBe(play));
  app.pad.press("confirm");
  const dialog = await screen.findByRole("dialog", { name: "Stop the other game?" });
  expect(dialog.textContent).toContain(
    "Living room is playing another game. Stop it and stream Fixture Game 0003?",
  );
  await waitFor(() => expect(focused()?.textContent).toBe("Stop and stream"));
  app.pad.press("confirm");
  // The stream starts, and console mode's curtain covers the page.
  await waitFor(() =>
    expect(screen.queryByRole("dialog", { name: "Stop the other game?" })).toBeNull(),
  );
  expect((await running(app, "fixture-2")).map((session) => session.entryId)).toEqual(["stream-2"]);
});

it("the guide menu ends a stream, saying the game stops on the other device", async () => {
  let send: (action: PadAction) => void = () => {};
  const api: GuideApi = {
    running: vi.fn(async () => [
      { sessionId: "s1", gameId: "fixture-2", title: "Celeste", stream: true },
    ]),
    returnToGame: vi.fn(async () => {}),
    stop: vi.fn(async () => {}),
  };
  render(
    <InputProvider
      console
      onBack={() => {}}
      subscribe={async (listener) => {
        send = listener;
        return () => {};
      }}
    >
      <GuideMenu api={api} openDetails={() => {}} />
    </InputProvider>,
  );
  const press = (action: Action) =>
    act(() => {
      send({ action, glyphs: "xbox" });
    });
  press("guide");
  const menu = await screen.findByRole("dialog", { name: "Quick menu" });
  expect(menu.textContent).toContain("is streaming from another device.");
  fireEvent.click(within(menu).getByRole("button", { name: "End stream" }));
  expect(menu.textContent).toContain(
    "End the stream? The game stops on the other device, and progress it hasn't saved is lost.",
  );
  fireEvent.click(within(menu).getByRole("button", { name: "End stream" }));
  await waitFor(() => expect(api.stop).toHaveBeenCalledWith("s1"));
});

it("a release says when its device is asleep or busy (#265)", async () => {
  open("/games/fixture-1", "desktop", { ...den, reach: "asleep" });
  const row = (
    await screen.findByRole("heading", { level: 3, name: "Stream from Living room" })
  ).closest('[data-part="entry"]') as HTMLElement;
  expect(within(row).getByText("Asleep: it wakes when you stream")).toBeTruthy();
  // The Release picker names the state too.
  const picker = screen.getByRole("button", { name: /Release/ });
  fireEvent.click(picker);
  expect(
    await screen.findByRole("option", {
      name: "Stream from Living room (Asleep: it wakes when you stream)",
    }),
  ).toBeTruthy();
});
