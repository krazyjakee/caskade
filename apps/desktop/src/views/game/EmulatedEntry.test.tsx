import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
} from "@tanstack/react-router";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import {
  type EmulatorTarget,
  type GameArgs,
  type GameEmulation,
  type GameEmulationApi,
  GameEmulationProvider,
} from "../../api/gameEmulation";
import { EmulatedEntry } from "./EmulatedEntry";

afterEach(cleanup);

const profile = {
  emulatorId: "ra",
  profileId: "snes",
  emulatorName: "RetroArch",
  profileName: "Snes9x",
  available: true,
};

function fakeApi(start: GameEmulation) {
  let game = structuredClone(start);
  const calls: { target: EmulatorTarget; args: GameArgs }[] = [];
  const api: GameEmulationApi = {
    async gameEmulation() {
      return structuredClone(game);
    },
    async setGameEmulation(_entryId, target, args) {
      calls.push({ target, args });
      game = {
        ...game,
        target,
        args: args.type === "append" ? { type: "append", args: args.args } : { type: "default" },
        profile: target.type === "profile" ? profile : null,
        readiness: { type: "ready" },
        message: null,
      };
      return structuredClone(game);
    },
    async splitArgs(text) {
      return text === '--config "my file.cfg"' ? ["--config", "my file.cfg"] : text.split(" ");
    },
  };
  return { api, calls };
}

const notReady: GameEmulation = {
  entryId: "e1",
  platformId: "nintendo_snes",
  platform: {
    id: "nintendo_snes",
    name: "Super Nintendo",
    short: "SNES",
    extensions: ["sfc"],
    bios: [],
  },
  target: { type: "platform_default" },
  args: { type: "default" },
  discs: [
    { name: "Disc 1", path: "/roms/Game (Disc 1).cue", exists: true, size: 10 },
    { name: "Disc 2", path: "/roms/Game (Disc 2).cue", exists: false, size: 10 },
  ],
  profile: null,
  compatible: [profile],
  bios: [
    {
      file: "scph5501.bin",
      required: true,
      found: false,
      state: "missing",
      path: null,
      platform: "sony_playstation",
    },
    {
      file: "dc/optional.bin",
      required: false,
      found: null,
      state: null,
      path: null,
      platform: null,
    },
  ],
  readiness: { type: "no_emulator" },
  message: "No emulator for SNES — configure",
};

function show(api: GameEmulationApi) {
  const root = createRootRoute({ component: Outlet });
  const game = createRoute({
    getParentRoute: () => root,
    path: "/",
    component: () => <EmulatedEntry entryId="e1" />,
  });
  const settings = createRoute({
    getParentRoute: () => root,
    path: "/settings",
    component: () => null,
  });
  const router = createRouter({
    routeTree: root.addChildren([game, settings]),
    history: createMemoryHistory({ initialEntries: ["/"] }),
  });
  render(
    <QueryClientProvider client={new QueryClient()}>
      <GameEmulationProvider api={api}>
        <RouterProvider router={router} />
      </GameEmulationProvider>
    </QueryClientProvider>,
  );
}

it("shows what to configure, then pins a profile and saves extra arguments", async () => {
  const { api, calls } = fakeApi(notReady);
  show(api);
  const status = await screen.findByRole("status");
  expect(status.textContent).toContain("No emulator for SNES — configure");
  expect(within(status).getByRole("link", { name: "Configure" }).getAttribute("href")).toContain(
    "tab=emulators",
  );
  const discs = screen.getByRole("list", { name: "Discs" });
  expect(within(discs).getByText("Disc 2").closest("[data-tone]")?.getAttribute("data-tone")).toBe(
    "danger",
  );
  const bios = screen.getByRole("list", { name: "BIOS" });
  expect(within(bios).getByText("Missing")).toBeTruthy();
  expect(within(bios).getByText("optional")).toBeTruthy();

  // Launch with → the compatible profile.
  fireEvent.click(screen.getByRole("button", { name: /Launch with/ }));
  fireEvent.click(await screen.findByRole("option", { name: "RetroArch · Snes9x" }));
  await screen.findByText("Ready");
  expect(calls[0]?.target).toEqual({ type: "profile", emulatorId: "ra", profileId: "snes" });
  expect(screen.queryByRole("status")).toBeNull();

  // Extra arguments are split by the backend and appended.
  const field = screen.getByRole("textbox", { name: /Extra arguments/ });
  fireEvent.change(field, { target: { value: '--config "my file.cfg"' } });
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
  });
  await screen.findByDisplayValue("--config my file.cfg");
  expect(calls[1]).toEqual({
    target: { type: "profile", emulatorId: "ra", profileId: "snes" },
    args: { type: "append", args: ["--config", "my file.cfg"] },
  });
});

it("renders nothing for a game that isn't emulated", async () => {
  const { api } = fakeApi({ ...notReady, readiness: { type: "not_emulated" }, message: null });
  show(api);
  await act(async () => {});
  expect(screen.queryByText("Launch with")).toBeNull();
});
