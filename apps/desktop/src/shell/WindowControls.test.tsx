import { checkAnatomy } from "@caskade/ui";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { WindowControls } from "./WindowControls";
import { fakeWindow } from "./windowFixture";

afterEach(cleanup);

it("drives the window and follows its maximized state", async () => {
  const appWindow = fakeWindow();
  const { container, unmount } = render(<WindowControls window={appWindow} />);
  const root = container.firstElementChild as HTMLElement;
  expect(root.dataset.status).toBe("restored");

  fireEvent.click(screen.getByRole("button", { name: "Minimize" }));
  expect(appWindow.calls).toEqual(["minimize"]);

  fireEvent.click(screen.getByRole("button", { name: "Maximize" }));
  await screen.findByRole("button", { name: "Restore" });
  expect(root.dataset.status).toBe("maximized");
  expect(checkAnatomy(root)).toEqual([]);

  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(appWindow.calls).toEqual(["minimize", "toggleMaximize", "close"]);

  unmount();
  await vi.waitFor(() => expect(appWindow.listeners.size).toBe(0));
});

it("keeps the controls out of the tab order", () => {
  render(<WindowControls window={fakeWindow()} />);
  for (const button of screen.getAllByRole("button"))
    expect(button.getAttribute("tabindex")).toBe("-1");
});
