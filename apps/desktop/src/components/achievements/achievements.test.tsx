import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { type AchievementSet, AchievementsProvider } from "../../api/achievements";
import {
  createAchievementsFixture,
  emptySet,
  retroSet,
  staleGogSet,
  steamSet,
} from "../../api/achievementsFixtures";
import { renderApp } from "../../test/harness";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const GAME = "fixture-1";

function render(sets: AchievementSet[], mode: "desktop" | "console" = "desktop") {
  const api = createAchievementsFixture({ [GAME]: sets });
  const app = renderApp(`/games/${GAME}`, {
    mode,
    wrap: (children) => <AchievementsProvider api={api}>{children}</AchievementsProvider>,
  });
  return { api, app };
}

const panel = () => screen.findByRole("region", { name: "Achievements" });
const list = (section: HTMLElement) => within(section).getByRole("list", { name: "Achievements" });

it("shows each store account's set with real counts, attribution and per-achievement state", async () => {
  render([steamSet(240), staleGogSet(), retroSet()]);
  const section = await panel();
  // The hero's summary counts every set: 60 + 5 + 10 of 240 + 12 + 30.
  expect(screen.getByRole("button", { name: /75 of 282 achievements unlocked/ })).toBeTruthy();
  expect(within(section).getByText("Steam account Ada", { exact: false })).toBeTruthy();
  expect(within(section).getByText("60 of 240 unlocked (25%)")).toBeTruthy();
  // Hundreds of rows, each reachable.
  expect(within(list(section)).getAllByRole("listitem")).toHaveLength(240);
  const first = within(list(section)).getAllByRole("listitem")[0] as HTMLElement;
  expect(first.dataset.state).toBe("unlocked");
  expect(first.textContent).toContain("Unlocked");
  expect(first.textContent).toContain("0% of players");
  // Progress where the service reports it.
  const progressing = within(list(section)).getByLabelText("Achievement 6, locked");
  expect(within(progressing).getByText("20/50")).toBeTruthy();

  // Another account's set: GOG, last refresh failed, so it says what it shows.
  fireEvent.click(within(section).getByRole("radio", { name: "GOG · ada_gog" }));
  expect(
    within(section).getByText(
      /Showing achievements from .* The last refresh failed: Caskade couldn't reach gog.com./,
    ),
  ).toBeTruthy();
  expect(within(section).getByText("5 of 12 unlocked (42%)")).toBeTruthy();

  // RetroAchievements: hardcore apart from softcore, with points.
  fireEvent.click(within(section).getByRole("radio", { name: "RetroAchievements · ada_ra" }));
  expect(
    within(section).getByText(/10 of 30 unlocked \(33%\) · 4 in hardcore · 150 of 450 points/),
  ).toBeTruthy();
  const rows = within(list(section)).getAllByRole("listitem");
  expect(rows[0]?.dataset.hardcore).toBe("true");
  expect(rows[0]?.textContent).toContain("Hardcore");
  expect(rows[5]?.textContent).toContain("Softcore only");
  expect(rows[20]?.textContent).toContain("Locked");
});

it("keeps hidden achievements concealed until unlocked or revealed", async () => {
  render([steamSet(30)]);
  const section = await panel();
  // ACH_3 is hidden and locked; ACH_24 is hidden and unlocked.
  const concealed = within(list(section))
    .getAllByRole("listitem")
    .filter((row) => row.dataset.concealed === "true");
  expect(concealed.length).toBeGreaterThan(0);
  expect(section.textContent).not.toContain("Secret ending 3");
  expect(section.textContent).not.toContain("The spoiler of secret 3.");
  expect(section.textContent).toContain("Secret ending 24");
  fireEvent.click(within(concealed[0] as HTMLElement).getByRole("button", { name: "Reveal" }));
  expect(section.textContent).toContain("Secret ending 3");
  expect(section.textContent).not.toContain("Secret ending 10");
  fireEvent.click(within(section).getByRole("switch", { name: /Show \d+ hidden achievements/ }));
  expect(section.textContent).toContain("Secret ending 10");
});

it("tells apart every state, with what to do about each", async () => {
  const { app } = render([
    emptySet("not_fetched", "steam", "Steam", "New"),
    emptySet("no_achievements", "steam", "Steam", "None"),
    emptySet("unsupported", "gog", "GOG", "Gog"),
    emptySet("private", "steam", "Steam", "Shy"),
    emptySet("sign_in_needed", "steam", "Steam", "Gone"),
  ]);
  const section = await panel();
  const pick = (name: string) =>
    fireEvent.click(within(section).getByRole("radio", { name: new RegExp(`· ${name}$`) }));
  expect(section.dataset.state).toBe("not_fetched");
  expect(within(section).getByText(/Not fetched yet/)).toBeTruthy();
  pick("None");
  expect(within(section).getByText("Steam lists no achievements for this game.")).toBeTruthy();
  pick("Gog");
  expect(within(section).getByText("GOG doesn't offer this game's achievements.")).toBeTruthy();
  pick("Shy");
  expect(within(section).getByText(/Shy's achievements are private on Steam/)).toBeTruthy();
  pick("Gone");
  expect(section.dataset.state).toBe("sign_in_needed");
  fireEvent.click(within(section).getByRole("button", { name: "Open store accounts" }));
  await waitFor(() => expect(app.router.state.location.search).toMatchObject({ tab: "stores" }));
});

it("refreshes, reports a stale answer offline, and stays quiet without sets", async () => {
  const { api } = render([steamSet(8)]);
  const section = await panel();
  expect(within(section).getByText("2 of 8 unlocked (25%)")).toBeTruthy();
  fireEvent.click(within(section).getByRole("button", { name: "Refresh achievements" }));
  await within(section).findByText("3 of 8 unlocked (38%)");
  expect(api.refreshes).toEqual([GAME]);
  api.setOffline(true);
  fireEvent.click(within(section).getByRole("button", { name: "Refresh achievements" }));
  await within(section).findByText(/The last refresh failed: Caskade is offline./);
  // Still the last answer.
  expect(within(section).getByText("3 of 8 unlocked (38%)")).toBeTruthy();
  cleanup();

  render([]);
  await screen.findByRole("heading", { level: 1 });
  await act(async () => {});
  expect(screen.queryByRole("region", { name: "Achievements" })).toBeNull();
});

it("a pad reaches the controls, every row and Reveal, without hover", async () => {
  const { app } = render([steamSet(12), retroSet()], "console");
  const section = await panel();
  const focus = (element: HTMLElement) => act(() => element.focus());
  // The hero's summary moves focus to the panel.
  fireEvent.click(screen.getByRole("button", { name: /achievements unlocked/ }));
  expect(document.activeElement?.id).toBe("achievements-heading");
  // Rows are stops, labelled with their state; a hidden one's stop is Reveal.
  const rows = within(list(section)).getAllByRole("listitem");
  expect(rows.filter((row) => row.tabIndex === 0)).toHaveLength(rows.length - 2);
  expect(rows[3]?.tabIndex).toBe(-1);
  focus(within(rows[3] as HTMLElement).getByRole("button", { name: "Reveal" }));
  app.pad.press("confirm");
  expect(section.textContent).toContain("Secret ending 3");
  focus(within(section).getByRole("button", { name: "Refresh achievements" }));
  app.pad.press("confirm");
  await within(section).findByText("4 of 12 unlocked (33%)");
});
