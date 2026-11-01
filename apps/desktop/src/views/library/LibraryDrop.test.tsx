import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { createFixtureApi, createFixtureGames } from "../../api/fixtures";
import { createInstallFixtureApi } from "../../api/installFixtures";
import {
  type InstallRequest,
  type InstallRequestApi,
  InstallRequestContext,
} from "../../api/installRequests";
import { createLibraryQueryClient, LibraryProvider } from "../../api/provider";
import { DropProvider } from "../../drop/DropProvider";
import { fakeDrops } from "../../drop/testing";
import { ActivityProvider } from "../activity/ActivityProvider";
import { InstallIngress } from "../activity/InstallIngress";
import { LibraryView } from "./LibraryView";

afterEach(cleanup);

const planned: InstallRequest = {
  requestId: "dropped-1",
  kind: "path",
  value: "/games/Hades",
  plan: {
    dropped: "/home/me/Desktop/Hades.desktop",
    kind: "shortcut",
    input: "/games/Hades",
    title: "Hades",
    launch: { path: "Hades.x86_64", args: ["--windowed"], workingDir: null },
  },
};

function requestsApi(): InstallRequestApi & { emit: (request: InstallRequest) => void } {
  let listener: (request: InstallRequest) => void = () => {};
  return {
    emit: (request) => listener(request),
    pending: vi.fn(async () => []),
    request: vi.fn(async (value: string) => ({
      requestId: "plain-1",
      kind: "path" as const,
      value,
    })),
    requestDrop: vi.fn(async (path: string) => {
      if (path.endsWith("unins000.exe"))
        throw new Error("unins000.exe is an uninstaller, not a game");
      // The core emits every request it queues.
      listener(planned);
      return planned;
    }),
    confirm: vi.fn(async () => "job-1"),
    dismiss: vi.fn(async () => {}),
    subscribe: vi.fn(async (next) => {
      listener = next;
      return () => {};
    }),
  };
}

async function renderLibrary() {
  const requests = requestsApi();
  let target: () => Element | null = () => null;
  const drops = fakeDrops(() => target());
  const root = createRootRoute({ component: Outlet });
  const route = createRoute({
    getParentRoute: () => root,
    path: "/library",
    component: () => (
      <ActivityProvider api={createInstallFixtureApi()}>
        <LibraryView filter="all" />
      </ActivityProvider>
    ),
  });
  const router = createRouter({
    routeTree: root.addChildren([route]),
    history: createMemoryHistory({ initialEntries: ["/library"] }),
  });
  render(
    <DropProvider source={drops.source} hitTest={drops.hitTest}>
      <InstallRequestContext.Provider value={requests}>
        <LibraryProvider
          api={createFixtureApi(createFixtureGames(0))}
          client={createLibraryQueryClient()}
        >
          <RouterProvider router={router} />
          <InstallIngress api={requests} />
        </LibraryProvider>
      </InstallRequestContext.Provider>
    </DropProvider>,
  );
  await screen.findByRole("heading", { name: "Library" });
  target = () => screen.getByRole("heading", { name: "Library" });
  return {
    requests,
    drops,
    pointAt: (element: () => Element | null) => {
      target = element;
    },
  };
}

it("a shortcut dropped on the library opens the install confirmation with what it installs", async () => {
  const { requests, drops } = await renderLibrary();
  drops.over();
  expect(screen.getByText("Drop to install into your library")).toBeTruthy();
  drops.drop("/home/me/Desktop/Hades.desktop");
  expect(requests.requestDrop).toHaveBeenCalledExactlyOnceWith("/home/me/Desktop/Hades.desktop");
  expect(requests.request).not.toHaveBeenCalled();
  const dialog = await screen.findByRole("dialog");
  within(dialog).getByRole("heading", { name: "Install Hades?" });
  expect(within(dialog).getByText("/home/me/Desktop/Hades.desktop")).toBeTruthy();
  expect(within(dialog).getByText("/games/Hades")).toBeTruthy();
  expect(within(dialog).getByText("Hades.x86_64 --windowed")).toBeTruthy();
  expect(requests.confirm).not.toHaveBeenCalled();
  fireEvent.click(within(dialog).getByRole("button", { name: "Install" }));
  await waitFor(() => expect(requests.confirm).toHaveBeenCalledExactlyOnceWith("dropped-1"));
});

it("says why a dropped file can't be installed", async () => {
  const { drops } = await renderLibrary();
  drops.drop("/games/Foo/unins000.exe");
  expect((await screen.findByRole("alert")).textContent).toContain(
    "unins000.exe is an uninstaller, not a game",
  );
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("a drop outside the library keeps the plain install confirmation", async () => {
  const { requests, drops, pointAt } = await renderLibrary();
  pointAt(() => document.body);
  drops.drop("/downloads/setup.exe");
  expect(requests.request).toHaveBeenCalledExactlyOnceWith("/downloads/setup.exe");
  expect(requests.requestDrop).not.toHaveBeenCalled();
  await screen.findByRole("heading", { name: "Install this game?" });
});
