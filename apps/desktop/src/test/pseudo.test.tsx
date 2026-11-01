/** Both shells, their menus and the game menu in the pseudo-locale (#215):
 * every text, aria-label, title and placeholder they show comes from a
 * catalog. Views' own content is checked by their issues (#216, #217). */
import { loadCatalog, pseudoLocale } from "@caskade/i18n";
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { createFixtureGames } from "../api/fixtures";
import { renderApp } from "./harness";
import { untranslated as findUntranslated } from "./untranslated";

afterEach(cleanup);

const locale = pseudoLocale ?? "en-XA";

/** Text that is data, not interface: the brand, fixture names and titles,
 * and the clock's AM/PM. */
const data = [/CASKADE/g, /Player/g, /Fixture Game \d+/g, /\b[AP]M\b/g];

const untranslated = (roots: Iterable<Element | null>) => findUntranslated(roots, { data });

const all = (selector: string) => document.querySelectorAll(selector);
const one = (selector: string) => document.querySelector(selector);

it("shows the desktop shell and its menus in the pseudo-locale", async () => {
  renderApp("/games/fixture-0", {
    locale,
    messages: await loadCatalog(locale),
    games: createFixtureGames(2),
  });
  await screen.findByRole("link", { name: /^\[.*Ĺĩƀŕàŕŷ.*\]$/ });
  const shell = () => [
    one('[data-cs="sidebar"]'),
    one('[data-cs="topbar"]'),
    one('[data-cs="status-bar"]'),
  ];
  await waitFor(() => expect(untranslated(shell())).toEqual([]));

  // The title bar's own menu.
  fireEvent.contextMenu(one('[data-cs="topbar"]') as Element, {
    clientX: 400,
    clientY: 10,
    button: 2,
  });
  await screen.findByRole("menu");
  expect(untranslated(all('[role="menu"]'))).toEqual([]);
  fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());

  // The account menu.
  fireEvent.click(one('[data-cs="topbar"] [data-part="avatar"]') as Element);
  await screen.findByRole("menu");
  expect(untranslated([...all('[role="menu"]'), one('[data-cs="account-menu"]')])).toEqual([]);
  fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());

  // A game's menu.
  const heading = await screen.findByRole("heading", { level: 1, name: "Fixture Game 0001" });
  fireEvent.contextMenu(heading, { clientX: 120, clientY: 80, button: 2 });
  await screen.findByRole("menu");
  expect(untranslated(all('[role="menu"]'))).toEqual([]);
});

it("shows the console shell and its hints in the pseudo-locale", async () => {
  renderApp("/", { mode: "console", locale, messages: await loadCatalog(locale) });
  await waitFor(() =>
    expect(all('[data-cs="hint-bar"] [data-part="hint"]').length).toBeGreaterThan(0),
  );
  const shell = () => [
    one('[data-cs="console-sidebar"]'),
    one('[data-cs="console-topbar"]'),
    one('[data-cs="hint-bar"]'),
  ];
  await waitFor(() => expect(untranslated(shell())).toEqual([]));
});
