/** #208: Settings → Accounts for admins and My profile for everyone. Hidden
 * tabs follow the signed-in account's permissions as they change, the
 * editor shows the core's reason for a change that breaks a rule, deleting
 * asks first, and changing your own PIN asks for the current one. */
import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { FixtureAccount } from "../../../api/accountsFixtures";
import { fakeLayout, focused, renderApp, until } from "../../../test/harness";

// The pad walks take a few seconds, more beside the other suites.
vi.setConfig({ testTimeout: 20_000 });

let undoLayout = () => {};
beforeEach(() => {
  undoLayout = fakeLayout();
  localStorage.clear();
});
afterEach(() => {
  undoLayout();
  cleanup();
  localStorage.clear();
});

const family: FixtureAccount[] = [
  { id: "ada", name: "Ada", role: "admin", avatar: "avatar:rocket", pin: "2468" },
  {
    id: "kid",
    name: "Kid",
    avatar: "avatar:moon",
    pin: "1357",
    permissions: { settings: ["appearance"], install: false },
    maxAge: 12,
  },
];

const sections = () =>
  within(screen.getByRole("tablist", { name: "Settings sections" }))
    .getAllByRole("tab")
    .map((tab) => tab.textContent);
const dialog = (name: string) => screen.findByRole("dialog", { name });
/** The pad's field; its dialog carries the same title as its name. */
const pinField = (title: string) =>
  screen.getByLabelText(title, { selector: "input" }) as HTMLInputElement;
const enterPin = async (title: string, pin: string) => {
  await waitFor(() => expect(pinField(title)).toBeTruthy());
  fireEvent.change(pinField(title), { target: { value: pin } });
  fireEvent.submit(pinField(title).closest("form") as HTMLFormElement);
};

it("shows an admin Accounts and My profile, and a standard account only what it may change", async () => {
  renderApp("/settings?tab=accounts", { accounts: family, activeAccount: "ada" });
  await screen.findByRole("button", { name: "Edit Kid" });
  expect(sections()).toContain("Accounts");
  expect(sections()).toContain("My profile");
  const list = screen.getByRole("list", { name: "Accounts" });
  expect(within(list).getByText(/Standard · PIN · PEGI 12/)).toBeTruthy();
  cleanup();

  renderApp("/settings?tab=appearance", { accounts: family, activeAccount: "kid" });
  await screen.findByRole("tab", { name: "Appearance", selected: true });
  expect(sections()).toEqual(["Appearance", "My profile", "Statistics", "About"]);
});

it("asks an admin for a hidden tab's link", async () => {
  const app = renderApp("/settings?tab=accounts", { accounts: family, activeAccount: "kid" });
  await screen.findByText("Your account can't change these settings. Ask an admin.");
  expect(screen.queryByRole("list", { name: "Accounts" })).toBeNull();
  await act(() => app.router.navigate({ to: "/settings", search: { tab: "storage" } }));
  await screen.findByText("Your account can't change these settings. Ask an admin.");
});

it("shows and hides tabs as soon as an admin changes the account's permissions", async () => {
  const app = renderApp("/settings?tab=appearance", { accounts: family, activeAccount: "kid" });
  await screen.findByRole("tab", { name: "Appearance", selected: true });
  expect(sections()).not.toContain("Storage");
  await act(() =>
    app.accounts.update("kid", {
      name: "Kid",
      avatar: "avatar:moon",
      access: {
        role: "standard",
        permissions: { settings: ["appearance", "storage"], install: false },
        maxAge: 12,
        unratedNeedsPin: false,
      },
    }),
  );
  await waitFor(() => expect(sections()).toContain("Storage"));
  await act(() =>
    app.accounts.update("kid", {
      name: "Kid",
      avatar: "avatar:moon",
      access: {
        role: "standard",
        permissions: { settings: [], install: false },
        maxAge: 12,
        unratedNeedsPin: false,
      },
    }),
  );
  await waitFor(() => expect(sections()).not.toContain("Appearance"));
  await screen.findByText("Your account can't change these settings. Ask an admin.");
});

it("shows the core's reason when a standard account needs the admin's PIN first", async () => {
  const app = renderApp("/settings?tab=accounts");
  fireEvent.click(await screen.findByRole("button", { name: "Add account" }));
  const editor = await dialog("Add account");
  fireEvent.change(within(editor).getByRole("textbox", { name: "Name" }), {
    target: { value: "Kid" },
  });
  fireEvent.click(within(editor).getByRole("button", { name: "Add" }));
  await within(editor).findByText("Every admin needs a PIN while there are standard accounts.");
  expect((await app.accounts.list()).accounts).toHaveLength(1);
});

it("keeps the last admin an admin", async () => {
  const app = renderApp("/settings?tab=accounts");
  fireEvent.click(await screen.findByRole("button", { name: "Edit Player" }));
  const editor = await dialog("Edit Player");
  fireEvent.click(within(editor).getByRole("radio", { name: "Standard" }));
  fireEvent.click(within(editor).getByRole("button", { name: "Save" }));
  await within(editor).findByText("The last admin can't be removed or made standard.");
  expect((await app.accounts.list()).accounts[0]?.role).toBe("admin");
});

it("adds a standard account with a PIN, its permissions and an age limit", async () => {
  const app = renderApp("/settings?tab=accounts", {
    accounts: [family[0] as FixtureAccount],
    activeAccount: "ada",
  });
  fireEvent.click(await screen.findByRole("button", { name: "Add account" }));
  const editor = await dialog("Add account");
  fireEvent.change(within(editor).getByRole("textbox", { name: "Name" }), {
    target: { value: "Sam" },
  });
  fireEvent.click(within(editor).getByRole("switch", { name: "Appearance settings" }));
  fireEvent.click(within(editor).getByRole("switch", { name: "Install games" }));
  fireEvent.click(within(editor).getByRole("button", { name: /Maximum age rating/ }));
  fireEvent.click(await screen.findByRole("option", { name: "PEGI 16" }));
  fireEvent.click(within(editor).getByRole("switch", { name: "Unrated games need a PIN" }));

  // The PIN, twice, on the pad.
  fireEvent.click(within(editor).getByRole("button", { name: "Set PIN" }));
  await enterPin("Choose a new PIN for Sam", "9753");
  await enterPin("Enter the new PIN again", "9753");
  await within(editor).findByRole("button", { name: "Change PIN" });

  fireEvent.click(within(editor).getByRole("button", { name: "Add" }));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Add account" })).toBeNull());
  const sam = (await app.accounts.list()).accounts.find((account) => account.name === "Sam");
  expect(sam).toMatchObject({
    role: "standard",
    hasPin: true,
    permissions: { settings: ["appearance"], install: true },
    maxAge: 16,
    unratedNeedsPin: true,
  });
  expect(app.accounts.pins.get(sam?.id ?? "")).toBe("9753");
});

it("asks before deleting an account, and says its collections go with it", async () => {
  const app = renderApp("/settings?tab=accounts", { accounts: family, activeAccount: "ada" });
  fireEvent.click(await screen.findByRole("button", { name: "Delete Kid" }));
  const confirm = await screen.findByRole("alertdialog", { name: "Delete Kid?" });
  expect(confirm.textContent).toContain("Kid's collections go with it.");
  fireEvent.click(within(confirm).getByRole("button", { name: "Delete" }));
  await waitFor(() => expect(screen.queryByRole("button", { name: "Edit Kid" })).toBeNull());
  expect((await app.accounts.list()).accounts.map((account) => account.id)).toEqual(["ada"]);
});

it("refuses to remove the admin's PIN while a standard account exists", async () => {
  renderApp("/settings?tab=accounts", { accounts: family, activeAccount: "ada" });
  fireEvent.click(await screen.findByRole("button", { name: "Edit Ada" }));
  const editor = await dialog("Edit Ada");
  fireEvent.click(within(editor).getByRole("button", { name: "Remove PIN" }));
  // Ada is the signed-in account: her current PIN first.
  await enterPin("Enter Ada's current PIN", "2468");
  await screen.findByText("Every admin needs a PIN while there are standard accounts.");
});

it("changes your own name and PIN in My profile, after the current PIN", async () => {
  const app = renderApp("/settings?tab=profile", { accounts: family, activeAccount: "kid" });
  const name = await screen.findByRole("textbox", { name: "Name" });
  fireEvent.change(name, { target: { value: "Ace" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(async () =>
    expect((await app.accounts.list()).accounts.find((a) => a.id === "kid")?.name).toBe("Ace"),
  );
  await screen.findByText(/Signed in as Ace/);

  fireEvent.click(screen.getByRole("button", { name: "Change PIN" }));
  await enterPin("Enter Ace's current PIN", "0000");
  await screen.findByText("That PIN isn't right.");
  await enterPin("Enter Ace's current PIN", "1357");
  await enterPin("Choose a new PIN for Ace", "8642");
  await enterPin("Enter the new PIN again", "8640");
  await screen.findByText("The PINs don't match. Try again.");
  await enterPin("Enter the new PIN again", "8642");
  await waitFor(() => expect(app.accounts.pins.get("kid")).toBe("8642"));
  // Still Kid's own session; nothing else changed.
  expect((await app.accounts.list()).activeId).toBe("kid");
});

it("sets another account's PIN with a pad only, on the PIN pad", async () => {
  const app = renderApp("/settings?tab=accounts", {
    mode: "console",
    accounts: family,
    activeAccount: "ada",
  });
  const walkTo = async (found: () => boolean, what: string) => {
    for (let i = 0; i < 80 && !found(); i++) app.pad.press("nav.down");
    await until(found, what);
  };
  (await screen.findByRole("button", { name: "Edit Kid" })).focus();
  app.pad.press("confirm");
  const editor = await dialog("Edit Kid");
  await until(() => editor.contains(focused()), "the editor");
  await walkTo(() => focused()?.textContent === "Change PIN", "Change PIN");
  app.pad.press("confirm");

  const type = async (pin: string) => {
    await until(() => focused()?.textContent === "1", "the number pad");
    for (const digit of pin) {
      for (let i = 0; i < 12 && focused()?.textContent !== digit; i++) app.pad.press("nav.down");
      expect(focused()?.textContent).toBe(digit);
      app.pad.press("confirm");
    }
    app.pad.press("menu");
  };
  // An admin changing another account's PIN isn't asked for the old one.
  await screen.findByLabelText("Choose a new PIN for Kid", { selector: "input" });
  await type("1234");
  await screen.findByLabelText("Enter the new PIN again", { selector: "input" });
  await type("1234");
  await waitFor(() => expect(app.accounts.pins.get("kid")).toBe("1234"));
  expect(screen.queryByLabelText("Enter the new PIN again")).toBeNull();
});
