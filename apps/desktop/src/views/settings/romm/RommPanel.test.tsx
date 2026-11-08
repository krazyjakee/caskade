import { checkAnatomy } from "@caskade/ui";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createEmulationFixtureApi } from "../../../api/emulationFixtures";
import { CommandFailure } from "../../../api/reply";
import type {
  RommApi,
  RommConnection,
  RommDownload,
  RommOverview,
  RommServer,
  RommServerStatus,
  RommSettings,
} from "../../../api/romm";
import { EmulationProvider } from "../emulators/EmulationProvider";
import { RommPanel } from "./RommPanel";
import { mergeDownload, RommApiContext } from "./useRomm";

afterEach(cleanup);

const report: RommConnection = {
  version: "4.4.0",
  platforms: [
    {
      platform: {
        id: 1,
        slug: "snes",
        fsSlug: "snes",
        name: "Super Nintendo Entertainment System",
        customName: null,
        romCount: 312,
        igdbId: 19,
      },
      suggested: "snes",
    },
    {
      platform: {
        id: 3,
        slug: "vectrex",
        fsSlug: "vectrex",
        name: "Vectrex",
        customName: null,
        romCount: 4,
        igdbId: null,
      },
      suggested: null,
    },
    {
      platform: {
        id: 4,
        slug: "3do",
        fsSlug: "3do",
        name: "3DO Interactive Multiplayer",
        customName: null,
        romCount: 0,
        igdbId: 50,
      },
      suggested: "3do",
    },
  ],
  collections: [],
};

const download = (phase: RommDownload["phase"], received = 50): RommDownload => ({
  entryId: "entry-chrono",
  gameId: "game-chrono",
  title: "Chrono Trigger",
  serverId: "home",
  phase,
  received,
  total: 100,
  error: null,
});

function backend(initial: Partial<RommOverview> = {}) {
  let overview: RommOverview = {
    settings: {
      servers: [],
      cache: { dir: "", limitGb: 100, keepFavorites: true, verify: true },
    },
    servers: [],
    cache: { dir: "/games/romm-cache", used: 0, limit: 100 * 1024 ** 3, games: [] },
    downloads: [],
    ...initial,
  };
  let listener: ((download: RommDownload) => void) | undefined;
  const copy = () => structuredClone(overview);
  const saveServer = vi.fn(async (server: RommServer, _secret: string | null) => {
    overview.settings.servers = [
      ...overview.settings.servers.filter((old) => old.id !== server.id),
      server,
    ];
    overview.servers = [
      ...overview.servers.filter((old) => old.id !== server.id),
      {
        id: server.id,
        state: "untested",
        version: null,
        error: null,
        lastSyncMs: null,
        syncing: false,
        games: 0,
        hasSecret: true,
      },
    ];
    return copy();
  });
  const api = {
    overview: vi.fn(async () => copy()),
    saveSettings: vi.fn(async (settings: RommSettings) => {
      overview = { ...overview, settings };
      return copy();
    }),
    saveServer,
    removeServer: vi.fn(async () => copy()),
    testServer: vi.fn(async () => report),
    sync: vi.fn(async (id: string | null) => [
      {
        serverId: id ?? "",
        added: 3,
        updated: 0,
        unchanged: 0,
        removed: 0,
        orphaned: 0,
        unmatchedPlatforms: [],
        warnings: [],
      },
    ]),
    clearCache: vi.fn(async () => copy().cache),
    setPinned: vi.fn(async (entryId: string, pinned: boolean) => ({
      entryId,
      serverId: "home",
      status: "cached",
      pinned,
      size: 1,
      message: null,
      download: null,
    })),
    evict: vi.fn(async () => true),
    cancelDownload: vi.fn(async () => true),
    subscribeDownloads: vi.fn(async (next: (download: RommDownload) => void) => {
      listener = next;
      return () => {
        listener = undefined;
      };
    }),
  } satisfies RommApi;
  return { api, emit: (next: RommDownload) => listener?.(next) };
}

function renderPanel(api: RommApi) {
  return render(
    <QueryClientProvider
      client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}
    >
      <EmulationProvider api={createEmulationFixtureApi()}>
        <RommApiContext.Provider value={api}>
          <RommPanel />
        </RommApiContext.Provider>
      </EmulationProvider>
    </QueryClientProvider>,
  );
}

const home: RommServer = {
  id: "home",
  name: "Home",
  url: "https://romm.local",
  auth: "password",
  username: "admin",
  allowInvalidCerts: false,
  importAll: true,
  platforms: [],
  collections: [],
  syncOnStartup: true,
  syncInterval: "6h",
};

const settingsWith = (server: RommServer): RommSettings => ({
  servers: [server],
  cache: { dir: "", limitGb: 100, keepFavorites: true, verify: true },
});

const statusOf = (patch: Partial<RommServerStatus>): RommServerStatus => ({
  id: "home",
  state: "untested",
  version: null,
  error: null,
  lastSyncMs: null,
  syncing: false,
  games: 0,
  hasSecret: true,
  ...patch,
});

/** The server's row in the table. */
const serverRow = async () => {
  await screen.findByRole("button", { name: "Home" });
  const row = document.querySelector<HTMLElement>('[data-part="romm-server"]');
  if (!row) throw new Error("no server row");
  return row;
};

describe("RommPanel", () => {
  it("adds a server, tests it and hands the password to the keychain", async () => {
    const { api } = backend();
    renderPanel(api);
    fireEvent.click(await screen.findByRole("button", { name: "Connect a server" }));
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Home" } });
    fireEvent.change(screen.getByLabelText("Server address"), {
      target: { value: "https://romm.local" },
    });
    fireEvent.change(screen.getByLabelText("Username"), { target: { value: "admin" } });
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "hunter2" } });

    fireEvent.click(screen.getByRole("button", { name: "Test connection" }));
    await screen.findByText("Connected · v4.4.0");
    expect(api.testServer).toHaveBeenCalledWith(
      expect.objectContaining({ url: "https://romm.local", username: "admin" }),
      "hunter2",
    );
    expect(screen.getByText("Super Nintendo Entertainment System")).toBeTruthy();
    // A platform without ROMs has nothing to import.
    expect(screen.queryByText("3DO Interactive Multiplayer")).toBeNull();
    expect(screen.getByText("2 platforms · 316 ROMs on the server")).toBeTruthy();
    expect(screen.getByText("Match a platform to import these ROMs.")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Save server" }));
    await waitFor(() =>
      expect(api.saveServer).toHaveBeenCalledWith(
        expect.objectContaining({
          name: "Home",
          url: "https://romm.local",
          auth: "password",
          username: "admin",
        }),
        "hunter2",
      ),
    );
    // The password itself never appears in settings.
    expect(JSON.stringify(api.saveServer.mock.calls[0]?.[0])).not.toContain("hunter2");
    await screen.findByRole("button", { name: "Home" });
    expect(screen.getByRole("button", { name: "Save changes" })).toBeTruthy();
  });

  it("asks before removing a server, naming what goes with it (#49)", async () => {
    const { api } = backend({
      settings: {
        servers: [home],
        cache: { dir: "", limitGb: 100, keepFavorites: true, verify: true },
      },
    });
    renderPanel(api);
    fireEvent.click(await screen.findByRole("button", { name: "Home" }));
    fireEvent.click(await screen.findByRole("button", { name: "Remove…" }));
    const confirm = await screen.findByRole("alertdialog", { name: "Remove Home?" });
    expect(confirm.textContent).toContain("Its games leave your library");
    fireEvent.click(within(confirm).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
    expect(api.removeServer).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Remove…" }));
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Remove" }),
    );
    await waitFor(() => expect(api.removeServer).toHaveBeenCalledWith("home"));
  });

  it("syncs a server on request", async () => {
    const { api } = backend({
      settings: {
        servers: [home],
        cache: { dir: "", limitGb: 100, keepFavorites: true, verify: true },
      },
    });
    renderPanel(api);
    fireEvent.click(await screen.findByRole("button", { name: "Sync Home" }));
    await waitFor(() => expect(api.sync).toHaveBeenCalledWith("home"));
  });

  it("shows a sync that fails before the server's row records it", async () => {
    const { api } = backend({
      settings: {
        servers: [home],
        cache: { dir: "", limitGb: 100, keepFavorites: true, verify: true },
      },
    });
    api.sync.mockRejectedValueOnce(new Error("The server's settings couldn't be read"));
    renderPanel(api);
    fireEvent.click(await screen.findByRole("button", { name: "Sync Home" }));
    expect((await screen.findByRole("alert")).textContent).toBe(
      "The server's settings couldn't be read",
    );
  });

  it("shows download progress and cancels a download", async () => {
    const { api, emit } = backend({ downloads: [download("downloading")] });
    renderPanel(api);
    await screen.findByText(/Downloading · 50%/);
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(api.cancelDownload).toHaveBeenCalledWith("entry-chrono");
    await waitFor(() => expect(api.subscribeDownloads).toHaveBeenCalled());
    emit(download("downloading", 75));
    await screen.findByText(/Downloading · 75%/);
    emit(download("cancelled", 75));
    await waitFor(() => expect(screen.queryByText("Chrono Trigger")).toBeNull());
  });

  it("shows the cache's size and clears it", async () => {
    const { api } = backend({
      cache: {
        dir: "/games/romm-cache",
        used: 2 * 1024 ** 3,
        limit: 100 * 1024 ** 3,
        games: [
          {
            serverId: "home",
            romId: 11,
            entryId: "entry-mana",
            gameId: "game-mana",
            title: "Secret of Mana",
            size: 2 * 1024 ** 3,
            lastUsedMs: 1,
            pinned: false,
            favorite: false,
            orphaned: false,
          },
        ],
      },
    });
    renderPanel(api);
    expect(await screen.findByText("1 game · 2.0 GiB")).toBeTruthy();
    expect(screen.queryByText("Secret of Mana")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Clear Cache" }));
    await waitFor(() => expect(api.clearCache).toHaveBeenCalled());
  });
});

describe("RommPanel sign-in on this device (#73)", () => {
  it("a server synced without its API token asks for one here", async () => {
    const synced: RommServer = { ...home, auth: "token", username: "" };
    const { api } = backend({
      settings: settingsWith(synced),
      servers: [statusOf({ state: "sign_in_needed", hasSecret: false })],
    });
    const { container } = renderPanel(api);
    const row = await serverRow();
    expect(row.dataset.status).toBe("sign_in_needed");
    expect(within(row).getByText("Sign-in needed on this device")).toBeTruthy();
    expect(within(row).getByText(/Enter this server's API token here/)).toBeTruthy();
    expect(screen.queryByText("Can't connect")).toBeNull();
    // Signing in replaces Sync now as the row's action.
    expect(within(row).queryByRole("button", { name: "Sync Home" })).toBeNull();
    const signIn = within(row).getByRole("button", { name: "Enter the API token for Home" });
    expect(signIn.textContent).toBe("Enter API token");

    fireEvent.click(signIn);
    await waitFor(() => expect(document.activeElement).toBe(screen.getByLabelText("API token")));
    fireEvent.change(screen.getByLabelText("API token"), { target: { value: "rmm_token" } });
    fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() =>
      expect(api.saveServer).toHaveBeenCalledWith(
        expect.objectContaining({ id: "home" }),
        "rmm_token",
      ),
    );
    // Signed in: the row goes back to syncing.
    await within(row).findByRole("button", { name: "Sync Home" });

    const root = container.querySelector('[data-cs="romm-servers"]');
    expect(root && checkAnatomy(root)).toEqual([]);
  });

  it("a refused sign-in shows the server's reason once, with Sign in", async () => {
    const { api } = backend({
      settings: settingsWith(home),
      servers: [
        statusOf({
          state: "sign_in_needed",
          error: "sign-in failed: Invalid username or password",
        }),
      ],
    });
    renderPanel(api);
    const row = await serverRow();
    expect(within(row).getAllByText(/Invalid username or password/)).toHaveLength(1);
    fireEvent.click(within(row).getByRole("button", { name: "Sign in to Home" }));
    await waitFor(() => expect(document.activeElement).toBe(screen.getByLabelText("Password")));
  });

  it("a failed sync shows its error once, not again under the status", async () => {
    const { api } = backend({
      settings: settingsWith(home),
      servers: [statusOf({ state: "error", error: "the server answered 500" })],
    });
    api.sync.mockRejectedValueOnce(
      new CommandFailure({ code: "server", summary: "The server answered 500" }),
    );
    renderPanel(api);
    const row = await serverRow();
    expect(within(row).getByText("the server answered 500")).toBeTruthy();
    fireEvent.click(within(row).getByRole("button", { name: "Sync Home" }));
    expect((await within(row).findByRole("alert")).textContent).toBe("The server answered 500");
    expect(within(row).queryByText("the server answered 500")).toBeNull();
  });

  it("the editor tells a refused sign-in from a connection that failed", async () => {
    const { api } = backend({ settings: settingsWith(home), servers: [statusOf({})] });
    api.testServer.mockRejectedValueOnce(
      new CommandFailure({ code: "auth", summary: "Sign-in failed: Invalid username or password" }),
    );
    renderPanel(api);
    fireEvent.click(await screen.findByRole("button", { name: "Test connection" }));
    await screen.findByText("Can't sign in");
    expect(screen.queryByText("Can't connect")).toBeNull();
    expect(document.querySelector('[data-part="connection"]')?.getAttribute("data-status")).toBe(
      "sign_in_needed",
    );
  });
});

describe("mergeDownload", () => {
  it("keeps the newest state per game and drops finished downloads", () => {
    const failed = { ...download("failed"), error: "the download doesn't match" };
    expect(mergeDownload([download("downloading")], download("verifying"))).toEqual([
      download("verifying"),
    ]);
    expect(mergeDownload([download("downloading")], download("done"))).toEqual([]);
    expect(mergeDownload([], failed)).toEqual([failed]);
  });
});
