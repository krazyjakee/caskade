import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { GameArt } from "./GameArt";

afterEach(cleanup);
it("loads the requested artwork thumbnail lazily without substituting an original", () => {
  const thumbnail = `caskade-asset://localhost/${"a".repeat(64)}/thumb-320.webp`;
  const { container } = render(<GameArt game={{ title: "Original fixture" }} image={thumbnail} />);
  const image = container.querySelector("img");
  expect(image?.getAttribute("src")).toBe(thumbnail);
  expect(image?.getAttribute("loading")).toBe("lazy");
  expect(container.innerHTML).not.toContain("original.webp");
});

it("shows a blank cover when the cached image can't be loaded", () => {
  const thumbnail = `caskade-asset://localhost/${"b".repeat(64)}/thumb-320.webp`;
  const { container } = render(<GameArt game={{ title: "Evicted fixture" }} image={thumbnail} />);
  fireEvent.error(container.querySelector("img") as HTMLImageElement);
  const art = container.firstElementChild as HTMLElement;
  expect(container.querySelector("img")).toBeNull();
  expect(art.dataset.source).toBe("placeholder");
  expect(art.textContent).toBe("");
});

it("shows a blank cover without text when there is no artwork", () => {
  const { container } = render(<GameArt game={{ title: "Bare fixture" }} />);
  const art = container.firstElementChild as HTMLElement;
  expect(art.dataset.source).toBe("placeholder");
  expect(art.textContent).toBe("");
});
