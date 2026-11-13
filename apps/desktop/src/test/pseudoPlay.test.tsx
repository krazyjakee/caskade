/** The shells and what Play asks, in the pseudo-locale (#9): the whole window,
 * signing in, the curtain and guide menu, and every question Play can ask
 * before a game starts. */
import { i18n, loadCatalog } from "@caskade/i18n";
import { I18nProvider } from "@lingui/react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { expect, it, vi } from "vitest";
import type { FixtureAccount } from "../api/accountsFixtures";
import { createFixtureGames } from "../api/fixtures";
import { fixtureJob } from "../api/installFixtures";
import type { BeforePlay, PlayingElsewhere } from "../api/saves";
import { StreamingApiProvider } from "../api/streaming";
import { createStreamingFixtureApi } from "../api/streamingFixtures";
import type { GameDetails, LaunchPrompt } from "../api/types";
import { type GameUpdate, type GameUpdatesApi, GameUpdatesProvider } from "../api/updates";
import { InputProvider } from "../input";
import { type GuideApi, GuideMenu, type RunningGame } from "../input/GuideMenu";
import type { Mode } from "../shell/console/mode";
import { fakePad, playedGames } from "./harness";
import {
  all,
  devices,
  dialogData,
  dialogOf,
  gameHeading,
  here,
  locale,
  modes,
  one,
  open,
  over,
  padGlyphs,
  paths,
  ready,
  shown,
  there,
} from "./pseudoHarness";

// Whole windows: the shells around a view.

it.each(modes)("shows the whole window in the pseudo-locale (%s)", async (mode) => {
  await open("/", { mode, games: playedGames(8) });
  const shell = `[data-cs="${mode === "console" ? "console-shell" : "app-shell"}"]`;
  await ready(`${shell} [data-view="home"] [data-cs="game-card"]`);
  if (mode === "desktop")
    await waitFor(() => expect(one(shell)?.textContent).toContain("Fixture installation"));
  expect(shown([shell], ["Fixture installation", /\bPlayer\b/g], padGlyphs)).toEqual([]);
});

// Signing in.

const family: FixtureAccount[] = [
  { id: "ada", name: "Ada", role: "admin", avatar: "avatar:rocket", pin: "2468" },
  { id: "kid", name: "Kid", avatar: "avatar:moon" },
];

const picker = '[data-cs="account-picker"]';
const pinPad = '[data-cs="pin-pad"]';

/** Accounts' names, as their tiles show them. */
const signIn = () => shown([picker], [/\b(?:Ada|Kid)\b/g], padGlyphs);

const enterPin = (pin: string) => {
  const field = one(`${pinPad} [data-part="field"]`) as HTMLInputElement;
  fireEvent.change(field, { target: { value: pin } });
  fireEvent.submit(field.closest("form") as Element);
};

it.each(modes)("shows who's playing and the PIN pad in the pseudo-locale (%s)", async (mode) => {
  await open("/", { mode, accounts: family });
  await ready(`${picker} [data-part="account"]`);
  if (mode === "console") await ready(`${picker} [data-cs="hint-bar"] [data-part="hint"]`);
  expect(signIn()).toEqual([]);

  fireEvent.click(one(`${picker} [data-part="account"][data-status="locked"]`) as Element);
  await ready(pinPad);
  if (mode === "console") await ready(`${picker} [data-cs="hint-bar"] [data-part="hint"]`);
  expect(signIn()).toEqual([]);

  // Too short, then wrong.
  enterPin("12");
  await waitFor(() => expect(one(`${pinPad} [data-part="message"]`)?.textContent).not.toBe(""));
  expect(signIn()).toEqual([]);
  enterPin("1357");
  await waitFor(() => expect(one(pinPad)?.getAttribute("data-status")).toBe("wrong"));
  expect(signIn()).toEqual([]);
});

it("shows a locked PIN pad in the pseudo-locale", async () => {
  await open("/", {
    accounts: [
      { ...(family[0] as FixtureAccount), lockedUntilMs: Date.now() + 90_000 },
      family[1] as FixtureAccount,
    ],
  });
  await ready(`${picker} [data-part="account"]`);
  fireEvent.click(one(`${picker} [data-part="account"][data-status="locked"]`) as Element);
  await waitFor(() => expect(one(pinPad)?.getAttribute("data-status")).toBe("locked"));
  expect(signIn()).toEqual([]);
});

// Console mode's curtain and guide menu.

it("shows the curtain over a running game in the pseudo-locale", async () => {
  const app = await open("/games/fixture-1", { mode: "console" });
  await gameHeading();
  await app.library.launchGame("entry-1");
  await waitFor(() =>
    expect(one('[data-cs="game-curtain"]')?.getAttribute("data-state")).toBe("covered"),
  );
  await waitFor(() =>
    expect(one('[data-cs="game-curtain"] [data-part="title"]')?.textContent).toMatch(/Fixture/),
  );
  // The guide button's name is printed on the pad, as in the hint bar.
  expect(shown(['[data-cs="game-curtain"]', '[role="dialog"]'], [], "kbd")).toEqual([]);
});

it("shows the curtain before a game's title is known in the pseudo-locale", async () => {
  const app = await open("/", { mode: "console" });
  await ready('[data-cs="game-curtain"]');
  act(() =>
    app.library.emit({
      type: "game.started",
      sessionId: "s1",
      entryId: "gone",
      gameId: "not-in-the-library",
    }),
  );
  await ready('[data-cs="game-curtain"] [data-part="title"]');
  expect(shown(['[data-cs="game-curtain"]'], [], "kbd")).toEqual([]);
});

/** The guide menu, as the shell mounts it, for `game`. */
async function guide(game: RunningGame, stop?: () => Promise<void>) {
  i18n.loadAndActivate({ locale, messages: await loadCatalog(locale) });
  const pad = fakePad();
  const api: GuideApi = {
    running: async () => [game],
    returnToGame: async () => {},
    stop: stop ?? (async () => {}),
  };
  render(
    <I18nProvider i18n={i18n}>
      <InputProvider console onBack={() => {}} subscribe={pad.subscribe}>
        <GuideMenu api={api} openDetails={() => {}} />
      </InputProvider>
    </I18nProvider>,
  );
  pad.press("guide");
  await ready('[data-cs="guide-menu"]');
}

const guideButton = (index: number) =>
  all('[data-cs="guide-menu"] button')[index] as HTMLButtonElement;

it.each([false, true])("shows the guide menu in the pseudo-locale (stream: %s)", async (stream) => {
  await guide({ sessionId: "s1", gameId: "fixture-1", title: "Fixture Game 0002", stream }, () =>
    Promise.reject(new Error("The game won't stop")),
  );
  expect(shown(['[data-cs="guide-menu"]'])).toEqual([]);
  // Stop asks first, and may fail. The reason is the core's (#214).
  fireEvent.click(guideButton(1));
  await waitFor(() => expect(all('[data-cs="guide-menu"] button')).toHaveLength(2));
  expect(shown(['[data-cs="guide-menu"]'])).toEqual([]);
  fireEvent.click(guideButton(0));
  await ready('[data-cs="guide-menu"] [role="alert"]');
  expect(shown(['[data-cs="guide-menu"]'], ["The game won't stop"])).toEqual([]);
});

// What Play asks first.

const play = async () => {
  const [button] = await screen.findAllByRole("button", { name: /Ƥĺàŷ/ });
  fireEvent.click(button as Element);
};

it.each(modes)("asks how to play in the pseudo-locale (%s)", async (mode) => {
  const prompt: LaunchPrompt = {
    entryId: "entry-1",
    platformId: "nintendo_snes",
    actions: [
      { id: "story", name: "Story", default: true },
      { id: "mods", name: "Mods", default: false },
    ],
    profiles: [
      {
        emulatorId: "ra",
        emulatorName: "RetroArch",
        profileId: "snes",
        profileName: "Snes9x",
        installed: true,
      },
      {
        emulatorId: "bsnes",
        emulatorName: "bsnes",
        profileId: "default",
        profileName: "Accuracy",
        installed: false,
      },
    ],
    discs: [{ index: 0, name: "Disc 1.chd" }],
  };
  const app = await open("/games/fixture-1", { mode });
  await gameHeading();
  const launch = vi.spyOn(app.library, "launchGame");
  launch.mockResolvedValueOnce({ type: "choose", prompt });
  await play();
  await dialogOf("launch-choice");
  const data = [/Story|Mods|RetroArch|Snes9x|bsnes|Accuracy|Disc 1/g, ...paths];
  expect(over(data)).toEqual([]);
  cleanup();

  const discs = await open("/games/fixture-1", { mode });
  await gameHeading();
  vi.spyOn(discs.library, "launchGame").mockResolvedValueOnce({
    type: "choose",
    prompt: { ...prompt, actions: [], profiles: [] },
  });
  await play();
  await dialogOf("launch-choice");
  expect(over(data)).toEqual([]);
});

const update: GameUpdate = {
  entryId: "entry-1",
  gameId: "fixture-1",
  provider: "github",
  project: "owner/game",
  projectUrl: "https://github.com/owner/game/releases",
  installed: "v1.0",
  latest: "v1.1",
  latestUrl: "https://github.com/owner/game/releases/tag/v1.1",
  available: true,
  skipped: null,
  prereleases: false,
  checkedAtMs: Date.UTC(2026, 9, 28),
  error: null,
};

const updatesApi = (offer: GameUpdate): GameUpdatesApi => ({
  game: async () => [offer],
  check: async () => offer,
  beforePlay: async () => offer,
  skip: async () => offer,
  setPrereleases: async () => offer,
  install: async () => "fixture-job",
});

it.each(modes)("offers an update before playing in the pseudo-locale (%s)", async (mode) => {
  // Known installed release, and not known.
  for (const offer of [update, { ...update, installed: null, latestUrl: null }]) {
    await open("/games/fixture-1", {
      mode,
      wrap: (children: ReactNode) => (
        <GameUpdatesProvider api={updatesApi(offer)}>{children}</GameUpdatesProvider>
      ),
    });
    await gameHeading();
    await play();
    await dialogOf("update-offer");
    expect(over([...dialogData])).toEqual([]);
    cleanup();
  }
});

it("follows the update it offered in the pseudo-locale", async () => {
  const app = await open("/games/fixture-1", {
    wrap: (children: ReactNode) => (
      <GameUpdatesProvider api={updatesApi(update)}>{children}</GameUpdatesProvider>
    ),
  });
  await gameHeading();
  await play();
  await dialogOf("update-offer");
  const dialog = one('[data-cs="update-offer"]') as Element;
  fireEvent.click([...all('[data-cs="update-offer"] button')].at(-1) as Element);
  await waitFor(() => expect(dialog.textContent).toContain("40"));
  expect(over()).toEqual([]);
  const states = [
    { status: "needs_user" },
    { status: "cancelled" },
    { status: "failed", error: "The disk is full" },
  ] as const;
  for (const [index, state] of states.entries()) {
    const before = dialog.textContent;
    act(() => app.installs.emit(fixtureJob({ ...state, revision: index + 2 })));
    await waitFor(() => expect(dialog.textContent).not.toBe(before));
    // The job's error is the core's (#214).
    expect(over(["The disk is full"])).toEqual([]);
  }
});

async function playChecked(mode: Mode, check: BeforePlay) {
  const app = await open("/games/fixture-1", { mode });
  app.library.saves.seedCheck("entry-1", check);
  await gameHeading();
  await play();
  return app;
}

it.each(modes)("asks which save to play with in the pseudo-locale (%s)", async (mode) => {
  await playChecked(mode, {
    check: { type: "conflict", versions: [there, here] },
    checked: true,
    warnings: [],
  });
  await dialogOf("save-conflict");
  expect(over([devices])).toEqual([]);
});

it.each(modes)("waits for a save on its way in the pseudo-locale (%s)", async (mode) => {
  await playChecked(mode, {
    check: { type: "notArrived", version: there },
    checked: true,
    warnings: [],
  });
  await dialogOf("save-arriving");
  expect(over([devices])).toEqual([]);
});

it("says what the save check did in the pseudo-locale", async () => {
  const warning = "A save folder couldn't be read";
  const checks: BeforePlay[] = [
    { check: { type: "restored", version: there }, checked: false, warnings: [warning] },
    { check: { type: "upToDate" }, checked: false, warnings: [] },
  ];
  for (const check of checks) {
    await playChecked("desktop", check);
    await dialogOf("save-note");
    // A warning's words are the core's (#214).
    expect(shown(['[data-cs="save-note"]'], [warning, devices])).toEqual([]);
    cleanup();
  }

  // Skipped the wait: the game started without the newer save.
  await playChecked("desktop", {
    check: { type: "notArrived", version: there },
    checked: true,
    warnings: [],
  });
  await dialogOf("save-arriving");
  fireEvent.click(all('[data-cs="save-arriving"] button')[1] as Element);
  await dialogOf("save-note");
  expect(shown(['[data-cs="save-note"]'], [devices])).toEqual([]);
});

const onDeck: PlayingElsewhere = {
  deviceId: "b".repeat(64),
  deviceName: "Steam Deck",
  gameId: "fixture-1",
  title: "Fixture Game 0002",
  accountId: "",
  startedAtMs: Date.UTC(2026, 10, 2, 19),
};

it.each(modes)("warns about a game running elsewhere in the pseudo-locale (%s)", async (mode) => {
  // A device by its name, and one that has none.
  for (const deviceName of ["Steam Deck", ""]) {
    const app = await open("/games/fixture-1", { mode });
    app.library.saves.setPlaying([{ ...onDeck, deviceName }]);
    await gameHeading();
    await ready('[data-view="game"] [data-part="playing-elsewhere"]');
    expect(shown(['[data-part="playing-elsewhere"]'], [devices])).toEqual([]);
    await play();
    await dialogOf("playing-elsewhere");
    expect(over([devices])).toEqual([]);
    cleanup();
  }
});

/** A game only the living room's device has. */
function streamedGames(): GameDetails[] {
  const games = createFixtureGames(3);
  (games[2] as GameDetails).entries = [
    {
      id: "stream-2",
      source: "stream:fixture-living-room",
      state: "partial",
      location: null,
      canLaunch: true,
      emulated: false,
    },
  ];
  return games;
}

it.each(modes)("asks before stopping a busy device in the pseudo-locale (%s)", async (mode) => {
  // Its game by name, and one it can't name.
  for (const playing of ["Hades", null]) {
    const app = await open("/games/fixture-2", {
      mode,
      games: streamedGames(),
      wrap: (children: ReactNode) => (
        <StreamingApiProvider api={createStreamingFixtureApi()}>{children}</StreamingApiProvider>
      ),
    });
    app.library.busyHosts.set("stream-2", { device: "Living room", playing });
    await screen.findByRole("heading", { level: 1, name: "Fixture Game 0003" });
    fireEvent.click(await screen.findByRole("button", { name: /Śţŕēàḿ ƒŕōḿ/ }));
    await dialogOf("stop-host");
    expect(over([devices, "Hades"])).toEqual([]);
    cleanup();
  }
});
