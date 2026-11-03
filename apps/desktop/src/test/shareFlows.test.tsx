/** #247: sharing a game once and receiving one with a pad alone: Share
 * with someone… from a game's menu (the link as a QR code in console mode),
 * Receive a shared game… with the link pasted on the on-screen keyboard,
 * and the shares list in Settings → Devices. Every step is a pad action;
 * the clipboard holds what someone sent. */
import { loadCatalog, pseudoLocale } from "@caskade/i18n";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.setConfig({ testTimeout: 30_000 });

import type { ShareRecord, SharesApi } from "../api/shares";
import { SharesProvider } from "../api/shares";
import {
  createSharesFixtureApi,
  FIXTURE_LINK,
  fixtureShare,
  fixtureShareInfo,
} from "../api/sharesFixtures";
import type { GameDetails } from "../api/types";
import type { Action } from "../input/actions";
import { InstallConfirmation } from "../views/activity/InstallConfirmation";
import { fakeLayout, fakeViewport, focused, playedGames, renderApp, until } from "./harness";
import { untranslated } from "./untranslated";

let undoLayout = () => {};
let clipboard = "";
beforeEach(() => {
  undoLayout = fakeLayout();
  fakeViewport();
  clipboard = "";
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: {
      readText: async () => clipboard,
      writeText: async (text: string) => {
        clipboard = text;
      },
    },
  });
});
afterEach(() => {
  undoLayout();
  cleanup();
});

/** Every game played and installed in a folder of its own, so each can
 * be shared. */
function games(): GameDetails[] {
  return playedGames(3).map((game) => ({
    ...game,
    entries: game.entries.map((entry) => ({ ...entry, state: "installed", canLaunch: true })),
  }));
}

function open(path: string, api: SharesApi = createSharesFixtureApi()) {
  const app = renderApp(path, {
    mode: "console",
    games: games(),
    wrap: (children) => <SharesProvider api={api}>{children}</SharesProvider>,
  });
  return { app, api };
}

type App = ReturnType<typeof renderApp>;

/** Presses `action` until `done`, at most `limit` times. */
async function padTo(app: App, action: Action, done: () => boolean, what: string, limit = 30) {
  for (let i = 0; i < limit && !done(); i++) app.pad.press(action);
  await until(done, what);
}

/** Moves focus into the view, then down it to what `done` names, else up
 * to it, at most `limit` steps each way. */
async function seek(app: App, done: () => boolean, what: string, limit = 80) {
  const inView = () => !focused()?.closest('[data-cs="console-sidebar"]');
  const step = (action: Action) => {
    if (!inView()) app.pad.press("nav.right");
    else app.pad.press(action);
  };
  for (let i = 0; i < limit && !done(); i++) step("nav.down");
  for (let i = 0; i < 2 * limit && !done(); i++) step("nav.up");
  await until(done, what);
}

const onCard = () => Boolean(focused()?.closest('[data-cs="game-card"]'));

const focusedText = () => focused()?.textContent ?? "";

it("a pad shares a game from its menu, and console mode shows the link as a QR code", async () => {
  const { app, api } = open("/library");
  await until(onCard, "a game");
  // Start: the game's menu, down to Share with someone….
  app.pad.press("menu");
  const menu = await screen.findByRole("menu");
  // Once the game's details are in, the menu offers it.
  await within(menu).findByRole("menuitem", { name: /Share with someone…/ });
  await padTo(app, "nav.down", () => focusedText().startsWith("Share with someone…"), "Share");
  app.pad.press("confirm");
  const dialog = await screen.findByRole("dialog", { name: /^Share / });
  await until(() => focusedText() === "Make a link", "Make a link");
  app.pad.press("confirm");

  // The link, its QR code, and Copy where Make a link was.
  await within(dialog).findByRole("img", { name: "The link as a QR code" });
  await until(() => focusedText() === "Copy", "Copy");
  const link = within(dialog).getByText((text) => text.startsWith(FIXTURE_LINK)).textContent ?? "";
  app.pad.press("confirm");
  await waitFor(() => expect(clipboard).toBe(link));
  expect(within(dialog).getByText("Nobody has opened the link yet.")).toBeTruthy();
  const [share] = await api.list();
  expect(share?.expiresAtMs).not.toBeNull();

  // B closes it; the share goes on.
  app.pad.press("back");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect((await api.list())[0]?.state.state).toBe("ready");
});

it("a pad receives a shared game, the link pasted on the on-screen keyboard", async () => {
  clipboard = `${FIXTURE_LINK}hades\n`;
  const { app, api } = open("/library");
  await until(onCard, "a game");
  await seek(app, () => focusedText() === "Add game", "Add game");
  app.pad.press("confirm");
  await screen.findByRole("menu");
  await padTo(app, "nav.down", () => focusedText().startsWith("Receive a shared game…"), "Receive");
  app.pad.press("confirm");
  const dialog = await screen.findByRole("dialog", { name: "Receive a shared game" });
  const field = within(dialog).getByRole("textbox", { name: "Share link" });
  await until(() => focused() === field, "the link field");

  // A opens the keyboard; down to its bottom row, along to Paste.
  app.pad.press("confirm");
  // Outside the dialog, so the dialog's modal hides it from roles.
  await until(() => Boolean(document.querySelector('[data-cs="osk"]')), "the keyboard");
  const keyboard = document.querySelector('[data-cs="osk"]') as HTMLElement;
  const on = (key: string) => Boolean(keyboard.querySelector(`[data-key="${key}"][data-focused]`));
  await padTo(app, "nav.down", () => on("layer"), "the bottom row", 6);
  await padTo(app, "nav.right", () => on("paste"), "Paste", 6);
  app.pad.press("confirm");
  await waitFor(() => expect((field as HTMLInputElement).value).toBe(`${FIXTURE_LINK}hades`));
  // Start is Done: the keyboard closes and the link is looked at.
  app.pad.press("menu");
  await within(dialog).findByText("Alex's Desk shares Hades with you.");

  // Help others receive it, then Receive.
  const onHelp = () =>
    Boolean(focused()?.closest('[data-cs="checkbox"]')?.textContent?.includes("Help others"));
  await seek(app, onHelp, "Help others");
  app.pad.press("confirm");
  await seek(app, () => focusedText() === "Receive", "Receive");
  app.pad.press("confirm");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  const [received] = await api.received();
  expect(received).toMatchObject({ title: "Hades", sharer: "Alex's Desk", keepHelping: true });
});

it("a link that expired says so, and nothing is received", async () => {
  const api = createSharesFixtureApi();
  await expect(api.inspect(`${FIXTURE_LINK}expired`)).rejects.toThrow(/expired on/);
  clipboard = `${FIXTURE_LINK}expired`;
  const { app } = open("/library", api);
  await until(onCard, "a game");
  await seek(app, () => focusedText() === "Add game", "Add game");
  app.pad.press("confirm");
  await screen.findByRole("menu");
  await padTo(app, "nav.down", () => focusedText().startsWith("Receive a shared game…"), "Receive");
  app.pad.press("confirm");
  const dialog = await screen.findByRole("dialog", { name: "Receive a shared game" });
  await until(() => focused()?.tagName === "INPUT", "the link field");
  app.pad.press("confirm");
  // Outside the dialog, so the dialog's modal hides it from roles.
  await until(() => Boolean(document.querySelector('[data-cs="osk"]')), "the keyboard");
  const keyboard = document.querySelector('[data-cs="osk"]') as HTMLElement;
  const on = (key: string) => Boolean(keyboard.querySelector(`[data-key="${key}"][data-focused]`));
  await padTo(app, "nav.down", () => on("layer"), "the bottom row", 6);
  await padTo(app, "nav.right", () => on("paste"), "Paste", 6);
  app.pad.press("confirm");
  const field = within(dialog).getByRole("textbox", { name: "Share link" }) as HTMLInputElement;
  await waitFor(() => expect(field.value).toBe(`${FIXTURE_LINK}expired`));
  app.pad.press("menu");
  await waitFor(() => expect(dialog.textContent).toContain("This link expired on 2026-11-01"));
  expect(
    (within(dialog).getByRole("button", { name: "Receive" }) as HTMLButtonElement).disabled,
  ).toBe(true);
  expect(await api.received()).toEqual([]);
});

it("Settings → Devices lists shares: a pad copies a link, stops a share and forgets it", async () => {
  const running: ShareRecord = fixtureShare({ title: "Celeste" });
  const api = createSharesFixtureApi({
    shares: [running],
    received: [
      {
        job: "job-1",
        title: "Hades",
        sharer: "Alex's Desk",
        receiving: true,
        keepHelping: false,
        serving: true,
        helping: 2,
      },
    ],
  });
  const { app } = open("/settings?tab=devices", api);
  const row = (await screen.findByText(/^Celeste/)).closest('[data-cs="share"]') as HTMLElement;
  expect(within(row).getByText("Sam's PC")).toBeTruthy();
  expect(within(row).getByText("Helping others receive it")).toBeTruthy();
  const received = screen
    .getByText("Hades from Alex's Desk")
    .closest('[data-cs="received-share"]') as HTMLElement;
  expect(within(received).getByText("Helping 2 others receive it.")).toBeTruthy();

  await seek(app, () => focusedText() === "Copy link", "Copy link");
  app.pad.press("confirm");
  await waitFor(() => expect(clipboard).toBe(`${FIXTURE_LINK}${running.id}`));
  // Stop sharing is beside it, and Forget takes Copy link's place.
  await padTo(app, "nav.down", () => focusedText() === "Stop sharing", "Stop sharing", 3);
  app.pad.press("confirm");
  await within(row).findByText("Stopped: the link doesn't work any more.");
  await seek(app, () => focusedText() === "Forget", "Forget", 4);
  app.pad.press("confirm");
  await waitFor(() => expect(screen.queryByText(/^Celeste/)).toBeNull());

  // The switch keeps the received share helping once it's here.
  const onSwitch = () =>
    focused()?.closest('[data-cs="switch"]')?.textContent === "Help others receive it";
  await seek(app, onSwitch, "the switch", 6);
  app.pad.press("confirm");
  await waitFor(async () => expect((await api.received())[0]?.keepHelping).toBe(true));
});

it("a shared collection says which games are here, and joins the collections", async () => {
  const api = createSharesFixtureApi();
  const app = renderApp("/library", {
    games: games(),
    wrap: (children) => <SharesProvider api={api}>{children}</SharesProvider>,
  });
  fireEvent.click(await screen.findByRole("button", { name: "Add game" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: /Receive a shared game…/ }));
  const dialog = await screen.findByRole("dialog", { name: "Receive a shared game" });
  fireEvent.change(within(dialog).getByRole("textbox", { name: "Share link" }), {
    target: { value: `${FIXTURE_LINK}collection` },
  });
  fireEvent.click(within(dialog).getByRole("button", { name: "Look" }));
  await within(dialog).findByText("Alex's Desk shares the collection Co-op nights with you.");
  const games_ = within(dialog).getByRole("list");
  expect(games_.textContent).toContain("HadesInstalled");
  expect(games_.textContent).toContain("It Takes TwoNot in your library");
  // A list: nothing to receive, no helping others.
  expect(within(dialog).queryByRole("button", { name: "Receive" })).toBeNull();
  expect(within(dialog).queryByText(/Help others/)).toBeNull();
  fireEvent.click(within(dialog).getByRole("button", { name: "Add to my collections" }));
  await waitFor(() =>
    expect(app.router.state.location.pathname).toMatch(/^\/collections\/collection-/),
  );
});

it("a share link opened from outside asks before anything comes", async () => {
  const confirm = vi.fn(async () => {});
  render(
    <QueryClientProvider client={new QueryClient()}>
      <InstallConfirmation
        request={{
          requestId: "r1",
          kind: "share",
          value: "Hades",
          share: fixtureShareInfo({ reach: "relayOnly", size: 1_500_000 }),
        }}
        confirm={confirm}
        dismiss={async () => {}}
      />
    </QueryClientProvider>,
  );
  const dialog = await screen.findByRole("dialog", { name: "Receive Hades?" });
  expect(dialog.textContent).toContain("Alex's Desk shares Hades with you.");
  expect(dialog.textContent).toContain("Windows (runs through Proton on Linux)");
  expect(dialog.textContent).toContain("Only a relay reaches Alex's Desk's computer right now.");
  within(dialog).getByRole("button", { name: "Receive" }).click();
  await waitFor(() => expect(confirm).toHaveBeenCalledWith("r1"));
});

it("the share dialogs and list are all translated (pseudo-locale)", async () => {
  const locale = pseudoLocale ?? "en-XA";
  const api = createSharesFixtureApi({
    shares: [fixtureShare()],
    received: [
      {
        job: "job-1",
        title: "Hades",
        sharer: "Alex",
        receiving: false,
        keepHelping: true,
        serving: true,
        helping: 1,
      },
    ],
  });
  const app = renderApp("/settings?tab=devices", {
    locale,
    messages: await loadCatalog(locale),
    games: games(),
    wrap: (children) => <SharesProvider api={api}>{children}</SharesProvider>,
  });
  const share = await waitFor(() => {
    const found = document.querySelector('[data-cs="share"]');
    if (!found) throw new Error("no share yet");
    return found;
  });
  // Names, sizes and dates are data.
  const data = {
    data: [
      /Celeste/g,
      /Hades/g,
      /Alex/g,
      /Sam's PC/g,
      /[\d.,]+\s?[KMGT]?B\b/g,
      /\w{3} \d{1,2}, \d{4}, \d{1,2}:\d{2}\s?[AP]M/g,
    ],
  };
  expect(untranslated([share, document.querySelector('[data-cs="received-share"]')], data)).toEqual(
    [],
  );
  void app;
});
