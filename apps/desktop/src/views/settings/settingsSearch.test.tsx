/** #46: Settings' sections under group headings, and a search that finds a
 * section by its name, the preferences inside it and other words for them,
 * among only the sections the signed-in account sees. */
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { FixtureAccount } from "../../api/accountsFixtures";
import { fakeLayout, renderApp } from "../../test/harness";

vi.setConfig({ testTimeout: 20_000 });

let undoLayout = () => {};
beforeEach(() => {
  undoLayout = fakeLayout();
});
afterEach(() => {
  undoLayout();
  cleanup();
});

const field = () => screen.getByRole("searchbox", { name: "Search settings" });
const search = async (text: string) => {
  fireEvent.change(await screen.findByRole("searchbox", { name: "Search settings" }), {
    target: { value: text },
  });
};
const results = () => screen.getByRole("list", { name: "Settings search results" });
const headings = () =>
  Array.from(
    document.querySelectorAll('[data-cs="settings-nav"] [data-part="group"]'),
    (heading) => heading.textContent,
  );

it("groups the sections under headings, in the list's order", async () => {
  renderApp("/settings?tab=general");
  await screen.findByRole("tab", { name: "General", selected: true });
  expect(headings()).toEqual([
    "App",
    "Library",
    "Games & play",
    "Devices & streaming",
    "People",
    "System",
  ]);
  // The headings are no tabs of their own.
  const list = screen.getByRole("tablist", { name: "Settings sections" });
  expect(within(list).queryByRole("tab", { name: "Library" })).toBeNull();
});

it("finds saves in Devices and opens it", async () => {
  const app = renderApp("/settings?tab=general");
  await search("saves");
  // The section list steps aside for the results.
  expect(screen.queryByRole("tablist", { name: "Settings sections" })).toBeNull();
  fireEvent.click(within(results()).getByRole("button", { name: "Devices" }));
  await waitFor(() => expect(app.router.state.location.search).toEqual({ tab: "devices" }));
  expect(field()).toHaveProperty("value", "");
  await screen.findByRole("tab", { name: "Devices", selected: true });
});

it("tells app updates from game updates, and highlights the one chosen", async () => {
  const app = renderApp("/settings?tab=appearance");
  await search("updates");
  const list = within(results());
  expect(list.getByRole("button", { name: "Caskade updates in General" })).toBeTruthy();
  // Both live in General (#54).
  fireEvent.click(list.getByRole("button", { name: "Game updates in General" }));
  await waitFor(() => expect(app.router.state.location.search).toEqual({ tab: "general" }));
  await waitFor(() =>
    expect(document.querySelector("[data-found]")?.textContent).toMatch(/^Game updates/),
  );
});

it("opens a preference's sub-section", async () => {
  const app = renderApp("/settings?tab=general");
  await search("rom scanners");
  fireEvent.click(within(results()).getByRole("button", { name: "ROM Scanners in Emulation" }));
  await waitFor(() =>
    expect(app.router.state.location.search).toEqual({ tab: "emulators", section: "scanners" }),
  );
});

it("opens the first result with Enter; Launchers links back to Library Discovery", async () => {
  const app = renderApp("/settings?tab=general");
  await search("launchers");
  // Both places launchers are set up, the one named for them first.
  const sections = within(results())
    .getAllByRole("button")
    .map((button) => button.getAttribute("aria-label"))
    .slice(0, 2);
  expect(sections).toEqual(["Launchers", "Library Discovery"]);
  fireEvent.keyDown(field(), { key: "Enter" });
  await waitFor(() => expect(app.router.state.location.search).toEqual({ tab: "launchers" }));
  const link = await screen.findByRole("link", { name: "Library Discovery" });
  expect(link.getAttribute("href")).toMatch(/tab=library-discovery/);
});

it("clears with Escape and says when nothing matches", async () => {
  renderApp("/settings?tab=general");
  await search("zzzz");
  await screen.findByText("No settings match “zzzz”.");
  fireEvent.keyDown(field(), { key: "Escape" });
  await waitFor(() => expect(field()).toHaveProperty("value", ""));
  expect(screen.getByRole("tablist", { name: "Settings sections" })).toBeTruthy();
});

it("finds only what a standard account may open", async () => {
  const family: FixtureAccount[] = [
    { id: "ada", name: "Ada", role: "admin" },
    { id: "kid", name: "Kid", permissions: { settings: ["appearance"], install: false } },
  ];
  renderApp("/settings?tab=appearance", { accounts: family, activeAccount: "kid" });
  await search("backups");
  await screen.findByText("No settings match “backups”.");
  await search("launchers");
  await screen.findByText("No settings match “launchers”.");
  // Everyone's sections are still there.
  await search("saves");
  expect(within(results()).getByRole("button", { name: "Devices" })).toBeTruthy();
  expect(within(results()).queryByRole("button", { name: /Backup/ })).toBeNull();
});
