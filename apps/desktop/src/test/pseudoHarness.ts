/** The views' own content in the pseudo-locale (#9): Home, the Library's
 * filters (populated, empty, by platform and the Web mosaic) in both
 * layouts and its quick look, a game's page, the Store and a catalog game's
 * page, Collections and a collection, Activity's jobs and notifications,
 * and the Settings tabs the settings tests leave out (Scripts, Store
 * accounts, Streaming, Backup). Then what opens over them: console mode's
 * quick look, a game's menu, Add game and its dialogs, the game editor's
 * tabs, the move, uninstall and remove dialogs, and what the app draws
 * around a plugin's tabs and views. Then what stands in for them or comes up
 * on its own: signing in, the guide menu and the curtain, every question
 * Play can ask, install requests, selection and the bulk editor, the search
 * overlays, pairing, and what shows when something fails. Views that differ
 * by shell are checked in both. Every text, aria-label, title and
 * placeholder they show comes from a catalog; what the fixtures name is
 * data. Each view is checked once its data has loaded: a check that may pass
 * early would pass on a view still loading.
 *
 * The suites share this harness; importing it registers each test's setup
 * and teardown. */
import { loadCatalog, pseudoLocale } from "@caskade/i18n";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, vi } from "vitest";
import { createFixtureGames } from "../api/fixtures";
import type { SaveVersion } from "../api/saves";
import { createStorageFixtureApi } from "../api/storageFixtures";
import type { GameDetails } from "../api/types";
import type { Mode } from "../shell/console/mode";
import { type AppOptions, fakeViewport, renderApp } from "./harness";
import { untranslated } from "./untranslated";

beforeEach(() => {
  fakeViewport();
  vi.stubGlobal("scrollTo", vi.fn());
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

export const locale = pseudoLocale ?? "en-XA";
export const modes: Mode[] = ["desktop", "console"];

const MONTH = "(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)";

/** Data every view may show: the brand, the fixture library's games, and
 * what the pseudo-locale formats as English (Intl knows en-XA as English):
 * dates, times, relative days and sizes. */
const common: (RegExp | string)[] = [
  /CASKADE|Caskade/g,
  /IGDB/g,
  "Original development fixture. No personal library data.",
  "Development library",
  /Fixture Game \d+/g,
  // The fixture games' tag and source.
  /\bFixture\b/gi,
  /\b(?:Adventure|Strategy|Puzzle)\b/g,
  /\bPC\b/g,
  new RegExp(
    `\\b${MONTH}(?![a-z])(?: \\d{1,2})?,? ?(?:\\d{4})?(?:, \\d{1,2}:\\d{2}\\s?[AP]M)?`,
    "g",
  ),
  /\b(?:Mon|Tues|Wednes|Thurs|Fri|Satur|Sun)day\b/g,
  /\b[AP]M\b/g,
  /\b(?:[Tt]oday|[Yy]esterday|[Ll]ast week|\d+ (?:days|weeks) ago)\b/g,
  /\d+(?:[.,]\d+)?\s?(?:kB|MB|GB|TB|PB)\b/g,
];

/** Devices paired through sync, as the fixtures name them. */
export const devices =
  /Den-PC|Steam Deck|Office laptop|Work PC|Living room|Kids' PC|Study|Deck|Bedroom TV/g;

/** Paths, file names and extensions. `~/`, not a bare `~`: the
 * pseudo-locale pads its text with tildes. */
export const paths = [/(?:~\/|\/)[^\s,·()[\]]*/g, /\.\w{2,5}\b/g];

export const strings = (values: (string | null | undefined)[]) =>
  values.filter((value): value is string => Boolean(value));

export async function open(path: string, options: Omit<AppOptions, "locale" | "messages"> = {}) {
  return renderApp(path, { locale, messages: await loadCatalog(locale), ...options });
}

export const one = (selector: string) => document.querySelector(selector);
export const all = (selector: string) => document.querySelectorAll(selector);

/** Waits until `selector` matches something. */
export const ready = (selector: string) => waitFor(() => expect(one(selector)).not.toBeNull());

/** English left in what `selectors` match. */
export const shown = (selectors: string[], data: (RegExp | string)[] = [], skip?: string) =>
  untranslated(
    selectors.flatMap((selector) => [...all(selector)]),
    { data: [...data, ...common], skip },
  );

/** A web game, as the Web tab lists them. */
export function webGame(id: string, overrides: Partial<GameDetails> = {}): GameDetails {
  const [template] = createFixtureGames(1);
  if (!template) throw new Error("no fixture");
  return {
    ...template,
    id,
    title: `Web ${id}`,
    entries: [
      {
        id: `${id}-entry`,
        source: "web",
        state: "installed",
        location: null,
        canLaunch: true,
        emulated: false,
      },
    ],
    ...overrides,
  };
}

// Collections.

/** The fixture collections' names and descriptions. */
export const collectionData = [
  "Favourites",
  "The ones worth coming back to.",
  "Weekend Picks",
  "Short list for when there's finally time.",
];

// What opens over the views. These sit in portals, outside every view.

/** Menus and dialogs, and console mode's hints for them. */
export const overlays = [
  '[role="menu"]',
  '[role="dialog"]',
  '[role="alertdialog"]',
  '[data-cs="hint-bar"]',
];

/** Not Caskade's text. Pad buttons keep the names printed on them
 * (`glyphs.ts`), and these come from React Aria's own catalogs, which
 * `LocaleProvider` gives the app's language (`ariaLocale.test.tsx`) but
 * which have no pseudo-locale: an empty Select, a search box's list and a
 * popover's hidden dismiss button. */
export const padGlyphs = '[data-part="glyph"]';
export const reactAria = ["Select an item", "Suggestions", "Dismiss"];

export const over = (data: (RegExp | string)[] = []) =>
  shown(overlays, [...data, ...reactAria], padGlyphs);

export const rightClick = (element: Element) =>
  fireEvent.contextMenu(element, { clientX: 120, clientY: 80, button: 2 });

/** The open menu's item named `name`, as pseudo text. */
export const menuItem = async (name: RegExp) =>
  within(await screen.findByRole("menu")).findByRole("menuitem", { name });

/** The game the storage fixture installed, and one with no entries. */
export function managedGames(): GameDetails[] {
  return createFixtureGames(2).map((game, index) =>
    index === 1
      ? {
          ...game,
          status: "installed",
          entries: [
            {
              id: "entry-1",
              source: "caskade-storage",
              state: "installed",
              location: "/fixture/Games/Fixture Game 0002",
              canLaunch: true,
              emulated: false,
            },
          ],
        }
      : game,
  );
}

export const managed = (mode: Mode = "desktop") =>
  open("/games/fixture-1", { mode, games: managedGames(), storage: createStorageFixtureApi() });

export const gameHeading = () =>
  screen.findByRole("heading", { level: 1, name: "Fixture Game 0002" });

// A game's menu.

/** Opens the managed game's menu and waits for the items that need its
 * details and the other devices. */
export async function gameMenu() {
  rightClick(await gameHeading());
  await menuItem(/Ĥĩďē ƒŕōḿ ĺĩƀŕàŕŷ/);
  await menuItem(/Śĥàŕē ŵĩţĥ śōḿēōńē/);
}

export const dialogData = ["/fixture/Games/Fixture Game 0002", ...paths, devices];

export const type = (field: Element | null, value: string) =>
  fireEvent.change(field as Element, { target: { value } });

export const dialogOf = (name: string) => ready(`[data-cs="${name}"]`);

/** A save made on another device, and one made here. */
export const version = (id: string, local: boolean, hour: number): SaveVersion => ({
  id,
  gameId: "fixture-1",
  accountId: "",
  deviceId: local ? "here" : "b".repeat(64),
  sessionId: null,
  createdAtMs: Date.UTC(2026, 10, 2, hour),
  contentHash: `sha256:${id}`,
  sizeBytes: 4096,
  fileCount: 1,
  parents: [],
  reason: "session",
  local,
});

export const there = version("there", false, 18);
export const here = version("here", true, 17);
