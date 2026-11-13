/** What opens over the views, in the pseudo-locale (#9): console mode's quick
 * look, a game's menu and its dialogs, the game editor, Add game, a plugin's
 * tabs and views, emulation, selection and the bulk editor, collection
 * dialogs and a Select's list. */
import type { PluginContext } from "@caskade/sdk";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { expect, it, vi } from "vitest";
import { BrowseApiContext } from "../api/browse";
import { createBrowseFixtureApi } from "../api/browseFixtures";
import { createEmulationFixtureApi } from "../api/emulationFixtures";
import { FIXTURE_LINK } from "../api/sharesFixtures";
import { createStorageFixtureApi } from "../api/storageFixtures";
import { PluginRegistry } from "../plugins/registry";
import type { UiPlugin } from "../plugins/types";
import type { Mode } from "../shell/console/mode";
import { focused, playedGames, until } from "./harness";
import {
  all,
  collectionData,
  dialogData,
  dialogOf,
  gameHeading,
  gameMenu,
  managed,
  managedGames,
  menuItem,
  modes,
  one,
  open,
  over,
  overlays,
  padGlyphs,
  paths,
  reactAria,
  ready,
  rightClick,
  shown,
  strings,
  type,
  webGame,
} from "./pseudoHarness";

// Console mode's quick look.

it("shows console mode's quick look from the pad in the pseudo-locale", async () => {
  const app = await open("/library?filter=installed&sort=title", {
    mode: "console",
    games: playedGames(8),
  });
  await until(() => Boolean(focused()?.closest('[data-cs="game-card"]')), "the first game");
  app.pad.press("details");
  await ready('[data-cs="quick-look"] [data-part="facts"]');
  await ready('[data-cs="hint-bar"] [data-part="hint"]');
  expect(over()).toEqual([]);
});

/** Opens the game menu's `item`. */
async function fromMenu(item: RegExp) {
  await gameMenu();
  fireEvent.click(await menuItem(item));
}

it.each(modes)("shows a game's menu in the pseudo-locale (%s)", async (mode) => {
  await managed(mode);
  await gameMenu();
  expect(over(dialogData)).toEqual([]);
});

it.each(modes)("shows a running game's menu in the pseudo-locale (%s)", async (mode) => {
  const app = await managed(mode);
  await gameHeading();
  await app.library.launchGame("entry-1");
  await gameMenu();
  await menuItem(/Śţōƥ/);
  expect(over(dialogData)).toEqual([]);
});

it("shows a web game's menu and its Remove dialog in the pseudo-locale", async () => {
  await open("/games/alpha", { games: [webGame("alpha")], storage: createStorageFixtureApi() });
  rightClick(await screen.findByRole("heading", { level: 1, name: "Web alpha" }));
  await menuItem(/Ƥĺàŷ ōƒƒĺĩńē/);
  expect(over([/Web alpha/g])).toEqual([]);
  fireEvent.click(await menuItem(/Ŕēḿōvē…/));
  await screen.findByRole("alertdialog");
  expect(over([/Web alpha/g])).toEqual([]);
});

// The dialogs a game's menu opens.

it.each(modes)("shows the move dialog in the pseudo-locale (%s)", async (mode) => {
  const roots = await createStorageFixtureApi().list();
  // Another location to go to, none, and a game Caskade didn't install.
  for (const list of [roots, roots.slice(0, 1), []]) {
    await open("/games/fixture-1", {
      mode,
      games: managedGames(),
      storage: { ...createStorageFixtureApi(), list: async () => structuredClone(list) },
    });
    await fromMenu(/Ḿōvē ĩńśţàĺĺēď ƒĩĺēś/);
    await ready('[role="dialog"] [data-cs="select"]');
    expect(over([...dialogData, /\bExternal\b/g])).toEqual([]);
    cleanup();
  }
});

it.each(modes)("shows the uninstall dialog in the pseudo-locale (%s)", async (mode) => {
  await managed(mode);
  await fromMenu(/Ũńĩńśţàĺĺ…/);
  await screen.findByRole("alertdialog");
  expect(over(dialogData)).toEqual([]);
});

it.each(modes)("shows the remove dialog in the pseudo-locale (%s)", async (mode) => {
  // With Caskade's install to delete, and without.
  for (const source of ["caskade-storage", "manual"]) {
    const games = managedGames().map((game) => ({
      ...game,
      entries: game.entries.map((entry) => ({ ...entry, source })),
    }));
    await open("/games/fixture-1", { mode, games, storage: createStorageFixtureApi() });
    rightClick(await gameHeading());
    fireEvent.click(await menuItem(/Ŕēḿōvē ƒŕōḿ ĺĩƀŕàŕŷ/));
    await screen.findByRole("alertdialog");
    expect(over(dialogData)).toEqual([]);
    cleanup();
  }
});

it.each(modes)("shows the collection picker in the pseudo-locale (%s)", async (mode) => {
  await managed(mode);
  await fromMenu(/Àďď ţō ćōĺĺēćţĩōń/);
  await ready('[role="dialog"] [role="checkbox"], [role="dialog"] input[type="checkbox"]');
  expect(over([...dialogData, ...collectionData])).toEqual([]);
});

it.each(modes)("shows Share with someone in the pseudo-locale (%s)", async (mode) => {
  await managed(mode);
  await fromMenu(/Śĥàŕē ŵĩţĥ śōḿēōńē/);
  const dialog = await screen.findByRole("dialog");
  expect(over(dialogData)).toEqual([]);
  // The link it makes.
  fireEvent.click(await within(dialog).findByRole("button", { name: /Ḿàķē à ĺĩńķ/ }));
  await ready('[data-cs="share-game"] [data-part="link"]');
  expect(over([/caskade:\/\/\S+/g, ...dialogData])).toEqual([]);
});

it("says a game didn't start in the pseudo-locale", async () => {
  const app = await managed();
  vi.spyOn(app.library, "launchGame").mockRejectedValue(new Error("The emulator is missing"));
  await fromMenu(/Ƥĺàŷ/);
  await screen.findByRole("alertdialog");
  // The reason is the core's (#214).
  expect(over([...dialogData, "The emulator is missing"])).toEqual([]);
});

// The game editor.

it.each(modes)("shows the game editor's tabs in the pseudo-locale (%s)", async (mode) => {
  const app = await managed(mode);
  const location = { kind: "folder", path: "/home/me/.local/share/fixture" } as const;
  const found = [{ spec: location, tags: ["save"], fileCount: 2 }];
  app.library.saves.seed({
    entryId: "entry-1",
    revision: 1,
    preferences: { manifestTitle: null, locations: null },
    matched: { type: "matched", title: "Fixture Game", method: "steam" },
    automatic: found,
    locations: found,
    warnings: [],
  });
  await fromMenu(/Ēďĩţ…/);
  // Every tab stays mounted, hidden, so all five are in the dialog at once.
  await ready('[data-cs="game-editor"] [data-part="field"]');
  await ready('[data-cs="game-editor"] [data-part="artwork"] [data-part="slot"]');
  await ready('[data-cs="game-editor"] [data-cs="game-actions"] [data-part="action"]');
  await ready('[data-cs="game-editor"] [data-cs="scripts"] [data-part="order"]');
  await ready('[data-cs="game-editor"] [data-cs="game-saves"] [data-part="location"]');
  // The built-in action's name is the core's (#214), as is the save match's;
  // Proton builds are named by their folders.
  const builds = await app.library.actions.protonBuilds();
  const data = [
    ...dialogData,
    /CASKADE_\w+/g,
    /\{\w+\}/g,
    /^Play$/,
    "Fixture Game",
    ...builds.installed,
  ];
  expect(over(data)).toEqual([]);

  // Unsaved work asks first.
  const title = one('[data-cs="game-editor"] input') as HTMLInputElement;
  fireEvent.change(title, { target: { value: "Changed" } });
  fireEvent.click(one('[data-cs="game-editor"] [data-part="cancel"]') as Element);
  await ready('[data-cs="game-editor-discard"]');
  expect(over(data)).toEqual([]);
});

// Add game.

async function openAddGame() {
  await ready('[data-cs="add-game"] button');
  fireEvent.click(one('[data-cs="add-game"] button') as Element);
}

/** Opens one of Add game's items. */
async function addGame(item: RegExp) {
  await openAddGame();
  fireEvent.click(await menuItem(item));
  return screen.findByRole("dialog");
}

const addGameTo = (mode: Mode) =>
  open("/library?filter=all&sort=title", { mode, storage: createStorageFixtureApi() });

it.each(modes)("shows Add game's menu in the pseudo-locale (%s)", async (mode) => {
  await addGameTo(mode);
  await openAddGame();
  await menuItem(/ƀŷ ţĩţĺē/);
  expect(over()).toEqual([]);
});

it.each(modes)(
  "shows Install from link and its storage in the pseudo-locale (%s)",
  async (mode) => {
    await addGameTo(mode);
    const dialog = await addGame(/ĺĩńķ…/);
    expect(over()).toEqual([]);
    type(dialog.querySelector("input"), "https://example.com/game.zip");
    fireEvent.submit(dialog.querySelector("form") as Element);
    await ready('[data-cs="install-root-picker"] input[type="radio"]');
    expect(over([...paths, /\b(?:Games|External)\b/g, /\bext4\b/g])).toEqual([]);
  },
);

it.each(modes)("shows Add a game on this computer in the pseudo-locale (%s)", async (mode) => {
  await addGameTo(mode);
  const dialog = await addGame(/ţĥĩś ćōḿƥũţēŕ/);
  expect(over()).toEqual([]);
  const [target, title] = dialog.querySelectorAll("input");
  // A shortcut, so the summary has all its rows, under a title already there.
  type(target ?? null, "/home/me/Desktop/celeste.desktop");
  fireEvent.blur(target as Element);
  await ready('[data-cs="manual-game"] [data-part="summary"]');
  type(title ?? null, "Fixture Game 0001");
  fireEvent.click(one('[data-cs="manual-game"] [data-part="add"]') as Element);
  await ready('[data-cs="manual-game"] [data-part="duplicate"]');
  expect(over([...paths, "--windowed"])).toEqual([]);
});

it.each(modes)("shows Add a game by title in the pseudo-locale (%s)", async (mode) => {
  await addGameTo(mode);
  await addGame(/ƀŷ ţĩţĺē/);
  expect(over()).toEqual([]);
});

it.each(modes)("shows Receive a shared game in the pseudo-locale (%s)", async (mode) => {
  await addGameTo(mode);
  const dialog = await addGame(/Ŕēćēĩvē à śĥàŕēď ĝàḿē/);
  expect(over()).toEqual([]);
  type(dialog.querySelector("input"), `${FIXTURE_LINK}hades`);
  fireEvent.click(await within(dialog).findByRole("button", { name: /Ĺōōķ/ }));
  await within(dialog).findByRole("button", { name: /Ŕēćēĩvē/ });
  expect(over([/Alex's Desk|Hades/g, /caskade:\/\/\S+/g])).toEqual([]);
});

it("shows the on-screen keyboard over Add game's field in the pseudo-locale", async () => {
  const app = await addGameTo("console");
  const dialog = await addGame(/ĺĩńķ…/);
  const field = dialog.querySelector("input");
  await until(() => focused() === field, "the link field");
  app.pad.press("confirm");
  await ready('[data-cs="osk"]');
  await ready('[data-cs="hint-bar"] [data-part="hint"][data-action="back"]');
  expect(shown(['[data-cs="osk"]', ...overlays], reactAria, padGlyphs)).toEqual([]);
});

it("shows console mode's folder browser from Add game in the pseudo-locale", async () => {
  await open("/library?filter=all&sort=title", {
    mode: "console",
    wrap: (children: ReactNode) => (
      <BrowseApiContext value={createBrowseFixtureApi()}>{children}</BrowseApiContext>
    ),
  });
  await addGame(/ƒōĺďēŕ…/);
  await ready('[data-cs="folder-browser"] [data-part="entry"]');
  // Files' names, and places the core names (#214).
  expect(over([/^(?:Home|File system|SD Card|Downloads|Emulators|Games|ROMs)$/, ...paths])).toEqual(
    [],
  );
});

// Plugins: their own words are theirs (a language they don't ship shows
// their English); what the app draws around them is not.

const hours: UiPlugin = {
  id: "dev.fixture.hours",
  name: "Hours",
  version: "0.1.0",
  entry: "caskade-plugin://dev.fixture.hours/ui.js",
  contributes: [
    { slot: "game.details.tabs", id: "hours", title: "How long" },
    { slot: "game.details.tabs", id: "broken", title: "Broken" },
    { slot: "game.actions", id: "repack", title: "Repack" },
    { slot: "views", id: "shelf", title: "Shelf" },
    { slot: "views", id: "couch", title: "Couch", modes: ["console"] },
    { slot: "views", id: "desk", title: "Desk", modes: ["desktop"] },
  ],
};

const pluginData = [/Hours|How long|Broken|Repack|Shelf|Couch|Desk|Twelve hours|no pack/g];

const HoursTab = () => <p>Twelve hours</p>;

function Broken(): ReactNode {
  throw new Error("broken on purpose");
}

function pluginRegistry() {
  const context = { plugin: { id: hours.id, name: hours.name } } as unknown as PluginContext;
  const registry = new PluginRegistry();
  registry.register(
    hours,
    "game.details.tabs",
    { id: "hours", title: "How long", component: HoursTab },
    context,
  );
  registry.register(
    hours,
    "game.details.tabs",
    { id: "broken", title: "Broken", component: Broken },
    context,
  );
  const run = () => Promise.reject(new Error("no pack"));
  registry.register(hours, "game.actions", { id: "repack", title: "Repack", run }, context);
  for (const [id, title] of [
    ["shelf", "Shelf"],
    ["couch", "Couch"],
    ["desk", "Desk"],
  ] as const)
    registry.register(hours, "views", { id, title, component: () => <p>{title}</p> }, context);
  return registry;
}

const pluginShown = (selectors: string[]) => shown(selectors, pluginData);

it.each(modes)("shows a plugin's tab on a game's page in the pseudo-locale (%s)", async (mode) => {
  // The crashed tab's placeholder logs what crashed.
  vi.spyOn(console, "error").mockImplementation(() => {});
  await open("/games/fixture-1", { mode, registry: pluginRegistry() });
  await ready('[data-part="plugin-details"] [role="tabpanel"]');
  const around = ['[data-part="plugin-details"]', '[data-part="plugin-nav"]'];
  expect(pluginShown(around)).toEqual([]);

  const tabs = all('[data-part="plugin-tabs"] [role="tab"]');
  fireEvent.click(tabs[1] as Element);
  await ready('[data-cs="plugin-error"]');
  expect(pluginShown(around)).toEqual([]);

  // An action that fails says so in the app's words.
  fireEvent.click(one('[data-part="plugin-actions"] button') as Element);
  await ready('[data-cs="plugin-notices"] [role="alert"]');
  expect(pluginShown(['[data-cs="plugin-notices"]'])).toEqual([]);
});

it.each(modes)("shows a plugin's view in the pseudo-locale (%s)", async (mode) => {
  // Its own, one the other mode hides, and one that isn't there.
  const hidden = mode === "console" ? "desk" : "couch";
  for (const view of ["shelf", hidden, "gone"]) {
    await open(`/plugins/${hours.id}/${view}`, { mode, registry: pluginRegistry() });
    await ready(view === "shelf" ? '[data-view="plugin"]' : 'main [role="status"]');
    expect(pluginShown(["main"])).toEqual([]);
    cleanup();
  }
});

it.each(modes)("shows Add web game in the pseudo-locale (%s)", async (mode) => {
  await open("/library?filter=web", {
    mode,
    games: [webGame("alpha")],
    storage: createStorageFixtureApi(),
  });
  fireEvent.click(await screen.findByRole("button", { name: /Àďď ŵēƀ ĝàḿē/ }));
  await dialogOf("add-web-game");
  const dialog = one('[data-cs="add-web-game"]') as Element;
  expect(over(["https://"])).toEqual([]);
  // Then where to keep the offline copy.
  fireEvent.change(dialog.querySelector('input[type="url"]') as Element, {
    target: { value: "https://example.com/game" },
  });
  fireEvent.click(dialog.querySelector('input[type="checkbox"]') as Element);
  fireEvent.submit(dialog.querySelector("form") as Element);
  await ready('[data-cs="install-root-picker"] input[type="radio"]');
  expect(over([...paths, /\b(?:Games|External)\b/g, /\bext4\b/g])).toEqual([]);
});

/** What the fixture emulators and platforms name, and how the core
 * describes its own variables. */
async function emulationWords() {
  const snapshot = await createEmulationFixtureApi().snapshot();
  return [
    ...strings([
      ...snapshot.emulators.flatMap((emulator) => [
        emulator.name,
        ...emulator.profiles.map(({ config }) =>
          config.type === "custom" ? config.name : config.definitionProfile,
        ),
        ...emulator.definitionProfiles.map((profile) => profile.name),
      ]),
      ...snapshot.definitions.flatMap((definition) => [
        definition.name,
        ...definition.profiles.map((profile) => profile.name),
      ]),
      ...snapshot.platforms.flatMap((platform) => [platform.name, platform.short]),
      ...snapshot.variables.flatMap((variable) => [variable.name, variable.description]),
    ]),
    ...paths,
  ];
}

it("shows a manual scan's review in the pseudo-locale", async () => {
  await open("/settings?tab=emulators&section=scanners");
  fireEvent.click(await screen.findByRole("button", { name: /Śćàń.*SNES collection/ }));
  await ready('[data-cs="rom-review"] [data-part="review-game"]');
  const data = [
    ...(await emulationWords()),
    /Chrono Trigger|Super Metroid|Final Fantasy III|Star Ocean(?: Bonus)?/g,
    /\b(?:USA|Japan|Rev 1|Disc \d)\b/g,
  ];
  expect(shown(['[data-cs="rom-review"]'], data)).toEqual([]);
});

it("shows the emulator profile editor in the pseudo-locale", async () => {
  await open("/settings?tab=emulators");
  fireEvent.click(await screen.findByRole("button", { name: /Àďď ćũśţōḿ ƥŕōƒĩĺē/ }));
  await dialogOf("emulation-profile-editor");
  // The preview shows a sample ROM's command, word by word.
  const data = [
    "Sample Game (USA).rom",
    ...(await emulationWords()),
    /\{\w+\}/g,
    /CASKADE_\w+/g,
    ...reactAria,
  ];
  const editor = '[data-cs="emulation-profile-editor"]';
  expect(shown([editor], data, 'code, [data-part="token"]')).toEqual([]);
});

// Choosing several games.

async function selecting(mode: Mode) {
  await open("/library?filter=all&sort=title", { mode });
  await ready('[data-view="library"] [data-cs="game-card"]');
  fireEvent.click(one('[data-view="library"] [data-part="select"]') as Element);
  await ready('[data-cs="selection-bar"]');
}

const bar = (part: string) => one(`[data-cs="selection-bar"] [data-part="${part}"]`) as Element;

/** Selects what's loaded and waits for the bar to say so. */
async function selectLoaded() {
  const before = bar("count").textContent;
  fireEvent.click(bar("select-loaded"));
  await waitFor(() => expect(bar("count").textContent).not.toBe(before));
}

it.each(modes)("shows the selection bar in the pseudo-locale (%s)", async (mode) => {
  await selecting(mode);
  expect(shown(['[data-cs="selection-bar"]'], [], padGlyphs)).toEqual([]);
  await selectLoaded();
  expect(shown(['[data-cs="selection-bar"]'], [], padGlyphs)).toEqual([]);
});

it.each(modes)("shows the bulk editor's steps in the pseudo-locale (%s)", async (mode) => {
  await selecting(mode);
  await selectLoaded();
  fireEvent.click(bar("edit"));
  await dialogOf("bulk-editor");
  const editor = one('[data-cs="bulk-editor"]') as Element;
  expect(over()).toEqual([]);

  // Every field ticked shows what it changes with.
  for (const box of editor.querySelectorAll('[data-part="field"] input[type="checkbox"]'))
    fireEvent.click(box);
  await waitFor(() => expect(editor.querySelector("textarea")).not.toBeNull());
  expect(over()).toEqual([]);

  const description = editor.querySelector("textarea") as Element;
  type(description, "Changed");
  const apply = () => one('[data-cs="bulk-editor"] [data-part="apply"]') as Element;
  fireEvent.click(apply());
  await ready('[data-cs="bulk-editor"] [data-part="results"]');
  // The games' values, and what the core says about them (#214).
  const data = [...dialogData, "Changed", /\bFixture Game\b/g];
  expect(over(data)).toEqual([]);
  fireEvent.click(apply());
  await ready('[data-cs="bulk-editor"] section[aria-label]');
  await waitFor(() => expect(one('[data-cs="bulk-editor"] [data-part="cancel"]')).toBeNull());
  expect(over(data)).toEqual([]);
});

it.each(modes)("shows Refresh details in the pseudo-locale (%s)", async (mode) => {
  await selecting(mode);
  await selectLoaded();
  fireEvent.click(bar("refresh"));
  await dialogOf("games-refresh-start");
  expect(over()).toEqual([]);
  const dialog = one('[data-cs="games-refresh-start"]') as Element;
  const paragraph = () => dialog.querySelector("p")?.textContent;
  const missing = paragraph();
  fireEvent.click(dialog.querySelectorAll('[role="radio"], input[type="radio"]')[1] as Element);
  await waitFor(() => expect(paragraph()).not.toBe(missing));
  expect(over()).toEqual([]);
  fireEvent.click(dialog.querySelector('[data-part="start"]') as Element);
  await ready('[data-cs="games-refresh-start"] a[href]');
  expect(over()).toEqual([]);
});

it.each(modes)(
  "shows a new and a smart collection's dialog in the pseudo-locale (%s)",
  async (mode) => {
    await open("/collections", { mode });
    fireEvent.click(await screen.findByRole("button", { name: /Ńēŵ ćōĺĺēćţĩōń|Ḿàńũàĺ ĺĩśţ/ }));
    await dialogOf("collection-dialog");
    expect(over(collectionData)).toEqual([]);
    const dialog = one('[data-cs="collection-dialog"]') as Element;
    fireEvent.click(dialog.querySelectorAll('[role="radio"], input[type="radio"]')[1] as Element);
    await ready('[data-cs="collection-dialog"] [data-part="rules"]');
    await waitFor(() =>
      expect(one('[data-cs="collection-dialog"] [data-part="matches"]')?.textContent).toMatch(/\d/),
    );
    expect(over([...collectionData, /\bPC\b/g])).toEqual([]);
  },
);

it.each(modes)("shows Edit Rules on a smart collection in the pseudo-locale (%s)", async (mode) => {
  await open("/collections/collection-favourites", { mode });
  fireEvent.click(await screen.findByRole("button", { name: /Ēďĩţ Ŕũĺēś/ }));
  await dialogOf("collection-dialog");
  await ready('[data-cs="collection-dialog"] [data-part="rules"]');
  expect(over(collectionData)).toEqual([]);
});

it.each(modes)("shows Add Games on a collection in the pseudo-locale (%s)", async (mode) => {
  await open("/collections/collection-weekend", { mode });
  fireEvent.click(await screen.findByRole("button", { name: /Àďď Ĝàḿēś/ }));
  await dialogOf("add-games");
  await ready('[data-cs="add-games"] [data-part="games"] input[type="checkbox"]');
  expect(over(collectionData)).toEqual([]);
  // Nothing found.
  type(one('[data-cs="add-games"] input'), "nothing-like-it");
  await waitFor(() =>
    expect(one('[data-cs="add-games"] [data-part="games"] input[type="checkbox"]')).toBeNull(),
  );
  await waitFor(() => expect(one('[data-cs="add-games"] [data-part="games"] p')).not.toBeNull());
  expect(over([...collectionData, "nothing-like-it"])).toEqual([]);
});

it("shows a Select's list in the pseudo-locale", async () => {
  await open("/library?filter=all&sort=title");
  await ready('[data-view="library"] [data-cs="select"] button');
  fireEvent.click(one('[data-view="library"] [data-cs="select"] button') as Element);
  await dialogOf("select-popover");
  await ready('[data-cs="select-popover"] [role="option"]');
  expect(shown(['[data-cs="select-popover"]'], reactAria)).toEqual([]);
});
