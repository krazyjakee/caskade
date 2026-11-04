import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { DraftSlider } from "./DraftSlider";

afterEach(cleanup);

it("saves a step once it ends, and follows the saved value (#55)", () => {
  const commit = vi.fn();
  const view = render(
    <DraftSlider aria-label="Size" minValue={0} maxValue={10} value={4} commit={commit} />,
  );
  const thumb = screen.getByRole("slider", { name: "Size" });
  fireEvent.keyDown(thumb, { key: "ArrowRight" });
  fireEvent.keyUp(thumb, { key: "ArrowRight" });
  expect(commit).toHaveBeenCalledExactlyOnceWith(5);
  expect((thumb as HTMLInputElement).value).toBe("5");
  // The stored value wins once it comes back.
  view.rerender(
    <DraftSlider aria-label="Size" minValue={0} maxValue={10} value={7} commit={commit} />,
  );
  expect((thumb as HTMLInputElement).value).toBe("7");
});
