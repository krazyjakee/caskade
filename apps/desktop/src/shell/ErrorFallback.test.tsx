import { checkAnatomy } from "@caskade/ui";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Link,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { AppErrorBoundary, ViewError } from "./ErrorFallback";

afterEach(cleanup);

it("shows a failing view's error in place and keeps the shell", async () => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  let fail = true;
  const root = createRootRoute({
    component: () => (
      <div>
        <nav>
          <Link to="/library">Library</Link>
        </nav>
        <Outlet />
      </div>
    ),
  });
  const broken = createRoute({
    getParentRoute: () => root,
    path: "/",
    component: () => {
      if (fail) throw new Error("bad game data");
      return <p>Recovered</p>;
    },
  });
  const library = createRoute({
    getParentRoute: () => root,
    path: "/library",
    component: () => <p>Library view</p>,
  });
  const router = createRouter({
    routeTree: root.addChildren([broken, library]),
    history: createMemoryHistory({ initialEntries: ["/"] }),
    defaultErrorComponent: ViewError,
  });
  render(<RouterProvider router={router} />);

  const alert = await screen.findByRole("alert");
  expect(alert.textContent).toContain("bad game data");
  expect(checkAnatomy(alert)).toEqual([]);
  expect(screen.getByRole("link", { name: "Library" })).toBeTruthy();

  fail = false;
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  await screen.findByText("Recovered");
});

function BrokenProvider(): never {
  throw new Error("provider failed");
}

it("catches what the router can't", () => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  render(
    <AppErrorBoundary>
      <BrokenProvider />
    </AppErrorBoundary>,
  );
  expect(screen.getByRole("alert").textContent).toContain("provider failed");
  expect(screen.getByRole("button", { name: "Reload" })).toBeTruthy();
});
