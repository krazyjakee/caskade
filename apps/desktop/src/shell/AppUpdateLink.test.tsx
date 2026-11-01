import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { type AppUpdateApi, AppUpdateProvider, type AppUpdateStatus } from "../api/appUpdate";
import { AppUpdateLink } from "./AppUpdateLink";

afterEach(cleanup);

const base: AppUpdateStatus = {
  currentVersion: "0.1.0",
  unavailable: null,
  phase: "upToDate",
  release: null,
  checkedAt: 0,
  downloaded: 0,
  total: null,
  error: null,
};

function renderLink(status: AppUpdateStatus) {
  const api: AppUpdateApi = {
    status: async () => status,
    check: async () => status,
    install: async () => status,
    restart: async () => {},
    subscribe: () => () => {},
  };
  const root = createRootRoute({
    component: () => (
      <AppUpdateProvider api={api}>
        <AppUpdateLink />
        <span>Shell</span>
      </AppUpdateProvider>
    ),
  });
  const router = createRouter({
    routeTree: root.addChildren([createRoute({ getParentRoute: () => root, path: "/" })]),
    history: createMemoryHistory({ initialEntries: ["/"] }),
  });
  render(
    <QueryClientProvider client={new QueryClient()}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
}

const release = { version: "0.2.0", notes: null, date: null };

it("offers a release a check found, linking to the General settings", async () => {
  renderLink({ ...base, phase: "available", release });
  const link = await screen.findByRole("link", { name: "Caskade v0.2.0 available" });
  expect(link.getAttribute("href")).toBe("/settings?tab=general");
});

it("asks for the restart once the update is installed", async () => {
  renderLink({ ...base, phase: "ready", release });
  expect(await screen.findByRole("link", { name: "Restart for Caskade v0.2.0" })).toBeTruthy();
});

it("stays out of the way otherwise", async () => {
  renderLink(base);
  await screen.findByText("Shell");
  expect(screen.queryByRole("link")).toBeNull();
});
