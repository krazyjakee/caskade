import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createFixtureGames } from "../../api/fixtures";
import { fakeViewport, renderApp } from "../../test/harness";

beforeEach(() => {
  fakeViewport();
  vi.stubGlobal("scrollTo", vi.fn());
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const card = (title: string) => screen.getByRole("link", { name: new RegExp(`^${title}`) });

async function refreshThree() {
  await screen.findAllByText("Fixture Game 0001");
  fireEvent.click(screen.getByRole("button", { name: "Select" }));
  for (const title of ["Fixture Game 0001", "Fixture Game 0002", "Fixture Game 0003"])
    fireEvent.click(card(title));
  const bar = screen.getByRole("toolbar", { name: "Selected games" });
  fireEvent.click(within(bar).getByRole("button", { name: "Refresh details…" }));
  return screen.findByRole("dialog", { name: "Refresh 3 games' details" });
}

it("refreshes chosen games, reports in Activity and retries the failed ones", async () => {
  const games = createFixtureGames(4);
  const [, second] = games;
  if (second) second.id = "fixture-fail-1";
  const app = renderApp("/library?filter=all&sort=title", { games });
  const start = vi.spyOn(app.library.edits, "refreshStart");
  const dialog = await refreshThree();
  fireEvent.click(within(dialog).getByRole("radio", { name: "Refresh automatic details" }));
  fireEvent.click(within(dialog).getByRole("button", { name: "Refresh" }));
  await within(dialog).findByRole("link", { name: "Follow it in Activity" });
  expect(start).toHaveBeenCalledWith(["fixture-0", "fixture-fail-1", "fixture-2"], "automatic");
  fireEvent.click(within(dialog).getByRole("link", { name: "Follow it in Activity" }));
  const progress = await screen.findByRole("region", { name: "Refreshing game details" });
  expect(progress.dataset.status).toBe("running");
  act(() => app.library.edits.finishRefresh());
  await waitFor(() => expect(progress.dataset.status).toBe("done"));
  expect(within(progress).getByText("2 updated")).toBeTruthy();
  expect(within(progress).getByText("1 failed")).toBeTruthy();
  fireEvent.click(within(progress).getByRole("button", { name: "Retry the failed game" }));
  await waitFor(() => expect(start).toHaveBeenLastCalledWith(["fixture-fail-1"], "automatic"));
});

it("cancels a refresh and continues with the games left", async () => {
  const app = renderApp("/library?filter=all&sort=title", { games: createFixtureGames(4) });
  const start = vi.spyOn(app.library.edits, "refreshStart");
  const dialog = await refreshThree();
  fireEvent.click(within(dialog).getByRole("button", { name: "Refresh" }));
  fireEvent.click(await within(dialog).findByRole("link", { name: "Follow it in Activity" }));
  const progress = await screen.findByRole("region", { name: "Refreshing game details" });
  fireEvent.click(within(progress).getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(progress.dataset.status).toBe("cancelled"));
  fireEvent.click(within(progress).getByRole("button", { name: "Continue with the 3 games left" }));
  await waitFor(() =>
    expect(start).toHaveBeenLastCalledWith(["fixture-0", "fixture-1", "fixture-2"], "missing"),
  );
});
