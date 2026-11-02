/** #245: the catalogue offers what the user's other devices have and this
 * one lacks; nothing installs until the user installs it, through the same
 * details and plugin review as any catalogue add-on. */
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { AddonsProvider } from "../../api/addons";
import { createAddonsFixture } from "../../api/addonsFixtures";
import { renderApp } from "../../test/harness";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const desk = "a".repeat(64);
const laptop = "c".repeat(64);

function render() {
  const addons = createAddonsFixture();
  addons.setFromDevices([
    { deviceId: desk, deviceName: "Desk", addonId: "dev.caskade.playtime-stats", version: null },
    {
      deviceId: laptop,
      deviceName: "Laptop",
      addonId: "dev.caskade.playtime-stats",
      version: null,
    },
    // Installed here already (an update is out): nothing to offer.
    { deviceId: desk, deviceName: "Desk", addonId: "dev.caskade.howlongtobeat", version: "0.1.0" },
    // Not in the catalogue: said, never installable from here.
    { deviceId: desk, deviceName: "Desk", addonId: "dev.private.tools", version: null },
  ]);
  const install = vi.spyOn(addons, "install");
  renderApp("/settings?tab=plugins", {
    wrap: (children) => <AddonsProvider api={addons}>{children}</AddonsProvider>,
  });
  return { install };
}

it("offers the other devices' add-ons without installing anything", async () => {
  const { install } = render();
  const offers = await screen.findByRole("region", { name: "On your other devices" });
  const rows = within(offers).getAllByRole("listitem");
  expect(rows.map((row) => row.textContent)).toEqual([
    expect.stringContaining("Playtime stats"),
    expect.stringContaining("dev.private.tools"),
  ]);
  expect(rows[0]?.textContent).toContain("On Desk, Laptop");
  expect(rows[0]?.textContent).toContain("Available");
  expect(rows[1]?.textContent).toContain("isn't in the catalogue");
  expect(within(rows[1] as HTMLElement).queryByRole("button")).toBeNull();
  expect(install).not.toHaveBeenCalled();

  // Seeing one opens its details; installing is the user's next step.
  fireEvent.click(within(offers).getByRole("button", { name: "See Playtime stats" }));
  const details = await screen.findByRole("dialog", { name: "Playtime stats" });
  expect(within(details).getByRole("button", { name: /Install/ })).toBeTruthy();
  expect(install).not.toHaveBeenCalled();
});

it("nothing to offer, nothing shown", async () => {
  const addons = createAddonsFixture();
  renderApp("/settings?tab=plugins", {
    wrap: (children) => <AddonsProvider api={addons}>{children}</AddonsProvider>,
  });
  await screen.findByRole("region", { name: "Plugin and theme catalogue" });
  await waitFor(() =>
    expect(screen.queryByRole("region", { name: "On your other devices" })).toBeNull(),
  );
});
