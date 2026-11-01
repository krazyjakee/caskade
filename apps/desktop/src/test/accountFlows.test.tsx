/** #207: who is playing. The picker at start, PIN entry with a pad and with
 * keys, the wrong-PIN wait, and the top bar's My profile and switching. */
import { act, cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, onTestFinished, vi } from "vitest";
import type { FixtureAccount } from "../api/accountsFixtures";
import { fakeLayout, focused, renderApp, until } from "./harness";

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
  { id: "kid", name: "Kid", avatar: "avatar:moon" },
];

const picker = () => document.querySelector<HTMLElement>('[data-cs="account-picker"]');
const pinPad = () => document.querySelector<HTMLElement>('[data-cs="pin-pad"]');
/** Everything kept in browser storage. */
const stored = () =>
  Array.from({ length: localStorage.length }, (_, i) => {
    const key = localStorage.key(i) ?? "";
    return `${key}=${localStorage.getItem(key)}`;
  }).join("\n");

it("starts straight in with one account and no PIN", async () => {
  renderApp("/");
  await screen.findByRole("button", { name: "Player, account" });
  expect(picker()).toBeNull();
});

it("asks who is playing when the only account has a PIN", async () => {
  renderApp("/", { accounts: [{ id: "ada", name: "Ada", role: "admin", pin: "2468" }] });
  await screen.findByRole("heading", { name: "Who's playing?" });
  expect(screen.getByRole("button", { name: "Ada, PIN locked" })).toBeTruthy();
});

it("signs in with a pad only: the last account, the number pad, Start", async () => {
  localStorage.setItem("caskade.accounts.last", "ada");
  const app = renderApp("/", { mode: "console", accounts: family });
  await screen.findByRole("heading", { name: "Who's playing?" });
  // The console picker, with its hints; focus starts on the last account.
  expect(picker()?.querySelector('[data-cs="hint-bar"]')).not.toBeNull();
  await until(() => focused()?.getAttribute("aria-label") === "Ada, PIN locked", "Ada");
  app.pad.press("confirm");
  await until(() => focused()?.textContent === "1", "the number pad");
  const hints = () => picker()?.querySelector('[data-cs="hint-bar"]')?.textContent ?? "";
  await waitFor(() => expect(hints()).toContain("Confirm"));

  for (const digit of ["2", "4", "6", "9"]) {
    for (let i = 0; i < 12 && focused()?.textContent !== digit; i++) app.pad.press("nav.down");
    expect(focused()?.textContent).toBe(digit);
    app.pad.press("confirm");
  }
  // B deletes the 9; then 8.
  await waitFor(() => expect(hints()).toContain("Delete"));
  app.pad.press("back");
  for (let i = 0; i < 12 && focused()?.textContent !== "8"; i++) app.pad.press("nav.up");
  app.pad.press("confirm");
  await waitFor(() =>
    expect((screen.getByLabelText("Enter Ada's PIN") as HTMLInputElement).value).toBe("2468"),
  );
  app.pad.press("menu");

  await screen.findByRole("button", { name: "Ada, account" });
  expect(picker()).toBeNull();
  expect((await app.accounts.list()).activeId).toBe("ada");
  expect(localStorage.getItem("caskade.accounts.last")).toBe("ada");
  expect(stored()).not.toContain("2468");
});

it("shakes and clears a wrong PIN, then counts down the wait", async () => {
  // The clock alone is the test's: timers stay real, so the wait reads the
  // same however slowly the test runs.
  vi.useFakeTimers({ toFake: ["Date"], now: Date.UTC(2026, 9, 30, 12) });
  onTestFinished(() => {
    vi.useRealTimers();
  });
  const app = renderApp("/", { accounts: family });
  fireEvent.click(await screen.findByRole("button", { name: "Ada, PIN locked" }));
  const field = () => screen.getByLabelText("Enter Ada's PIN") as HTMLInputElement;
  const enter = (pin: string) => {
    fireEvent.change(field(), { target: { value: pin } });
    fireEvent.submit(field().closest("form") as HTMLFormElement);
  };
  // Keys type in the field.
  await waitFor(() => expect(focused()).toBe(field()));

  enter("1357");
  await waitFor(() => expect(pinPad()?.dataset.status).toBe("wrong"));
  expect(field().value).toBe("");
  expect(field().className).toContain("animate-shake");
  expect(screen.getByRole("status").textContent).toBe("That PIN isn't right.");

  // Four more: the fifth starts a wait.
  for (let attempt = 2; attempt <= 5; attempt++) {
    enter("1357");
    await waitFor(() => expect(app.accounts.failures.get("ada")).toBe(attempt));
  }
  await waitFor(() => expect(pinPad()?.dataset.status).toBe("locked"));
  expect(screen.getByRole("status").textContent).toBe("Too many wrong PINs. Try again in 0:30.");
  expect(field().readOnly).toBe(true);
  expect((await app.accounts.list()).activeId).toBeNull();
  // It counts down with the clock.
  vi.setSystemTime(Date.now() + 1500);
  // The next tick (real time, once a second) shows it.
  await waitFor(
    () =>
      expect(screen.getByRole("status").textContent).toBe(
        "Too many wrong PINs. Try again in 0:29.",
      ),
    { timeout: 3000 },
  );

  // Escape goes back to the accounts.
  fireEvent.keyDown(field(), { key: "Escape" });
  await screen.findByRole("heading", { name: "Who's playing?" });
});

it("shows a wait that is already running when the PIN pad opens", async () => {
  vi.useFakeTimers({ toFake: ["Date"], now: Date.UTC(2026, 9, 30, 12) });
  onTestFinished(() => {
    vi.useRealTimers();
  });
  renderApp("/", {
    accounts: [
      { ...(family[0] as FixtureAccount), lockedUntilMs: Date.now() + 90_000 },
      family[1] as FixtureAccount,
    ],
  });
  fireEvent.click(await screen.findByRole("button", { name: "Ada, PIN locked" }));
  await waitFor(() => expect(pinPad()?.dataset.status).toBe("locked"));
  expect(screen.getByRole("status").textContent).toBe("Too many wrong PINs. Try again in 1:30.");
});

it("switches account from the top bar, but not while a game runs", async () => {
  const app = renderApp("/library", { accounts: family, activeAccount: "ada" });
  const avatar = await screen.findByRole("button", { name: "Ada, account" });

  let session = "";
  await act(async () => {
    const launched = await app.library.launchGame("entry-1");
    if (launched.type === "started") session = launched.ticket.sessionId;
  });
  expect(session).not.toBe("");
  fireEvent.click(avatar);
  const busy = await screen.findByRole("menuitem", { name: /Switch account/ });
  await waitFor(() => expect(busy.getAttribute("aria-disabled")).toBe("true"));
  expect(busy.textContent).toContain("Not while a game is running.");
  fireEvent.keyDown(busy, { key: "Escape" });
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());

  act(() => app.library.finishSession(session, 60));
  fireEvent.click(screen.getByRole("button", { name: "Ada, account" }));
  const item = await screen.findByRole("menuitem", { name: "Switch account" });
  await waitFor(() => expect(item.getAttribute("aria-disabled")).toBeNull());
  fireEvent.click(item);

  // Signed out: the picker; Kid has no PIN.
  fireEvent.click(await screen.findByRole("button", { name: "Kid" }));
  await screen.findByRole("button", { name: "Kid, account" });
  expect((await app.accounts.list()).activeId).toBe("kid");
  expect(app.router.state.location.pathname).toBe("/library");
});

it("opens My profile from the top bar", async () => {
  const app = renderApp("/library", { accounts: family, activeAccount: "kid" });
  fireEvent.click(await screen.findByRole("button", { name: "Kid, account" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "My profile" }));
  await waitFor(() => expect(app.router.state.location.pathname).toBe("/settings"));
  expect(app.router.state.location.search).toEqual({ tab: "profile" });
});

it("leaves a collection that belonged to the last account", async () => {
  const app = renderApp("/collections/collection-favourites", {
    accounts: family,
    activeAccount: "kid",
  });
  fireEvent.click(await screen.findByRole("button", { name: "Kid, account" }));
  fireEvent.click(await screen.findByRole("menuitem", { name: "Switch account" }));
  fireEvent.click(await screen.findByRole("button", { name: "Kid" }));
  await waitFor(() => expect(app.router.state.location.pathname).toBe("/collections"));
});
