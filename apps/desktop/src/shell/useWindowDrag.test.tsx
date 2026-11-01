import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import type { AppWindow } from "./appWindow";
import { useWindowDrag } from "./useWindowDrag";
import { fakeWindow } from "./windowFixture";

afterEach(cleanup);

function TitleBar({ appWindow }: { appWindow: AppWindow }) {
  useWindowDrag(appWindow);
  return (
    <header data-window-drag data-testid="bar">
      <span data-testid="title">Caskade</span>
      <button type="button">Action</button>
      <div data-window-drag="false" data-testid="search" />
    </header>
  );
}

it("moves the window once a press on the bar travels", () => {
  const appWindow = fakeWindow();
  render(<TitleBar appWindow={appWindow} />);
  fireEvent.mouseDown(screen.getByTestId("title"), {
    button: 0,
    detail: 1,
    screenX: 10,
    screenY: 10,
  });
  fireEvent.mouseMove(document, { buttons: 1, screenX: 12, screenY: 11 });
  expect(appWindow.calls).toEqual([]);
  fireEvent.mouseMove(document, { buttons: 1, screenX: 20, screenY: 10 });
  fireEvent.mouseMove(document, { buttons: 1, screenX: 40, screenY: 10 });
  expect(appWindow.calls).toEqual(["startDragging"]);
});

it("maximizes on a double click, and a plain click does nothing", () => {
  const appWindow = fakeWindow();
  render(<TitleBar appWindow={appWindow} />);
  const bar = screen.getByTestId("bar");
  fireEvent.mouseDown(bar, { button: 0, detail: 1, screenX: 10, screenY: 10 });
  fireEvent.mouseUp(bar);
  fireEvent.mouseMove(document, { buttons: 0, screenX: 40, screenY: 10 });
  expect(appWindow.calls).toEqual([]);
  fireEvent.mouseDown(bar, { button: 0, detail: 2, screenX: 10, screenY: 10 });
  expect(appWindow.calls).toEqual(["toggleMaximize"]);
});

it("leaves controls and opted-out areas alone", () => {
  const appWindow = fakeWindow();
  render(<TitleBar appWindow={appWindow} />);
  for (const target of [screen.getByRole("button"), screen.getByTestId("search")]) {
    fireEvent.mouseDown(target, { button: 0, detail: 2, screenX: 10, screenY: 10 });
    fireEvent.mouseDown(target, { button: 0, detail: 1, screenX: 10, screenY: 10 });
    fireEvent.mouseMove(document, { buttons: 1, screenX: 60, screenY: 10 });
  }
  expect(appWindow.calls).toEqual([]);
});
