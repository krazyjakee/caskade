/** #155: Settings' sections are a vertical tab list beside the open one.
 * #197: the emulation section is labelled Emulation; its URL stays
 * `tab=emulators`. */
import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { fakeLayout, fakeViewport, focused, renderApp } from "../../test/harness";
import { settingsTabs } from "../SettingsView";

let undoLayout = () => {};
afterEach(() => {
  undoLayout();
  undoLayout = () => {};
  cleanup();
});

const sectionList = () => screen.getByRole("tablist", { name: "Settings sections" });
const tab = (name: string) => within(sectionList()).getByRole("tab", { name });

it("lists every section vertically, in order, each tab tied to its panel", async () => {
  const app = renderApp("/settings?tab=emulators&section=platforms");
  const open = await screen.findByRole("tab", { name: "Emulation", selected: true });
  expect(sectionList().getAttribute("aria-orientation")).toBe("vertical");
  expect(
    within(sectionList())
      .getAllByRole("tab")
      .map((element) => element.textContent),
  ).toEqual(settingsTabs.map((t) => t.label.message));
  expect(screen.queryByRole("tab", { name: "Emulators" })).toBeNull();
  // The open tab controls the panel, and the panel is labelled by it.
  const panel = screen.getByRole("tabpanel");
  expect(open.getAttribute("aria-controls")).toBe(panel.id);
  expect(panel.getAttribute("aria-labelledby")).toBe(open.id);
  expect(screen.getByRole("tabpanel", { name: "Emulation" })).toBe(panel);
  // The URL keeps its values, the subsection included.
  expect(app.router.state.location.search).toMatchObject({
    tab: "emulators",
    section: "platforms",
  });
  await waitFor(() =>
    expect(screen.getByRole("radio", { name: "Platforms" }).getAttribute("aria-checked")).toBe(
      "true",
    ),
  );
});

it("opens a section in place on a click, so Back leaves Settings", async () => {
  const app = renderApp("/");
  await app.router.navigate({ to: "/settings", search: { tab: "emulators", section: "scanners" } });
  await screen.findByRole("tab", { name: "Emulation", selected: true });
  fireEvent.click(tab("Storage"));
  await waitFor(() => expect(app.router.state.location.search).toEqual({ tab: "storage" }));
  expect(tab("Storage").getAttribute("aria-selected")).toBe("true");
  app.pad.press("back");
  await waitFor(() => expect(app.router.state.location.pathname).toBe("/"));
});

it("moves through the list with the up and down arrow keys", async () => {
  const app = renderApp("/settings?tab=general");
  const general = await screen.findByRole("tab", { name: "General", selected: true });
  act(() => general.focus());
  fireEvent.keyDown(general, { key: "ArrowDown" });
  await waitFor(() =>
    expect(app.router.state.location.search).toMatchObject({ tab: "library-discovery" }),
  );
  await waitFor(() => expect(focused()).toBe(tab("Library Discovery")));
  fireEvent.keyDown(focused() as HTMLElement, { key: "ArrowUp" });
  await waitFor(() => expect(app.router.state.location.search).toMatchObject({ tab: "general" }));
  await waitFor(() => expect(focused()).toBe(tab("General")));
});

/** jsdom has no layout: the section list as a column at the left, and
 * everything else in the view as a column beside it. */
function sideBySide() {
  const main = document.querySelector('[data-focus-section="main"]') as HTMLElement;
  for (const [i, element] of within(sectionList()).getAllByRole("tab").entries())
    element.setAttribute("data-rect", `40 ${100 + i * 40} 160 36`);
  const rest = Array.from(
    main.querySelectorAll(
      'a[href], button, input, select, textarea, [tabindex], [role], [data-cs="setting-row"]',
    ),
  ).filter((element) => !sectionList().contains(element));
  for (const [i, element] of rest.entries())
    element.setAttribute("data-rect", `300 ${100 + i * 40} 600 36`);
}

it("walks the list with a pad, opens a section, and goes into the panel and back", async () => {
  undoLayout = fakeLayout();
  fakeViewport();
  const app = renderApp("/settings?tab=general", { mode: "console" });
  const general = await screen.findByRole("tab", { name: "General", selected: true });
  // A pad starts on the open section.
  await waitFor(() => expect(focused()).toBe(general));

  // Down moves focus along the list without opening anything; A opens.
  app.pad.press("nav.down");
  expect(focused()).toBe(tab("Library Discovery"));
  expect(app.router.state.location.search).toMatchObject({ tab: "general" });
  app.pad.press("confirm");
  await waitFor(() =>
    expect(app.router.state.location.search).toMatchObject({ tab: "library-discovery" }),
  );

  // RB opens the next section, and focus follows it along the list.
  app.pad.press("tab.next");
  await waitFor(() => expect(app.router.state.location.search).toMatchObject({ tab: "launchers" }));
  await waitFor(() => expect(focused()).toBe(tab("Launchers")));

  // Right goes into the panel; left comes back to the section it left.
  await waitFor(() =>
    expect(screen.getByRole("tabpanel").querySelector("input, button")).not.toBeNull(),
  );
  await new Promise((resolve) => setTimeout(resolve, 50));
  sideBySide();
  app.pad.press("nav.right");
  expect(screen.getByRole("tabpanel").contains(focused())).toBe(true);
  app.pad.press("nav.left");
  expect(focused()).toBe(tab("Launchers"));
  expect(app.router.state.location.search).toMatchObject({ tab: "launchers" });
});
