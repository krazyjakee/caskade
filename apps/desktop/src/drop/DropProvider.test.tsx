import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { type DropHandler, DropProvider, useDropFallback, useDropZone } from "./DropProvider";
import { fakeDrops } from "./testing";

afterEach(cleanup);

function Zone({
  name,
  onDrop,
  children,
}: {
  name: string;
  onDrop: DropHandler | null;
  children?: React.ReactNode;
}) {
  const zone = useDropZone<HTMLDivElement>(onDrop);
  return (
    <div ref={zone.ref} data-testid={name} data-active={zone.active || undefined}>
      {children}
    </div>
  );
}
function Fallback({ onDrop }: { onDrop: DropHandler }) {
  useDropFallback(onDrop);
  return null;
}

let target: string | null = null;
const drops = fakeDrops(() => (target ? screen.getByTestId(target) : document.body));

function renderZones(inner: DropHandler | null) {
  const outer = vi.fn();
  const fallback = vi.fn();
  render(
    <DropProvider source={drops.source} hitTest={drops.hitTest}>
      <Zone name="outer" onDrop={outer}>
        <Zone name="inner" onDrop={inner}>
          <span data-testid="text">Text</span>
        </Zone>
      </Zone>
      <div data-testid="elsewhere" />
      <Fallback onDrop={fallback} />
    </DropProvider>,
  );
  return { outer, fallback };
}

it("hands a drop to the innermost zone under the pointer, highlighted while dragged over", async () => {
  const inner = vi.fn();
  const { outer, fallback } = renderZones(inner);
  await waitFor(() =>
    expect(screen.getByTestId("inner").hasAttribute("data-drop-zone")).toBe(true),
  );
  target = "text";
  drops.over();
  expect(screen.getByTestId("inner").hasAttribute("data-drop-active")).toBe(true);
  expect(screen.getByTestId("inner").dataset.active).toBe("true");
  expect(screen.getByTestId("outer").hasAttribute("data-drop-active")).toBe(false);
  drops.drop("/games/Celeste", "/games/Hades.desktop");
  expect(inner).toHaveBeenCalledExactlyOnceWith(["/games/Celeste", "/games/Hades.desktop"]);
  expect(outer).not.toHaveBeenCalled();
  expect(fallback).not.toHaveBeenCalled();
  expect(screen.getByTestId("inner").hasAttribute("data-drop-active")).toBe(false);
});

it("skips a zone that is off, and leaving clears the highlight", async () => {
  const { outer } = renderZones(null);
  await waitFor(() =>
    expect(screen.getByTestId("outer").hasAttribute("data-drop-zone")).toBe(true),
  );
  target = "text";
  drops.over();
  expect(screen.getByTestId("outer").hasAttribute("data-drop-active")).toBe(true);
  drops.leave();
  expect(screen.getByTestId("outer").hasAttribute("data-drop-active")).toBe(false);
  drops.drop("/games/Celeste");
  expect(outer).toHaveBeenCalledExactlyOnceWith(["/games/Celeste"]);
});

it("sends a drop outside every zone to the fallback", async () => {
  const inner = vi.fn();
  const { outer, fallback } = renderZones(inner);
  await waitFor(() =>
    expect(screen.getByTestId("outer").hasAttribute("data-drop-zone")).toBe(true),
  );
  target = "elsewhere";
  drops.drop("/downloads/setup.exe");
  expect(fallback).toHaveBeenCalledExactlyOnceWith(["/downloads/setup.exe"]);
  expect(inner).not.toHaveBeenCalled();
  expect(outer).not.toHaveBeenCalled();
});
