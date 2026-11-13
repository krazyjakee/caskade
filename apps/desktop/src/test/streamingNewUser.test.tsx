/** #30: a new user lets other devices play, pairs a device and streams a game
 * in one run, by pressing what Caskade shows, and never reads the name of a
 * program underneath. One app plays both devices' parts: the host's and the
 * client's are the same tab. The catalogs hold the rest of what can be shown. */
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.setConfig({ testTimeout: 30_000 });

import { createFixtureGames } from "../api/fixtures";
import { StreamingApiProvider } from "../api/streaming";
import { createStreamingFixtureApi } from "../api/streamingFixtures";
import type { GameDetails } from "../api/types";
import { fakeLayout, fakeViewport, renderApp } from "./harness";

const NAMES = /sunshine|moonlight/i;
const READ_OUT = ["aria-label", "aria-description", "title", "placeholder", "alt"];

/** Every name shown so far, with what it was in. An observer sees each commit,
 * so a status line that has gone by the next step still counts. */
const shown = new Set<string>();
const look = () => {
  const attributes = Array.from(document.querySelectorAll("*"), (element) =>
    READ_OUT.map((name) => element.getAttribute(name)),
  );
  for (const text of [document.title, document.body.textContent, ...attributes.flat()])
    if (text && NAMES.test(text)) shown.add(text.slice(0, 200));
};
const watcher = new MutationObserver(look);

let undoLayout = () => {};
beforeEach(() => {
  undoLayout = fakeLayout();
  fakeViewport();
  shown.clear();
  watcher.observe(document, {
    subtree: true,
    childList: true,
    characterData: true,
    attributes: true,
  });
});
afterEach(() => {
  watcher.disconnect();
  undoLayout();
  cleanup();
});

/** The core adds a device's games once it's paired; the fixture library can't
 * gain a release later, and nothing here opens this game before then. */
function gamesFromStudy(): GameDetails[] {
  const all = createFixtureGames(4);
  (all[2] as GameDetails).entries = [
    {
      id: "stream-1",
      source: "stream:fixture-study",
      state: "partial",
      location: null,
      canLaunch: true,
      emulated: false,
    },
  ];
  return all;
}

it("hosts, pairs and streams without leaving Caskade or reading a name", async () => {
  const leaving = vi.spyOn(window, "open").mockReturnValue(null);
  const streaming = createStreamingFixtureApi();
  const app = renderApp("/settings?tab=streaming", {
    games: gamesFromStudy(),
    wrap: (children) => <StreamingApiProvider api={streaming}>{children}</StreamingApiProvider>,
  });

  // Hosting on.
  fireEvent.click(
    await screen.findByRole("switch", { name: "Let other devices stream games from this one" }),
  );
  await screen.findByText("Ready. Devices on this network can pair and play.");
  expect(streaming.hostingCalls).toEqual([true]);

  // A device asks to pair, and the code it shows goes in here.
  streaming.request({ id: "r1", name: "Steam Deck", address: "192.168.1.40" });
  const asking = await screen.findByText(
    "Steam Deck wants to stream games from this device.",
    {},
    { timeout: 5_000 },
  );
  const form = asking.closest("form") as HTMLElement;
  fireEvent.change(within(form).getByRole("textbox", { name: "Code shown on Steam Deck" }), {
    target: { value: "4831" },
  });
  fireEvent.click(within(form).getByRole("button", { name: "Pair" }));
  await screen.findByText("Steam Deck");
  expect(streaming.accepted).toEqual([{ id: "r1", code: "4831", name: "Steam Deck" }]);

  // This device pairs with another, whose admin enters the code.
  const study = (await screen.findByText("Study")).closest("li") as HTMLElement;
  fireEvent.click(within(study).getByRole("button", { name: "Pair…" }));
  const dialog = await screen.findByRole("dialog", { name: "Pair with Study" });
  await within(dialog).findByText("4831");
  streaming.finishPairing("fixture-study", "paired");
  await within(dialog).findByText(
    "Paired. Study's games show in your library as Stream from Study.",
  );
  fireEvent.click(within(dialog).getByRole("button", { name: "Done" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

  // From the Library to the game, and Stream from Study.
  fireEvent.click(screen.getByRole("link", { name: "Library" }));
  await screen.findByRole("heading", { name: "Library" });
  fireEvent.click(await screen.findByRole("link", { name: /Fixture Game 0003/ }));
  fireEvent.click(await screen.findByRole("button", { name: "View more" }));
  fireEvent.click(await screen.findByRole("button", { name: "Stream from Study" }));
  const end = await screen.findByRole("button", { name: "End stream" });
  const running = async () =>
    (await app.library.listSessions("fixture-2")).filter((session) => session.endedAt === null);
  expect((await running()).map((session) => session.entryId)).toEqual(["stream-1"]);
  fireEvent.click(end);
  await waitFor(async () => expect(await running()).toEqual([]));

  look();
  expect([...shown]).toEqual([]);
  expect(leaving).not.toHaveBeenCalled();
});

it("a search for a name finds Streaming and shows only Caskade's words", async () => {
  renderApp("/settings");
  const search = await screen.findByRole("searchbox", { name: "Search settings" });
  for (const name of ["Sunshine", "Moonlight"]) {
    fireEvent.change(search, { target: { value: name } });
    const results = await screen.findByRole("list", { name: "Settings search results" });
    expect(within(results).getByRole("button", { name: "Streaming" })).toBeTruthy();
  }
  look();
  expect([...shown]).toEqual([]);
});

/** What the search index matches and never draws, as the test above shows;
 * the native UI's index (#103) shares the words. */
const SEARCH_WORDS = [
  "client, Moonlight, pair",
  "host, hosting, Sunshine",
  "stream, remote play, play on TV, Moonlight, Sunshine, host, quality, bitrate, resolution, frame rate",
];

it("no catalog text can show a name, but the settings search's own words", () => {
  const locales = join(import.meta.dirname, "../../../../locales");
  const named: Record<string, string[]> = {};
  for (const file of readdirSync(locales).filter((name) => name.endsWith(".pot"))) {
    // Long strings wrap over lines; comments never reach the screen.
    const text = readFileSync(join(locales, file), "utf8").replaceAll('"\n"', "");
    for (const entry of text.split(/\n{2,}/)) {
      for (const [, message] of entry.matchAll(/^msgid(?:_plural)? "(.*)"$/gm)) {
        if (!message || !NAMES.test(message)) continue;
        named[message] = [...entry.matchAll(/^#: (.*)$/gm)].flatMap(([, from]) =>
          (from as string).split(" "),
        );
      }
    }
  }
  expect(named).toEqual(
    Object.fromEntries(
      SEARCH_WORDS.map((words) => [
        words,
        [
          "apps/desktop/src/views/settings/searchIndex.ts",
          "apps/native/src/settings/search/search_index.rs",
        ],
      ]),
    ),
  );
});
