import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { createEmulationFixtureApi } from "../../api/emulationFixtures";
import { createSettingsFixture } from "../../test/settingsFixture";
import { EmulatorSettings } from "./EmulatorSettings";
import { EmulationProvider } from "./emulators/EmulationProvider";
import { SettingsProvider } from "./useSettings";

afterEach(cleanup);
it("opens the section named in ?section= and writes section changes back to the URL", async () => {
  const root = createRootRoute({ component: Outlet });
  const settings = createRoute({
    getParentRoute: () => root,
    path: "/settings",
    validateSearch: (search: Record<string, unknown>) => ({
      tab: "emulators" as const,
      ...(typeof search.section === "string" && { section: search.section }),
    }),
    component: EmulatorSettings,
  });
  const router = createRouter({
    routeTree: root.addChildren([settings]),
    history: createMemoryHistory({ initialEntries: ["/settings?tab=emulators&section=platforms"] }),
  });
  const preferences = createSettingsFixture();
  render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <SettingsProvider api={preferences.api}>
        <EmulationProvider api={createEmulationFixtureApi()}>
          <RouterProvider router={router} />
        </EmulationProvider>
      </SettingsProvider>
    </QueryClientProvider>,
  );
  await screen.findByText("Super Nintendo");
  expect(screen.getByRole("radio", { name: "Platforms" }).getAttribute("aria-checked")).toBe(
    "true",
  );
  fireEvent.click(screen.getByRole("radio", { name: "Emulators" }));
  await waitFor(() => expect(router.state.location.search).toMatchObject({ section: "emulators" }));
  await screen.findByRole("listbox", { name: "Emulators" });

  // Desktop full screen is `core.emulation.desktopFullscreen`.
  fireEvent.click(await screen.findByRole("switch", { name: "Desktop full screen" }));
  await waitFor(() =>
    expect(preferences.values["core.emulation"]).toMatchObject({ desktopFullscreen: true }),
  );
});
