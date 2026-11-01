import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, onTestFinished, vi } from "vitest";
import { fakeLayout, fakeViewport, renderApp } from "../test/harness";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const overlay = () => document.querySelector('[data-cs="type-search"]');
const box = () => screen.queryByRole("textbox", { name: "Search games" });
const count = () => overlay()?.querySelector('[data-part="count"]')?.textContent;
const cards = () =>
  [...document.querySelectorAll('[data-cs="game-card"] [data-part="title"]')].map(
    (title) => title.textContent,
  );

it("in the Library, typing filters the games, with the count; Backspace on an empty line closes it", async () => {
  fakeViewport();
  const app = renderApp("/library?filter=all");
  await screen.findAllByText("Fixture Game 0001");

  fireEvent.keyDown(document.body, { key: "F" });
  const input = await screen.findByRole<HTMLInputElement>("textbox", { name: "Search games" });
  expect(input.value).toBe("F");
  expect(document.activeElement).toBe(input);
  expect(overlay()?.getAttribute("role")).toBe("dialog");
  await waitFor(() => expect(app.router.state.location.search).toMatchObject({ q: "F" }));

  fireEvent.change(input, { target: { value: "Fixture Game 0003" } });
  await waitFor(() => expect(cards()).toEqual(["Fixture Game 0003"]));
  // The filter stays the one the library was on.
  expect(app.router.state.location.search).toMatchObject({
    filter: "all",
    q: "Fixture Game 0003",
  });
  await waitFor(() => expect(count()).toBe("1 game"));

  fireEvent.change(input, { target: { value: "" } });
  await waitFor(() => expect(cards()).toHaveLength(8));
  expect(app.router.state.location.search).not.toHaveProperty("q");
  fireEvent.keyDown(input, { key: "Backspace" });
  expect(box()).toBeNull();
});

it("Enter goes to the results; Escape puts the earlier search back", async () => {
  fakeViewport();
  const app = renderApp("/library?filter=all&q=Fixture");
  await screen.findAllByText("Fixture Game 0001");

  fireEvent.keyDown(document.body, { key: "8" });
  const input = await screen.findByRole<HTMLInputElement>("textbox", { name: "Search games" });
  await waitFor(() => expect(cards()).toEqual(["Fixture Game 0008"]));
  fireEvent.keyDown(input, { key: "Enter" });
  expect(box()).toBeNull();
  await waitFor(() =>
    expect(document.activeElement?.closest('[data-cs="game-card"]')?.textContent).toContain(
      "Fixture Game 0008",
    ),
  );
  expect(app.router.state.location.search).toMatchObject({ q: "8" });

  fireEvent.keyDown(document.activeElement ?? document.body, { key: "2" });
  const again = await screen.findByRole<HTMLInputElement>("textbox", { name: "Search games" });
  expect(again.value).toBe("2");
  fireEvent.keyDown(again, { key: "Escape" });
  expect(box()).toBeNull();
  await waitFor(() => expect(app.router.state.location.search).toMatchObject({ q: "8" }));
});

it("anywhere else, it counts the whole library and Enter opens the Library on the results", async () => {
  fakeViewport();
  const app = renderApp("/settings?tab=general");
  await screen.findByRole("heading", { name: "Settings" });
  // `/` opens it empty.
  fireEvent.keyDown(document.body, { key: "/" });
  const input = await screen.findByRole<HTMLInputElement>("textbox", { name: "Search games" });
  expect(input.value).toBe("");
  fireEvent.change(input, { target: { value: "Game 0002" } });
  await waitFor(() => expect(count()).toBe("1 game"));
  // Typing here doesn't leave Settings.
  expect(app.router.state.location.pathname).toBe("/settings");
  fireEvent.keyDown(input, { key: "Enter" });
  await waitFor(() => expect(app.router.state.location.pathname).toBe("/library"));
  expect(app.router.state.location.search).toMatchObject({ filter: "all", q: "Game 0002" });
  await waitFor(() => expect(cards()).toEqual(["Fixture Game 0002"]));
});

it("leaves keys alone in text fields and with modifiers; the topbar box keeps Ctrl K", async () => {
  fakeViewport();
  renderApp("/library?filter=all");
  await screen.findAllByText("Fixture Game 0001");
  fireEvent.keyDown(document.body, { key: "k", ctrlKey: true });
  expect(document.activeElement).toBe(screen.getByRole("combobox", { name: "Search games" }));
  fireEvent.keyDown(document.body, { key: " " });
  fireEvent.keyDown(document.body, { key: "Shift" });
  fireEvent.keyDown(screen.getByRole("combobox", { name: "Search games" }), { key: "a" });
  expect(overlay()).toBeNull();
});

it("in console mode, action keys keep their actions, / opens it, and Escape cancels", async () => {
  fakeViewport();
  // Boxes for spatial navigation and the overlays.
  const layout = fakeLayout();
  onTestFinished(layout);
  const app = renderApp("/library?filter=all", { mode: "console" });
  await screen.findAllByText("Fixture Game 0001");
  // `i` is Details in console mode (the first game's quick look); it
  // doesn't start a search.
  await waitFor(() =>
    expect(document.activeElement?.closest('[data-cs="game-card"]')).not.toBeNull(),
  );
  fireEvent.keyDown(document.activeElement ?? document.body, { key: "i" });
  expect(overlay()).toBeNull();
  await waitFor(() => expect(document.querySelector('[data-cs="quick-look"]')).not.toBeNull());
  fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
  await waitFor(() => expect(document.querySelector('[data-cs="quick-look"]')).toBeNull());

  // The search key: the overlay, not the on-screen keyboard.
  fireEvent.keyDown(document.body, { key: "/" });

  const input = await screen.findByRole<HTMLInputElement>("textbox", { name: "Search games" });
  expect(document.querySelector('[data-cs="osk"]')).toBeNull();
  // Once open, action letters type.
  fireEvent.change(input, { target: { value: "5" } });
  await waitFor(() => expect(cards()).toEqual(["Fixture Game 0005"]));
  fireEvent.keyDown(input, { key: "Escape" });
  expect(overlay()).toBeNull();
  await waitFor(() => expect(app.router.state.location.search).not.toHaveProperty("q"));
});
