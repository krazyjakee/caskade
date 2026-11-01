import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { ErrorNotice } from "./ErrorNotice";

afterEach(cleanup);

it("keeps the details behind a toggle a keyboard or pad reaches", () => {
  render(
    <ErrorNotice
      title="Couldn't load your library"
      summary="The library database couldn't be read."
      hints={["Restart Caskade."]}
      detail="database is locked"
      reference="1a2b3c4d"
    />,
  );
  const alert = screen.getByRole("alert");
  expect(alert.textContent).toContain("The library database couldn't be read.");
  expect(alert.textContent).toContain("Restart Caskade.");
  expect(screen.queryByText("database is locked")).toBeNull();

  const toggle = screen.getByRole("button", { name: "Show details" });
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
  expect(toggle.tabIndex).not.toBe(-1);
  fireEvent.click(toggle);
  expect(screen.getByText("database is locked")).toBeTruthy();
  expect(screen.getByText("Reference: 1a2b3c4d")).toBeTruthy();
  const hide = screen.getByRole("button", { name: "Hide details" });
  expect(hide.getAttribute("aria-expanded")).toBe("true");
  expect(alert.getAttribute("data-expanded")).toBe("true");
});

it("has no toggle when there is nothing more to show", () => {
  render(<ErrorNotice summary="A PIN is 4 to 8 digits." variant="inline" />);
  expect(screen.queryByRole("button")).toBeNull();
  expect(screen.getByRole("alert").hasAttribute("data-expanded")).toBe(false);
});
