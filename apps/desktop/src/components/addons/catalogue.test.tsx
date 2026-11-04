import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { AddonsProvider } from "../../api/addons";
import { createAddonsFixture, fixtureCatalogueItems } from "../../api/addonsFixtures";
import { renderApp } from "../../test/harness";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

function items() {
  const list = fixtureCatalogueItems();
  // A plugin whose download fails its checksum, and one that won't start.
  const tampered = structuredClone(list[0]);
  const broken = structuredClone(list[0]);
  if (tampered && broken) {
    tampered.entry.id = "dev.example.tampered";
    tampered.entry.name = "Tampered";
    broken.entry.id = "dev.example.broken";
    broken.entry.name = "Broken";
    broken.compatibility.state = "updateAvailable";
    broken.compatibility.installedVersion = "0.1.0";
    list.push(tampered, broken);
  }
  return list;
}

function render(mode: "desktop" | "console" = "desktop") {
  const addons = createAddonsFixture(items());
  const app = renderApp("/settings?tab=plugins", {
    mode,
    wrap: (children) => <AddonsProvider api={addons}>{children}</AddonsProvider>,
  });
  return { app, addons };
}

const catalogue = () => screen.findByRole("region", { name: "Plugin and theme catalogue" });
const row = (name: string) =>
  within(screen.getByRole("list", { name: "Catalogue", hidden: true }))
    .getAllByRole("listitem", { hidden: true })
    .find((item) => item.textContent?.includes(name)) as HTMLElement;
const reviewDialog = () =>
  waitFor(() => {
    const review = document.querySelector<HTMLElement>('[data-cs="plugin-review"]');
    if (!review) throw new Error("no review yet");
    return review;
  });
const details = async (name: string) => {
  fireEvent.click(
    within(row(name)).getByRole("button", { name: `Details of ${name}`, hidden: true }),
  );
  return screen.findByRole("dialog", { name });
};

it("lists the catalogue with what each add-on means here, searchable and by kind", async () => {
  render();
  const section = await catalogue();
  await within(section).findByText("Playtime stats");
  expect(within(row("Playtime stats")).getByText("Available")).toBeTruthy();
  expect(within(row("HowLongToBeat")).getByText("Update to 0.2.0")).toBeTruthy();
  expect(within(row("From the future")).getByText("Not compatible")).toBeTruthy();
  expect(within(row("Paper")).getByText("Installed 1.0.0")).toBeTruthy();
  expect(within(section).getByText(/2 updates/)).toBeTruthy();

  fireEvent.click(within(section).getByRole("radio", { name: "Themes" }));
  expect(
    within(screen.getByRole("list", { name: "Catalogue" })).getAllByRole("listitem"),
  ).toHaveLength(1);
  fireEvent.click(within(section).getByRole("radio", { name: "All" }));
  fireEvent.change(within(section).getByRole("textbox", { name: "Search the catalogue" }), {
    target: { value: "longtobeat" },
  });
  expect(
    within(screen.getByRole("list", { name: "Catalogue" })).getAllByRole("listitem"),
  ).toHaveLength(1);
  fireEvent.change(within(section).getByRole("textbox", { name: "Search the catalogue" }), {
    target: { value: "nothing like this" },
  });
  expect(within(section).getByText("Nothing in the catalogue matches.")).toBeTruthy();
});

it("installs a plugin after the permission review, and shows details as text", async () => {
  const { addons } = render();
  await (await catalogue()).ownerDocument;
  await screen.findByText("Playtime stats");
  const install = vi.spyOn(addons, "install");
  const dialog = await details("Playtime stats");
  // Markup in a description stays text.
  expect(within(dialog).getByText(/<b>Markup<\/b> shows as text\./)).toBeTruthy();
  expect(dialog.querySelector("b")).toBeNull();
  // A screenshot that can't load leaves a placeholder.
  expect(within(dialog).getByRole("img", { name: "Playtime stats screenshot" })).toBeTruthy();
  fireEvent.click(within(dialog).getByRole("button", { name: "Install" }));
  const review = await reviewDialog();
  expect(install).toHaveBeenLastCalledWith("dev.caskade.playtime-stats", "desktop", false, false);
  fireEvent.click(within(review).getByRole("button", { name: "Install and allow", hidden: true }));
  await within(dialog).findByText("0.2.0 is installed.");
  expect(install).toHaveBeenLastCalledWith("dev.caskade.playtime-stats", "desktop", true, false);
  await waitFor(() =>
    expect(within(row("Playtime stats")).getByText("Installed 0.2.0")).toBeTruthy(),
  );
});

it("reports a refused download and a rollback, and retries only on request", async () => {
  const { addons } = render();
  await screen.findByText("Tampered");
  const install = vi.spyOn(addons, "install");
  let dialog = await details("Tampered");
  fireEvent.click(within(dialog).getByRole("button", { name: "Install" }));
  fireEvent.click(
    within(await reviewDialog()).getByRole("button", { name: "Install and allow", hidden: true }),
  );
  expect((await within(dialog).findByRole("alert")).textContent).toContain("checksum");
  fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));
  dialog = await details("Tampered");
  // Remembered: the button says Try again, which retries.
  expect(within(dialog).getByRole("alert").textContent).toContain("checksum");
  fireEvent.click(within(dialog).getByRole("button", { name: "Try again" }));
  await waitFor(() =>
    expect(install).toHaveBeenLastCalledWith("dev.example.tampered", "desktop", false, true),
  );
  // The retry asks for the review again; cancelling it leaves nothing installed.
  fireEvent.click(
    within(await reviewDialog()).getByRole("button", { name: "Cancel", hidden: true }),
  );
  await waitFor(() => expect(document.querySelector('[data-cs="plugin-review"]')).toBeNull());
  fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));

  dialog = await details("Broken");
  fireEvent.click(within(dialog).getByRole("button", { name: "Update to 0.2.0" }));
  expect((await within(dialog).findByRole("alert")).textContent).toContain(
    "previous version is back",
  );
});

it("installs, applies and removes a theme; skips an update; keeps plugin data on removal", async () => {
  const { addons, app } = render();
  await screen.findByText("Paper");
  let dialog = await details("Paper");
  const use = within(dialog).getByRole("button", { name: "Use this theme" });
  await waitFor(() => expect(use.hasAttribute("disabled")).toBe(false));
  fireEvent.click(use);
  await waitFor(async () =>
    expect((await app.settings.api.get("core.appearance")).value).toMatchObject({
      theme: "caskade.paper",
    }),
  );
  fireEvent.click(within(dialog).getByRole("button", { name: "Uninstall…" }));
  fireEvent.click(within(dialog).getByRole("button", { name: "Uninstall" }));
  await waitFor(() => expect(within(row("Paper")).getByText("Available")).toBeTruthy());
  fireEvent.click(within(dialog).getByRole("button", { name: "Close" }));

  const skip = vi.spyOn(addons, "skipUpdate");
  dialog = await details("HowLongToBeat");
  fireEvent.click(within(dialog).getByRole("button", { name: "Skip this version" }));
  await waitFor(() => expect(skip).toHaveBeenCalledWith("dev.caskade.howlongtobeat", "0.2.0"));
  const uninstall = vi.spyOn(addons, "uninstall");
  fireEvent.click(within(dialog).getByRole("button", { name: "Uninstall…" }));
  expect(
    within(dialog).getByRole("checkbox", { name: "Keep its settings and data for a reinstall" }),
  ).toBeTruthy();
  fireEvent.click(
    within(dialog).getAllByRole("button", { name: "Uninstall" }).at(-1) as HTMLElement,
  );
  await waitFor(() => expect(uninstall).toHaveBeenCalledWith("dev.caskade.howlongtobeat", true));
});

it("shows the cached catalogue offline and says installing needs a connection", async () => {
  const { addons } = render();
  addons.setOffline(true);
  const section = await catalogue();
  fireEvent.click(within(section).getByRole("button", { name: "Check for updates" }));
  await within(section).findByText(/Offline: showing the catalogue from/);
  const dialog = await details("Playtime stats");
  expect(within(dialog).getByText("Installing needs a connection.")).toBeTruthy();
});

it("browses and installs by pad alone in console mode", async () => {
  const { app } = render("console");
  await screen.findByText("From the future");
  const focus = (element: HTMLElement) => act(() => element.focus());
  focus(within(row("From the future")).getByRole("button", { hidden: true }));
  app.pad.press("confirm");
  let dialog = await screen.findByRole("dialog", { name: "From the future" });
  // Not compatible: the reason shows, and there is nothing to install.
  expect(within(dialog).getByText(/needs plugin API/)).toBeTruthy();
  expect(within(dialog).queryByRole("button", { name: "Install" })).toBeNull();
  focus(within(dialog).getByRole("button", { name: "Close" }));
  app.pad.press("confirm");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

  focus(within(row("Playtime stats")).getByRole("button", { hidden: true }));
  app.pad.press("confirm");
  dialog = await screen.findByRole("dialog", { name: "Playtime stats" });
  focus(within(dialog).getByRole("button", { name: "Install" }));
  app.pad.press("confirm");
  const review = await reviewDialog();
  focus(within(review).getByRole("button", { name: "Install and allow", hidden: true }));
  app.pad.press("confirm");
  await within(dialog).findByText("0.2.0 is installed.");
});
