import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { DiscoveryApi, DiscoverySnapshot } from "../../../api/discovery";
import { createFixtureApi, createFixtureGames } from "../../../api/fixtures";
import { createLibraryQueryClient, LibraryProvider } from "../../../api/provider";
import type { SettingsApi, SettingsSnapshot } from "../../../api/settings";
import type { LibraryApi } from "../../../api/types";
import { LibraryDiscoverySettings } from "../LibraryDiscoverySettings";
import { SettingsProvider } from "../useSettings";
import { DiscoveryProvider, newest } from "./DiscoveryProvider";

afterEach(cleanup);
function backend() {
  let snapshot: DiscoverySnapshot = {
    revision: 4,
    os: "linux",
    checks: [{ id: "desktop", label: "Desktop entries", sources: ["desktop"] }],
    phase: "complete",
    launchers: [
      {
        source: "steam",
        found: true,
        path: "/fixture/Steam",
        version: "1",
        kind: { type: "native" },
      },
    ],
    sources: [
      {
        source: "desktop",
        count: 1,
        durationMs: 12,
        warnings: ["Skipped malformed fixture"],
        error: null,
      },
    ],
    found: 1,
    imported: 0,
    review: [
      {
        id: "review-1",
        title: "Possible Fixture Game",
        location: "/fixture/games/possible",
        sources: ["desktop"],
        evidence: ["Unity engine: +70"],
      },
    ],
    warnings: [],
    error: null,
    watching: false,
  };
  const listeners = new Set<(snapshot: DiscoverySnapshot) => void>();
  const emit = (next: DiscoverySnapshot) => {
    snapshot = next;
    for (const listener of listeners) listener(next);
  };
  const discovery: DiscoveryApi = {
    status: vi.fn(async () => snapshot),
    launchers: vi.fn(async () => snapshot),
    scan: vi.fn(async () => {
      emit({ ...snapshot, revision: snapshot.revision + 1, phase: "scanning" });
      return snapshot;
    }),
    cancel: vi.fn(async () => {
      emit({ ...snapshot, revision: snapshot.revision + 1, phase: "cancelled" });
      return snapshot;
    }),
    review: vi.fn(async () => {
      emit({ ...snapshot, revision: snapshot.revision + 1, review: [] });
      return snapshot;
    }),
    subscribe: async (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
  const values: Record<string, Record<string, unknown>> = {
    "core.discovery": {
      enabled: true,
      scanOnStartup: true,
      backgroundRefresh: true,
      refreshInterval: "24h",
      watchFolders: true,
      deepScans: {
        customFolders: false,
        externalDrives: false,
        portableExecutables: false,
        winePrefixes: false,
        fullDisk: false,
      },
      osChecks: { linux: { desktop: true } },
      scanPaths: [],
      ignoredEntries: [],
    },
    "core.launchers": { enabled: { steam: true } },
    "discovery.import": { seedPlaytime: true },
  };
  const defaults = structuredClone(values);
  const revisions: Record<string, number> = {};
  const settings: SettingsApi = {
    sections: async () =>
      Object.keys(values).map((namespace) => ({
        namespace,
        version: 1,
        schema: {},
        defaults: defaults[namespace],
      })),
    get: async (namespace) => ({
      namespace,
      version: 1,
      revision: revisions[namespace] ?? 0,
      value: structuredClone(values[namespace] ?? {}),
    }),
    set: vi.fn(async (namespace, value, revision): Promise<SettingsSnapshot> => {
      if (revision !== (revisions[namespace] ?? 0)) throw new Error("Conflict");
      values[namespace] = value;
      revisions[namespace] = revision + 1;
      return { namespace, version: 1, revision: revision + 1, value: structuredClone(value) };
    }),
    reset: async (namespace, revision) =>
      settings.set(namespace, structuredClone(defaults[namespace] ?? {}), revision),
    subscribe: async () => () => {},
  };
  return { discovery, settings, values };
}
function panel(fixture: ReturnType<typeof backend>, library: LibraryApi = createFixtureApi()) {
  return render(
    <LibraryProvider api={library} client={createLibraryQueryClient()}>
      <SettingsProvider api={fixture.settings}>
        <DiscoveryProvider api={fixture.discovery}>
          <LibraryDiscoverySettings />
        </DiscoveryProvider>
      </SettingsProvider>
    </LibraryProvider>,
  );
}
it("renders actual backend device/detection, reports and classifier evidence", async () => {
  panel(backend());
  await screen.findByText("Possible Fixture Game");
  // Launchers are switched on their own page (#48); this one links there.
  expect(screen.queryByRole("checkbox", { name: "steam" })).toBeNull();
  expect(screen.getByText("Desktop entries: 1 game")).toBeTruthy();
  expect(screen.getByText("Skipped malformed fixture")).toBeTruthy();
  expect(screen.getByText("Unity engine: +70")).toBeTruthy();
  expect(screen.queryByText("Merge duplicate entries")).toBeNull();
  expect(screen.queryByText("Search ROM directories")).toBeNull();
});
it("runs and cancels actual controller operations with phase feedback", async () => {
  const api = backend();
  panel(api);
  fireEvent.click(await screen.findByRole("button", { name: "Scan now" }));
  await waitFor(() => expect(api.discovery.scan).toHaveBeenCalledOnce());
  await waitFor(() =>
    expect(
      (screen.getByRole("button", { name: "Cancel scan" }) as HTMLButtonElement).disabled,
    ).toBe(false),
  );
  fireEvent.click(screen.getByRole("button", { name: "Cancel scan" }));
  await waitFor(() => expect(api.discovery.cancel).toHaveBeenCalledOnce());
});
it("bulk accepts or ignores only selected retained IDs with the snapshot revision", async () => {
  const api = backend();
  panel(api);
  fireEvent.click(await screen.findByRole("checkbox", { name: "Select all" }));
  fireEvent.click(screen.getByRole("button", { name: "Ignore selected" }));
  await waitFor(() =>
    expect(api.discovery.review).toHaveBeenCalledExactlyOnceWith(["review-1"], false, 4),
  );
  await screen.findByText("No games awaiting review.");
});
it("accepts selected records explicitly without requiring all candidates", async () => {
  const api = backend();
  panel(api);
  fireEvent.click(await screen.findByRole("checkbox", { name: "Possible Fixture Game" }));
  fireEvent.click(screen.getByRole("button", { name: "Accept selected (1)" }));
  await waitFor(() =>
    expect(api.discovery.review).toHaveBeenCalledExactlyOnceWith(["review-1"], true, 4),
  );
});
it("saves every visible scheduling, deep-scan, system check and history switch through settings", async () => {
  const api = backend();
  panel(api);
  await screen.findByText("Possible Fixture Game");
  for (const label of [
    "Full scan on startup",
    "Background refresh",
    "Watch library folders",
    "Import existing play history",
    "Custom folders",
    "External drives",
    "Portable executables",
    "Wine and Proton prefixes",
    "Full disk scan",
  ]) {
    const before = vi.mocked(api.settings.set).mock.calls.length;
    fireEvent.click(screen.getByRole("switch", { name: label }));
    await waitFor(() =>
      expect(vi.mocked(api.settings.set).mock.calls.length).toBeGreaterThan(before),
    );
  }
  fireEvent.click(screen.getByRole("checkbox", { name: "Desktop entries" }));
  await waitFor(() =>
    expect(
      ((api.values["core.discovery"]?.osChecks ?? {}) as Record<string, Record<string, boolean>>)
        .linux?.desktop,
    ).toBe(false),
  );
  expect(api.values["discovery.import"]?.seedPlaytime).toBe(false);
  fireEvent.change(screen.getByRole("textbox", { name: "Folder to search" }), {
    target: { value: "/fixture/custom-games" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Add folder" }));
  await waitFor(() =>
    expect(api.values["core.discovery"]?.scanPaths).toEqual(["/fixture/custom-games"]),
  );
  fireEvent.click(screen.getByRole("button", { name: "Remove /fixture/custom-games" }));
  await waitFor(() => expect(api.values["core.discovery"]?.scanPaths).toEqual([]));
});

it("persists the refresh interval, re-allows ignored entries and resets discovery preferences", async () => {
  const api = backend();
  const preferences = api.values["core.discovery"];
  if (!preferences) throw new Error("fixture preferences missing");
  preferences.ignoredEntries = [
    { source: "desktop", key: "ignored.desktop", title: "Old Tool" },
    { source: "steam", key: "440" },
  ];
  panel(api);
  await screen.findByText("Possible Fixture Game");
  // Named by title where the decision kept one, else by its key (#53).
  const ignored = screen.getByText("Old Tool").closest("li") as HTMLElement;
  expect(within(ignored).getByText("Desktop entries")).toBeTruthy();
  const older = screen.getByText("440").closest("li") as HTMLElement;
  expect(within(older).getByText("Steam")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: /Refresh interval/ }));
  fireEvent.click(await screen.findByRole("option", { name: "Weekly" }));
  await waitFor(() => expect(api.values["core.discovery"]?.refreshInterval).toBe("7d"));
  fireEvent.click(within(ignored).getByRole("button", { name: "Allow again" }));
  fireEvent.click(within(older).getByRole("button", { name: "Allow again" }));
  await waitFor(() => expect(api.values["core.discovery"]?.ignoredEntries).toEqual([]));
  await screen.findByText("Nothing ignored.");
  fireEvent.click(screen.getByRole("switch", { name: "Enable library discovery" }));
  await waitFor(() => expect(api.values["core.discovery"]?.enabled).toBe(false));
  expect((screen.getByRole("button", { name: "Scan now" }) as HTMLButtonElement).disabled).toBe(
    true,
  );
  // The reset names what goes back, and asks first (#49).
  fireEvent.click(screen.getByRole("button", { name: "Reset discovery settings" }));
  const confirm = await screen.findByRole("alertdialog", {
    name: "Reset Library Discovery settings?",
  });
  await within(confirm).findByText("Refresh interval");
  expect(within(confirm).getByText("Enable library discovery")).toBeTruthy();
  expect(within(confirm).queryByText("Full scan on startup")).toBeNull();
  expect(api.values["core.discovery"]?.enabled).toBe(false);
  fireEvent.click(within(confirm).getByRole("button", { name: "Reset" }));
  await waitFor(() => expect(api.values["core.discovery"]?.enabled).toBe(true));
  expect(api.values["core.discovery"]?.refreshInterval).toBe("24h");
});

it("lists hidden games and shows one again", async () => {
  const library = createFixtureApi(createFixtureGames(3));
  await library.setHidden?.("fixture-1", true);
  await library.setHidden?.("fixture-2", true);
  panel(backend(), library);
  const list = await screen.findByRole("list", { name: "Hidden games" });
  expect(list.textContent).toContain("Fixture Game 0002");
  expect(list.textContent).toContain("Fixture Game 0003");
  fireEvent.click(screen.getByRole("button", { name: "Show Fixture Game 0002 again" }));
  await waitFor(() => expect(list.textContent).not.toContain("Fixture Game 0002"));
  expect((await library.hiddenGames?.())?.map((game) => game.id)).toEqual(["fixture-2"]);
  const page = await library.listGames({ filter: "all", sort: "title", offset: 0, limit: 10 });
  expect(page.items.map((game) => game.id)).toEqual(["fixture-0", "fixture-1"]);
});

it("keeps the later snapshot, and takes the same revision again for its text", async () => {
  const shown = await backend().discovery.status();
  const older = { ...shown, revision: shown.revision - 1 };
  expect(newest(shown, older)).toBe(shown);
  const relabelled = {
    ...shown,
    checks: [{ id: "desktop", label: "Skrivbordsposter", sources: ["desktop"] }],
  };
  expect(newest(shown, relabelled)).toBe(relabelled);
});
