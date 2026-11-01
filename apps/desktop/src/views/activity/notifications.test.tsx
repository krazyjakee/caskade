/** #198: the bell's unread count, and Activity's notification feed with a
 * mouse, the keyboard and a pad. */
import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { fixtureJob } from "../../api/installFixtures";
import { NotificationsProvider } from "../../api/notifications";
import {
  createNotificationsFixtureApi,
  fixtureNotification,
  fixtureNotifications,
} from "../../api/notificationsFixtures";
import { fakeLayout, fakeViewport, renderApp } from "../../test/harness";
import { timeRange } from "./NotificationFeed";

vi.setConfig({ testTimeout: 20_000 });

let undoLayout = () => {};
beforeEach(() => {
  undoLayout = fakeLayout();
  fakeViewport();
});
afterEach(() => {
  undoLayout();
  cleanup();
});

function renderFeed(
  path: string,
  api = createNotificationsFixtureApi(),
  options: Parameters<typeof renderApp>[1] = {},
) {
  const app = renderApp(path, {
    ...options,
    wrap: (children) => <NotificationsProvider api={api}>{children}</NotificationsProvider>,
  });
  return { ...app, notifications: api };
}

const heading = (name: string) => screen.findByRole("heading", { name });
const card = async (name: string) =>
  (await heading(name)).closest('[data-cs="notification"]') as HTMLElement;

it("the bell counts unread notifications apart from jobs and opens the feed", async () => {
  const app = renderFeed("/");
  const bell = await screen.findByRole("link", {
    name: "Activity, 1 active job, 2 unread notifications",
  });
  expect(bell.querySelector('[data-part="job-count"]')?.textContent).toBe("1");
  await waitFor(() =>
    expect(bell.querySelector('[data-part="notification-count"]')?.textContent).toBe("2"),
  );
  fireEvent.click(bell);
  await heading("10 games added");
  expect(app.router.state.location.search).toMatchObject({ tab: "notifications" });
});

it("the bell still opens the jobs while one needs an answer", async () => {
  const app = renderFeed("/", undefined, {
    jobs: [fixtureJob({ id: "waiting", status: "needs_user" })],
  });
  const bell = await screen.findByRole("link", { name: /1 needs your attention, 2 unread/ });
  fireEvent.click(bell);
  await waitFor(() => expect(app.router.state.location.search).toMatchObject({ tab: "jobs" }));
  await screen.findByText("Fixture installation");
});

it("showing the feed reads nothing; opening a batch reads what was shown and lists its games", async () => {
  const app = renderFeed("/activity?tab=notifications");
  const batch = await card("10 games added");
  expect(batch.getAttribute("data-status")).toBe("unread");
  expect(within(batch).getByText(/RomM 6 · Steam 4/)).toBeTruthy();
  expect(await app.notifications.unread()).toBe(2);

  const markRead = vi.spyOn(app.notifications, "markRead");
  fireEvent.click(within(batch).getByRole("button", { name: "Show games" }));
  await waitFor(() => expect(markRead).toHaveBeenCalledWith("batch", 1));
  await waitFor(() =>
    expect(app.router.state.location.search).toMatchObject({ tab: "notifications", open: "batch" }),
  );
  const members = await within(batch).findAllByRole("listitem");
  expect(members).toHaveLength(10);
  // A game merged since links to the game it joined; a removed one says so.
  expect(within(batch).getByRole("link", { name: /Fixture Game 2 \(now part of/ })).toBeTruthy();
  expect(within(batch).getByText(/no longer in the library/)).toBeTruthy();
  await waitFor(() => expect(batch.getAttribute("data-status")).toBe("read"));

  // More games join the batch: unread again, with the new count.
  act(() => app.notifications.arrive("batch", 2));
  const grown = await card("12 games added");
  await waitFor(() => expect(grown.getAttribute("data-status")).toBe("unread"));
});

it("a failed update opens its game, and Back returns to the feed", async () => {
  const app = renderFeed("/activity?tab=notifications");
  const failed = await card("Celeste 1.4 couldn't update");
  expect(failed.getAttribute("data-severity")).toBe("error");
  fireEvent.click(within(failed).getByRole("button", { name: "Open game" }));
  await waitFor(() => expect(app.router.state.location.pathname).toBe("/games/celeste"));
  expect(app.router.state.location.search).toMatchObject({ from: "/activity?tab=notifications" });
  expect(await app.notifications.unread()).toBe(1);
});

it("filters unread, marks one unread, marks all read and pages", async () => {
  const items = [
    ...fixtureNotifications(),
    ...Array.from({ length: 22 }, (_, i) =>
      fixtureNotification({
        id: `old-${i}`,
        title: `Old batch ${i}`,
        version: 10 + i,
        unread: false,
        updatedAtMs: i,
      }),
    ),
  ];
  const app = renderFeed("/activity?tab=notifications", createNotificationsFixtureApi(items));
  await heading("10 games added");
  expect(screen.getByText("1–20 of 25")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Older" }));
  await screen.findByText("21–25 of 25");
  await heading("Old batch 0");

  fireEvent.click(screen.getByRole("link", { name: "Unread" }));
  await waitFor(() => expect(app.router.state.location.search).toMatchObject({ unread: true }));
  await heading("10 games added");
  expect(screen.queryByRole("heading", { name: "Plugin dev.hltb was turned off" })).toBeNull();

  fireEvent.click(screen.getByRole("button", { name: "Mark all read" }));
  await screen.findByText("No unread notifications.");
  expect(await app.notifications.unread()).toBe(0);

  fireEvent.click(screen.getByRole("link", { name: "All" }));
  const plugin = await card("Plugin dev.hltb was turned off");
  fireEvent.click(
    within(plugin).getByRole("button", { name: "Mark unread: Plugin dev.hltb was turned off" }),
  );
  await waitFor(() => expect(plugin.getAttribute("data-status")).toBe("unread"));
  expect(await app.notifications.unread()).toBe(1);
});

it("says when the feed can't load, and tries again", async () => {
  const api = createNotificationsFixtureApi();
  const list = vi.spyOn(api, "list").mockRejectedValue(new Error("database is locked"));
  renderFeed("/activity?tab=notifications", api);
  const alert = await screen.findByRole("alert", {}, { timeout: 5000 });
  expect(alert.textContent).toContain("database is locked");
  list.mockRestore();
  fireEvent.click(within(alert).getByRole("button", { name: "Try again" }));
  await heading("10 games added");
});

it("a pad switches to the feed with RB, opens a batch and marks an item read", async () => {
  const app = renderFeed("/activity", undefined, { mode: "console" });
  await screen.findByText("Fixture installation");
  act(() => screen.getByRole("tab", { name: /^Jobs/ }).focus());
  app.pad.press("tab.next");
  await waitFor(() =>
    expect(app.router.state.location.search).toMatchObject({ tab: "notifications" }),
  );
  await heading("10 games added");
  const showGames = screen.getByRole("button", { name: "Show games" });
  for (let i = 0; i < 40 && document.activeElement !== showGames; i++) {
    app.pad.press("nav.down");
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
  expect(document.activeElement).toBe(showGames);
  app.pad.press("confirm");
  await waitFor(() => expect(app.router.state.location.search).toMatchObject({ open: "batch" }));
  expect(await app.notifications.unread()).toBe(1);
  app.pad.press("tab.prev");
  await waitFor(() => expect(app.router.state.location.search).toMatchObject({ tab: "jobs" }));
});

it("labels a batch with when its additions happened, not 'the last hour'", () => {
  const at = Date.UTC(2026, 9, 28, 14, 5);
  expect(timeRange(at, at)).not.toContain("–");
  expect(timeRange(at, at + 45 * 60_000)).toMatch(/–/);
  expect(timeRange(at, at + 2 * 86_400_000)).toMatch(/ – /);
});
