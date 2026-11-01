import { cleanup, render } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { allStories } from "./test/stories";

afterEach(cleanup);

it("finds the stories", () => {
  expect(allStories.length).toBeGreaterThan(0);
});

it.each(allStories)("%s renders without errors or console warnings", (_, Story) => {
  const error = vi.spyOn(console, "error");
  const warn = vi.spyOn(console, "warn");
  render(<Story />);
  const problems = document.querySelector('[data-playground="theme-problems"]');
  expect(problems?.textContent ?? null, "theme problems").toBeNull();
  expect(error.mock.calls).toEqual([]);
  expect(warn.mock.calls).toEqual([]);
});
