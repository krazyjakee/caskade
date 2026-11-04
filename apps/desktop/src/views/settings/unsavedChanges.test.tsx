/** #43: a form with a Save button keeps its edits when another section is
 * chosen: Save, Discard or Stay, asked whichever way the section was chosen
 * (a click or LB/RB), and Back too. */
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { fakeLayout, focused, renderApp, until } from "../../test/harness";

vi.setConfig({ testTimeout: 20_000 });

let undoLayout = () => {};
beforeEach(() => {
  undoLayout = fakeLayout();
});
afterEach(() => {
  undoLayout();
  cleanup();
});

const tab = (name: string) =>
  within(screen.getByRole("tablist", { name: "Settings sections" })).getByRole("tab", { name });
const question = () => screen.findByRole("alertdialog", { name: "Save your changes?" });
const nameField = () => screen.findByRole("textbox", { name: "Name" }) as Promise<HTMLInputElement>;
const savedName = async (app: ReturnType<typeof renderApp>) =>
  (await app.accounts.list()).accounts.find((account) => account.id === "player")?.name;

/** My profile with the name changed and not saved. */
async function editedProfile(options: Parameters<typeof renderApp>[1] = {}) {
  const app = renderApp("/settings?tab=profile", options);
  fireEvent.change(await nameField(), { target: { value: "Ace" } });
  await screen.findByText("Unsaved changes");
  return app;
}

it("says a form has unsaved changes only while it has", async () => {
  renderApp("/settings?tab=profile");
  const field = await nameField();
  expect(screen.queryByText("Unsaved changes")).toBeNull();
  fireEvent.change(field, { target: { value: "Ace" } });
  await screen.findByText("Unsaved changes");
  fireEvent.change(field, { target: { value: "Player" } });
  await waitFor(() => expect(screen.queryByText("Unsaved changes")).toBeNull());
});

it("asks before another section replaces a form with edits; Stay keeps them", async () => {
  const app = await editedProfile();
  fireEvent.click(tab("General"));
  const dialog = await question();
  expect(within(dialog).getByText("My profile")).toBeTruthy();
  expect(app.router.state.location.search).toMatchObject({ tab: "profile" });

  fireEvent.click(within(dialog).getByRole("button", { name: "Stay" }));
  await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
  expect(app.router.state.location.search).toMatchObject({ tab: "profile" });
  expect((await nameField()).value).toBe("Ace");
  expect(await savedName(app)).toBe("Player");
});

it("Discard drops the edits and opens the section", async () => {
  const app = await editedProfile();
  fireEvent.click(tab("General"));
  fireEvent.click(within(await question()).getByRole("button", { name: "Discard" }));
  await waitFor(() => expect(app.router.state.location.search).toMatchObject({ tab: "general" }));
  expect(await savedName(app)).toBe("Player");

  // Nothing left to ask about: back to My profile and away again.
  fireEvent.click(tab("My profile"));
  expect((await nameField()).value).toBe("Player");
  fireEvent.click(tab("General"));
  await waitFor(() => expect(app.router.state.location.search).toMatchObject({ tab: "general" }));
  expect(screen.queryByRole("alertdialog")).toBeNull();
});

it("Save saves the edits, then opens the section", async () => {
  const app = await editedProfile();
  fireEvent.click(tab("General"));
  fireEvent.click(within(await question()).getByRole("button", { name: "Save" }));
  await waitFor(() => expect(app.router.state.location.search).toMatchObject({ tab: "general" }));
  expect(await savedName(app)).toBe("Ace");
});

it("stays when the edits can't be saved, and says so", async () => {
  const app = await editedProfile();
  fireEvent.change(await nameField(), { target: { value: "   " } });
  fireEvent.click(tab("General"));
  const dialog = await question();
  fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
  await within(dialog).findByText(/Some changes couldn't be saved/);
  expect(app.router.state.location.search).toMatchObject({ tab: "profile" });
});

it("asks for LB/RB too, and a pad answers it", async () => {
  const app = await editedProfile({ mode: "console" });
  app.pad.press("tab.next");
  await question();
  expect(app.router.state.location.search).toMatchObject({ tab: "profile" });
  // A pad starts on Stay.
  await until(() => focused()?.textContent === "Stay", "Stay");
  app.pad.press("confirm");
  await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
  expect(app.router.state.location.search).toMatchObject({ tab: "profile" });
  expect((await nameField()).value).toBe("Ace");

  app.pad.press("tab.prev");
  fireEvent.click(within(await question()).getByRole("button", { name: "Discard" }));
  await waitFor(() =>
    expect(app.router.state.location.search).not.toMatchObject({ tab: "profile" }),
  );
});

it("asks before leaving Settings too", async () => {
  const app = await editedProfile();
  app.router.history.push("/library");
  fireEvent.click(within(await question()).getByRole("button", { name: "Stay" }));
  await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
  expect(app.router.state.location.pathname).toBe("/settings");
});

it("asks for the custom CSS's draft", async () => {
  const app = renderApp("/settings?tab=appearance");
  const css = await screen.findByRole("textbox", { name: "Custom CSS" });
  fireEvent.change(css, { target: { value: '[data-cs="hero"] { color: hotpink; }' } });
  await screen.findByText("Unsaved changes");
  fireEvent.click(tab("General"));
  const dialog = await question();
  expect(within(dialog).getByText("Custom CSS")).toBeTruthy();
  fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
  await waitFor(() => expect(app.router.state.location.search).toMatchObject({ tab: "general" }));
  await waitFor(() => expect(app.settings.writes).toContain("core.appearance"));
});
