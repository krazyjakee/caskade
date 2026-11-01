/** #151: Settings, Activity and the Emulation panel with a pad only. Walks
 * press through whole views, so they get more time than a unit test. */
import { cleanup, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.setConfig({ testTimeout: 30_000 });

import { fixtureJob } from "../api/installFixtures";
import { settingsTabs } from "../views/SettingsView";
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

type App = ReturnType<typeof renderApp>;
const main = () => document.querySelector<HTMLElement>('[data-focus-section="main"]');

/** Presses down until `found` holds for the focused element; null when
 * focus stops moving first. */
async function walkTo(app: App, found: (element: HTMLElement) => boolean, limit = 80) {
  for (let i = 0; i < limit; i++) {
    const at = focused();
    if (at && at !== document.body && found(at)) return at;
    app.pad.press("nav.down");
    await new Promise((resolve) => setTimeout(resolve, 0));
    if (focused() === at && i > 0) return null;
  }
  return null;
}

/** Every control a user could operate in the view. */
function interactive(within: HTMLElement): HTMLElement[] {
  return Array.from(
    within.querySelectorAll<HTMLElement>(
      'a[href], button, input, select, textarea, [role="switch"], [role="tab"], [role="option"], [role="slider"]',
    ),
  ).filter(
    (element) =>
      !(element as HTMLButtonElement).disabled &&
      element.getAttribute("aria-disabled") !== "true" &&
      !element.closest("[hidden], [inert], [aria-hidden='true']") &&
      element.getAttribute("tabindex") !== "-1",
  );
}

/** Down from the top of the view to its end; everything focus visited. */
async function walk(app: App): Promise<Set<HTMLElement>> {
  const visited = new Set<HTMLElement>();
  const first = interactive(main() as HTMLElement)[0];
  first?.focus();
  for (let i = 0; i < 400; i++) {
    const at = focused();
    if (at) visited.add(at);
    app.pad.press("nav.down");
    await new Promise((resolve) => setTimeout(resolve, 0));
    if (focused() === at) break;
  }
  return visited;
}

const switchable = (element: HTMLElement) =>
  element instanceof HTMLInputElement &&
  element.type === "checkbox" &&
  !element.disabled &&
  Boolean(element.closest('[role="tabpanel"]'));

/** The settings namespace each tab's first switch writes. */
const writes: Partial<Record<string, string>> = {
  general: "core.general",
  "library-discovery": "core.discovery",
  launchers: "core.launchers",
  appearance: "core.appearance",
  storage: "core.storage",
  installer: "core.installer",
  stores: "core.storeAccounts",
  backup: "core.backups",
};

it("RB walks every Settings tab, and a switch in each changes a setting that persists", async () => {
  const app = renderApp("/settings?tab=general", { mode: "console" });
  await screen.findByRole("heading", { name: "Settings" });
  for (const [index, tab] of settingsTabs.entries()) {
    if (index > 0) app.pad.press("tab.next");
    await waitFor(() => expect(app.router.state.location.search).toMatchObject({ tab: tab.id }));
    const namespace = writes[tab.id];
    if (!namespace) continue;
    await waitFor(() => expect(document.querySelector('[role="tabpanel"] input')).not.toBeNull());
    const control = await walkTo(app, switchable);
    expect(control, `a switch in ${tab.label}`).not.toBeNull();
    const before = (control as HTMLInputElement).checked;
    const count = app.settings.writes.length;
    app.pad.press("confirm");
    await waitFor(() => expect(app.settings.writes.length).toBeGreaterThan(count));
    expect(app.settings.writes.at(-1)).toBe(namespace);
    await waitFor(() => expect((control as HTMLInputElement).checked).toBe(!before));
  }
  // RB wraps around to the first tab.
  app.pad.press("tab.next");
  await waitFor(() => expect(app.router.state.location.search).toMatchObject({ tab: "general" }));
});

const views = [
  ...settingsTabs.map((tab) => `/settings?tab=${tab.id}`),
  "/settings?tab=emulators&section=scanners",
  "/settings?tab=emulators&section=platforms",
  "/activity",
];

it.each(views)("every control in %s is reached by direction", async (path) => {
  const app = renderApp(path, {
    mode: "console",
    jobs: [fixtureJob(), fixtureJob({ id: "waiting", status: "needs_user", question })],
  });
  await waitFor(() => expect(interactive(main() as HTMLElement).length).toBeGreaterThan(0));
  // Let the view's data settle.
  await new Promise((resolve) => setTimeout(resolve, 50));
  const visited = await walk(app);
  const missed = interactive(main() as HTMLElement).filter((element) => !visited.has(element));
  expect(missed.map((element) => element.outerHTML.slice(0, 120))).toEqual([]);
});

const question = {
  id: "7",
  kind: "launch_target",
  title: "Choose a launch target",
  message: "Several executable files were found.",
  choices: [
    { id: "0", label: "Fixture game", detail: "Game.exe" },
    { id: "1", label: "Level editor", detail: "Editor.exe" },
  ],
};

it("answers an install question in Activity", async () => {
  const app = renderApp("/activity", {
    mode: "console",
    jobs: [fixtureJob({ id: "waiting", status: "needs_user", question })],
  });
  await screen.findByText("Fixture installation");
  await walkTo(app, (element) => element.textContent === "Review choices");
  app.pad.press("confirm");
  await screen.findByRole("dialog");
  // Into the choices, pick the second, then Continue.
  const second = await walkTo(app, (element) => element.getAttribute("type") === "radio");
  expect(second).not.toBeNull();
  app.pad.press("nav.down");
  app.pad.press("confirm");
  await waitFor(() =>
    expect((screen.getByRole("radio", { name: /Level editor/ }) as HTMLInputElement).checked).toBe(
      true,
    ),
  );
  await walkTo(app, (element) => element.textContent === "Continue");
  app.pad.press("confirm");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  const [job] = await app.installs.listJobs();
  expect(job?.status).toBe("running");
});

it("reviews a ROM scan and imports it", async () => {
  const app = renderApp("/settings?tab=emulators&section=scanners", { mode: "console" });
  await screen.findByRole("button", { name: "SNES collection" });
  await walkTo(app, (element) => element.getAttribute("aria-label") === "Scan SNES collection");
  app.pad.press("confirm");
  const region = await screen.findByRole("region", { name: /Review scan results/ });
  const offered = Number(region.textContent?.match(/Import (\d+) games/)?.[1]);
  expect(offered).toBeGreaterThan(1);
  // Leave Super Metroid out, then import the rest.
  const metroid = await walkTo(
    app,
    (element) =>
      element.getAttribute("type") === "checkbox" &&
      element.closest("label")?.textContent?.trim() === "Import" &&
      Boolean(element.closest("li")?.textContent?.includes("Super Metroid")),
  );
  expect(metroid).not.toBeNull();
  app.pad.press("confirm");
  await waitFor(() => expect((metroid as HTMLInputElement).checked).toBe(false));
  const importing = await walkTo(app, (element) =>
    /^Import \d+ games$/.test(element.textContent ?? ""),
  );
  expect(importing?.textContent).toBe(`Import ${offered - 1} games`);
  app.pad.press("confirm");
  await screen.findByText(new RegExp(`Last scan: ${offered - 1} added`));
});

it("backs up now with a pad", async () => {
  const app = renderApp("/settings?tab=backup", { mode: "console" });
  await screen.findByRole("button", { name: "Back up now" });
  const button = await walkTo(app, (element) => element.textContent === "Back up now");
  expect(button).not.toBeNull();
  app.pad.press("confirm");
  await screen.findByText(/Backed up .*, checked\./);
  await walkTo(app, (element) => element.textContent === "Show in folder");
  expect(focused()?.textContent).toBe("Show in folder");
});
