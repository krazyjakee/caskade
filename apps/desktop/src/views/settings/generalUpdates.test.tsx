/** Links to Caskade updates land on them, not on the top of General (#68). */
import { cleanup, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { renderApp } from "../../test/harness";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  delete (Element.prototype as Partial<Element>).scrollIntoView;
});

const scrolls = () => {
  const scrolled = vi.fn();
  Element.prototype.scrollIntoView = scrolled;
  return scrolled;
};

it("scrolls to Caskade updates when a link asks for them", async () => {
  const scrolled = scrolls();
  renderApp("/settings?tab=general&section=updates");
  await screen.findByRole("heading", { name: "Caskade updates" });
  await waitFor(() => expect(scrolled).toHaveBeenCalledWith({ block: "start" }));
});

it("leaves General at its top otherwise", async () => {
  const scrolled = scrolls();
  renderApp("/settings?tab=general");
  await screen.findByRole("heading", { name: "Caskade updates" });
  expect(scrolled).not.toHaveBeenCalled();
});
