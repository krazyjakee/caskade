import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import {
  type PluginManagerApi,
  PluginManagerProvider,
  type PluginSummary,
} from "../../api/pluginManager";
import { DropProvider } from "../../drop/DropProvider";
import { fakeDrops } from "../../drop/testing";
import { capabilityLines, PluginsSettings } from "./PluginsSettings";

afterEach(cleanup);

/** Cast: newer hosts add fields (e.g. #128's settingsError) the test needn't set. */
function summary(overrides: Partial<PluginSummary>): PluginSummary {
  return {
    settingsError: null,
    id: "dev.example.stats",
    name: "Playtime stats",
    version: "0.1.0",
    description: "Charts of your playtime.",
    author: null,
    folder: "/plugins/dev.example.stats",
    status: "enabled",
    errors: [],
    permissions: { network: [], fs: [], process: false, library: "read" },
    contributes: {
      slots: [],
      views: [{ id: "stats", title: "Stats", icon: null, modes: [] }],
      hooks: [],
      providers: [],
      commands: [],
      settings: null,
      emulation: [],
    },
    hasUi: true,
    hasBackend: false,
    failures: 0,
    lastError: null,
    ...overrides,
  } as PluginSummary;
}

function fake(initial: PluginSummary[]) {
  let plugins = structuredClone(initial);
  const calls: string[] = [];
  const offered = summary({
    id: "dev.example.hltb",
    name: "HowLongToBeat",
    version: "1.0.0",
    status: "disabled",
    permissions: {
      network: ["howlongtobeat.com"],
      fs: ["$HOME/ROMs:ro"],
      process: false,
      library: "none",
    },
  });
  const api: PluginManagerApi = {
    async list() {
      return structuredClone(plugins);
    },
    async setEnabled(id, enabled) {
      calls.push(`${enabled ? "enable" : "disable"} ${id}`);
      plugins = plugins.map((p) =>
        p.id === id ? { ...p, status: enabled ? "enabled" : "disabled" } : p,
      );
      return plugins.find((p) => p.id === id) as PluginSummary;
    },
    async chooseFolder() {
      calls.push("choose");
      return "/downloads/hltb";
    },
    async preview(folder) {
      calls.push(`preview ${folder}`);
      return { plugin: offered, installedVersion: null };
    },
    async uninstall(id) {
      calls.push(`uninstall ${id}`);
      plugins = plugins.filter((p) => p.id !== id);
    },
    async install(folder) {
      calls.push(`install ${folder}`);
      plugins = [...plugins, { ...offered, status: "enabled" }];
      return offered;
    },
  };
  return { api, calls };
}

function show(api: PluginManagerApi) {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <PluginManagerProvider api={api}>
        <PluginsSettings />
      </PluginManagerProvider>
    </QueryClientProvider>,
  );
}

it("describes permissions in plain words", () => {
  expect(
    capabilityLines({
      network: ["a.com", "b.com"],
      fs: ["$HOME/ROMs:ro", "/data:rw"],
      process: true,
      library: "write",
    }),
  ).toEqual([
    "Connect to a.com, b.com",
    "Read files in $HOME/ROMs",
    "Read and change files in /data",
    "Run programs and change how games are launched",
    "Read and change your library",
  ]);
});

it("installs a folder only after its permissions were reviewed and accepted", async () => {
  const { api, calls } = fake([summary({})]);
  show(api);
  const list = await screen.findByRole("list", { name: "Installed plugins" });
  expect(within(list).getByText("Playtime stats")).toBeTruthy();
  expect(within(list).getByText("Read your library")).toBeTruthy();

  // Review, then decline: nothing is installed.
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Install from folder" }));
  });
  let dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByText("HowLongToBeat")).toBeTruthy();
  const permissions = within(dialog).getByRole("list", { name: "Permissions" });
  expect(within(permissions).getByText("Connect to howlongtobeat.com")).toBeTruthy();
  expect(within(permissions).getByText("Read files in $HOME/ROMs")).toBeTruthy();
  // A UI half runs in the app's page with the app's access, whatever it declares.
  expect(
    within(permissions).getByText(
      "Run interface code inside Caskade, with the same access as the app",
    ),
  ).toBeTruthy();
  expect(within(permissions).queryByText("Nothing beyond its own storage")).toBeNull();
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(calls).toEqual(["choose", "preview /downloads/hltb"]);

  // Review, then accept.
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Install from folder" }));
  });
  dialog = await screen.findByRole("dialog");
  await act(async () => {
    fireEvent.click(within(dialog).getByRole("button", { name: "Install and allow" }));
  });
  expect(calls).toContain("install /downloads/hltb");
  expect(await screen.findByText("HowLongToBeat")).toBeTruthy();
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("a plugin folder dropped on the list opens its review without the picker", async () => {
  const { api, calls } = fake([summary({})]);
  const drops = fakeDrops(() => screen.getByRole("heading", { name: "Installed plugins" }));
  render(
    <DropProvider source={drops.source} hitTest={drops.hitTest}>
      <QueryClientProvider client={new QueryClient()}>
        <PluginManagerProvider api={api}>
          <PluginsSettings />
        </PluginManagerProvider>
      </QueryClientProvider>
    </DropProvider>,
  );
  await screen.findByRole("list", { name: "Installed plugins" });
  drops.drop("/downloads/hltb");
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByText("HowLongToBeat")).toBeTruthy();
  expect(calls).toEqual(["preview /downloads/hltb"]);
});

it("asks again when a plugin needs approval, and toggles the others", async () => {
  const { api, calls } = fake([
    summary({}),
    summary({
      id: "dev.example.grown",
      name: "Grown plugin",
      status: "needs_consent",
      permissions: { network: ["example.com"], fs: [], process: true, library: "none" },
    }),
  ]);
  show(api);
  await screen.findByText("Needs your approval");
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Review and allow" }));
  });
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByText("Run programs and change how games are launched")).toBeTruthy();
  await act(async () => {
    fireEvent.click(within(dialog).getByRole("button", { name: "Allow" }));
  });
  expect(calls).toContain("enable dev.example.grown");

  await act(async () => {
    fireEvent.click(screen.getByRole("switch", { name: "Enable Playtime stats" }));
  });
  expect(calls).toContain("disable dev.example.stats");
});

it("says a plugin can do nothing beyond its storage only without a UI half or permissions", async () => {
  const none = () => ({ network: [], fs: [], process: false, library: "none" as const });
  const { api } = fake([
    summary({
      id: "dev.example.backend",
      name: "Backend only",
      status: "needs_consent",
      hasUi: false,
      hasBackend: true,
      permissions: none(),
    }),
    summary({
      id: "dev.example.ui",
      name: "UI only",
      status: "needs_consent",
      hasUi: true,
      permissions: none(),
    }),
  ]);
  show(api);
  const list = await screen.findByRole("list", { name: "Installed plugins" });
  const review = async (name: string) => {
    const row = within(list).getByText(name).closest("li") as HTMLElement;
    await act(async () => {
      fireEvent.click(within(row).getByRole("button", { name: "Review and allow" }));
    });
    const dialog = await screen.findByRole("dialog");
    const lines = within(within(dialog).getByRole("list", { name: "Permissions" }))
      .getAllByRole("listitem")
      .map((item) => item.textContent);
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    return lines;
  };
  expect(await review("Backend only")).toEqual(["Nothing beyond its own storage"]);
  expect(await review("UI only")).toEqual([
    "Run interface code inside Caskade, with the same access as the app",
  ]);
});

it("uninstalls only after a second, explicit confirmation", async () => {
  const { api, calls } = fake([summary({})]);
  show(api);
  await screen.findByText("Playtime stats");
  fireEvent.click(screen.getByRole("button", { name: "Uninstall" }));
  fireEvent.click(screen.getByRole("button", { name: "Keep" }));
  expect(calls).toEqual([]);
  fireEvent.click(screen.getByRole("button", { name: "Uninstall" }));
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Uninstall Playtime stats" }));
  });
  expect(calls).toEqual(["uninstall dev.example.stats"]);
  expect(
    await screen.findByText(
      "No plugins yet. Get one from the catalogue below, install one from a folder, or drop its folder here.",
    ),
  ).toBeTruthy();
});

it("loads an unpacked folder in development builds and marks dev plugins", async () => {
  const { api, calls } = fake([summary({ id: "dev.example.mine", name: "Mine", dev: true })]);
  api.loadUnpacked = async (folder) => {
    calls.push(`unpacked ${folder}`);
  };
  show(api);
  const list = await screen.findByRole("list", { name: "Installed plugins" });
  expect(within(list).getByText("Dev")).toBeTruthy();
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Load unpacked" }));
  });
  expect(calls).toEqual(["choose", "unpacked /downloads/hltb"]);
});
