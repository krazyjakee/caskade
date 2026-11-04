/** #43: a form with a Save button keeps its edits when another section is
 * chosen: Save, Discard or Stay, asked whichever way the section was chosen
 * (a click or LB/RB), and Back too. #58: so does switching accounts or modes,
 * and a close of the window that would quit. */
import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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

describe("switching accounts", () => {
  const switchAccount = async () => {
    fireEvent.click(await screen.findByRole("button", { name: /, account$/ }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Switch account" }));
  };
  const picker = () => screen.findByRole("heading", { name: "Who's playing?" });

  it("asks first; Stay keeps the edits and the account", async () => {
    const app = await editedProfile();
    await switchAccount();
    const dialog = await question();
    within(dialog).getByText(/Switching accounts without saving/);
    within(dialog).getByText("My profile");
    fireEvent.click(within(dialog).getByRole("button", { name: "Stay" }));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
    expect((await nameField()).value).toBe("Ace");
    expect((await app.accounts.list()).activeId).toBe("player");
  });

  it("Discard drops the edits and switches", async () => {
    const app = await editedProfile();
    await switchAccount();
    fireEvent.click(within(await question()).getByRole("button", { name: "Discard" }));
    await picker();
    expect((await app.accounts.list()).activeId).toBeNull();
    expect(await savedName(app)).toBe("Player");
  });

  it("Save saves the edits, then switches", async () => {
    const app = await editedProfile();
    await switchAccount();
    fireEvent.click(within(await question()).getByRole("button", { name: "Save" }));
    await picker();
    expect(await savedName(app)).toBe("Ace");
  });

  it("stays signed in when the edits can't be saved", async () => {
    const app = await editedProfile();
    fireEvent.change(await nameField(), { target: { value: "   " } });
    await switchAccount();
    const dialog = await question();
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await within(dialog).findByText(/Some changes couldn't be saved/);
    expect((await app.accounts.list()).activeId).toBe("player");
  });

  it("doesn't ask without edits", async () => {
    renderApp("/settings?tab=profile");
    await nameField();
    await switchAccount();
    await picker();
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });
});

describe("closing the window", () => {
  /** My profile with an edit, and the window listening for closes. */
  async function editedAndListening(options: Parameters<typeof renderApp>[1] = {}) {
    const app = await editedProfile(options);
    await waitFor(() => expect(app.window.closeListeners.size).toBe(1));
    return app;
  }
  /** The close was let through. */
  const closed = (app: ReturnType<typeof renderApp>) =>
    waitFor(() => expect(app.window.calls).toContain("close"));
  /** The adapter was told the close was heard: the hold, then that word. */
  const heard = (app: ReturnType<typeof renderApp>) =>
    waitFor(() => expect(app.window.holds).toEqual([true, true]));
  /** Long enough for the webview to have answered, if it was going to. */
  const settled = () => new Promise((done) => setTimeout(done, 250));
  const playing = (app: ReturnType<typeof renderApp>) =>
    act(async () => {
      await app.library.launchGame("entry-1");
    });

  it("doesn't ask or listen without edits", async () => {
    const app = renderApp("/settings?tab=profile");
    await nameField();
    expect(app.window.closeListeners.size).toBe(0);
    expect(app.window.holds).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await closed(app);
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("releases closes once the edits are gone, so nothing holds one", async () => {
    const app = await editedAndListening();
    expect(app.window.holds).toEqual([true]);
    fireEvent.change(await nameField(), { target: { value: "Player" } });
    await waitFor(() => expect(app.window.closeListeners.size).toBe(0));
    await waitFor(() => expect(app.window.holds).toEqual([true, false]));
  });

  it("asks at the close button; Stay keeps the window and the edits", async () => {
    const app = await editedAndListening();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    const dialog = await question();
    within(dialog).getByText(/Closing Caskade without saving/);
    within(dialog).getByText("My profile");

    await heard(app);
    fireEvent.click(within(dialog).getByRole("button", { name: "Stay" }));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
    expect(app.window.calls).toEqual(["close refused"]);
    expect((await nameField()).value).toBe("Ace");
    expect(app.window.closeListeners.size).toBe(1);
    // A fresh hold, which is how the adapter forgets the quit.
    await waitFor(() => expect(app.window.holds).toEqual([true, true, false, true]));
  });

  it("asks for a close from outside the page: Alt+F4, or the tray's Quit", async () => {
    const app = await editedAndListening();
    // The tray's menu has focus, not the window.
    app.window.place({ focused: false });
    await app.window.close();
    await question();
    await heard(app);
    expect(app.window.calls).toEqual(["close refused"]);
  });

  it("Discard drops the edits, releases the hold and lets the close through", async () => {
    const app = await editedAndListening();
    await app.window.close();
    fireEvent.click(within(await question()).getByRole("button", { name: "Discard" }));
    await closed(app);
    expect(app.window.closeListeners.size).toBe(0);
    expect(app.window.holds.at(-1)).toBe(false);
    expect(await savedName(app)).toBe("Player");
  });

  it("Save saves the edits, then lets the close through", async () => {
    const app = await editedAndListening();
    await app.window.close();
    fireEvent.click(within(await question()).getByRole("button", { name: "Save" }));
    await closed(app);
    expect(await savedName(app)).toBe("Ace");
  });

  it("stays open when the edits can't be saved, and says so", async () => {
    const app = await editedAndListening();
    fireEvent.change(await nameField(), { target: { value: "   " } });
    await app.window.close();
    const dialog = await question();
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await within(dialog).findByText(/Some changes couldn't be saved/);
    expect(app.window.calls).toEqual(["close refused"]);
  });

  it("a pad answers it in console mode", async () => {
    const app = await editedAndListening({ mode: "console" });
    await app.window.close();
    await question();
    await until(() => focused()?.textContent === "Stay", "Stay");
    app.pad.press("confirm");
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
    expect(app.window.calls).toEqual(["close refused"]);
  });

  it("asks nothing when the close hid the window to the tray, and the hold stays", async () => {
    const app = await editedAndListening();
    app.window.place({ visible: false });
    await app.window.close();
    await settled();
    expect(screen.queryByRole("alertdialog")).toBeNull();
    // The hold stays, so the tray's Quit brings the window back to ask.
    expect(app.window.holds).toEqual([true]);
    expect((await nameField()).value).toBe("Ace");

    app.window.place({ visible: true });
    await app.window.close();
    await question();
    await heard(app);
  });

  it("says a question was heard only once it can be seen", async () => {
    const app = await editedAndListening();
    // Raised, but not there yet: the adapter keeps counting.
    app.window.place({ minimized: true, focused: false });
    await app.window.close();
    await question();
    await settled();
    expect(app.window.holds).toEqual([true]);

    app.window.place({ minimized: false, focused: true });
    act(() => app.window.focus(true));
    await heard(app);
  });

  it("leaves a quit under a running game to the adapter's limit", async () => {
    const app = await editedAndListening();
    await playing(app);
    app.window.place({ focused: false });
    await app.window.close();
    await question();
    await settled();
    expect(app.window.holds).toEqual([true]);
  });

  it("holds a quit with a game running when Caskade is in front", async () => {
    const app = await editedAndListening();
    await playing(app);
    await app.window.close();
    await question();
    await heard(app);
  });
});

describe("switching modes", () => {
  const consoleMode = () => screen.findByRole("button", { name: "Console mode" });

  it("asks first; Stay keeps the mode and the edits", async () => {
    const app = await editedProfile();
    fireEvent.click(await consoleMode());
    const dialog = await question();
    within(dialog).getByText(/Switching modes without saving/);
    fireEvent.click(within(dialog).getByRole("button", { name: "Stay" }));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
    expect(app.console.calls).toEqual([]);
    expect((await nameField()).value).toBe("Ace");
  });

  it("Discard drops the edits and switches", async () => {
    const app = await editedProfile();
    fireEvent.click(await consoleMode());
    fireEvent.click(within(await question()).getByRole("button", { name: "Discard" }));
    await waitFor(() => expect(app.console.calls).toEqual(["console"]));
    expect(await savedName(app)).toBe("Player");
  });

  it("Save saves the edits, then switches", async () => {
    const app = await editedProfile();
    fireEvent.click(await consoleMode());
    fireEvent.click(within(await question()).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(app.console.calls).toEqual(["console"]));
    expect(await savedName(app)).toBe("Ace");
  });

  it("asks for a pad's View too, on the way out of console mode", async () => {
    const app = await editedProfile({ mode: "console" });
    app.pad.press("view");
    await question();
    expect(app.console.calls).toEqual([]);
  });

  it("doesn't ask without edits", async () => {
    const app = renderApp("/settings?tab=profile");
    await nameField();
    fireEvent.click(await consoleMode());
    await waitFor(() => expect(app.console.calls).toEqual(["console"]));
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });
});
