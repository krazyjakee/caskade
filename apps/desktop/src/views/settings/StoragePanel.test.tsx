import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { SettingsApi } from "../../api/settings";
import { formatBytes, type RootSummary, type StorageApi } from "../../api/storage";
import { StoragePanel } from "./StoragePanel";
import { SettingsProvider } from "./useSettings";

afterEach(cleanup);

function root(id: string, label: string, isDefault = false): RootSummary {
  return {
    root: { id, path: `/games/${id}`, label, isDefault },
    status: "online",
    capabilities: {
      writable: true,
      executable: true,
      symlinks: true,
      caseSensitive: true,
      freeBytes: 10 * 1024 ** 3,
      totalBytes: 20 * 1024 ** 3,
      filesystem: "ext4",
      warnings: [],
    },
    folderTemplate: "{title}",
    games: [],
    gamesBytes: 0,
    otherBytes: 10 * 1024 ** 3,
  };
}
function setup(initial: RootSummary[]) {
  let roots = structuredClone(initial);
  let preference = { alwaysUseDefault: false };
  let revision = 0;
  const settings: SettingsApi = {
    sections: async () => [],
    get: async (namespace) => ({ namespace, version: 1, revision, value: preference }),
    set: vi.fn(async (namespace, value) => {
      preference = value as typeof preference;
      return { namespace, version: 1, revision: ++revision, value };
    }),
    reset: async (namespace) => ({
      namespace,
      version: 1,
      revision: ++revision,
      value: { alwaysUseDefault: false },
    }),
    subscribe: async () => () => {},
  };
  const api: StorageApi = {
    list: vi.fn(async () => structuredClone(roots)),
    preferences: async () => preference,
    add: vi.fn(async (_path, label) => {
      const added = root("archive", label ?? "Archive");
      roots.push(added);
      return added;
    }),
    edit: vi.fn(async (id, label, template) => {
      const found = roots.find((r) => r.root.id === id);
      if (!found) throw new Error("missing root");
      found.root.label = label;
      found.folderTemplate = template;
      return structuredClone(found);
    }),
    setDefault: vi.fn(async (id) => {
      for (const root of roots) root.root.isDefault = root.root.id === id;
      return structuredClone(roots.find((r) => r.root.id === id) as RootSummary);
    }),
    remove: vi.fn(async (id) => {
      roots = roots.filter((r) => r.root.id !== id);
    }),
    moveAndRemove: vi.fn(async (id) => {
      roots = roots.filter((r) => r.root.id !== id);
    }),
    chooseDirectory: vi.fn(async () => "/games/archive"),
    move: vi.fn(async () => {}),
    uninstall: vi.fn(async () => {}),
  };
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <SettingsProvider api={settings}>
        <StoragePanel api={api} />
      </SettingsProvider>
    </QueryClientProvider>,
  );
  return { api, settings };
}
describe("storage settings", () => {
  it("adds a native folder, edits its label/template, changes default and keeps files on removal", async () => {
    const { api } = setup([root("fast", "Fast SSD", true)]);
    await screen.findByRole("button", { name: /Fast SSD/ });
    fireEvent.click(screen.getByRole("button", { name: "Browse…" }));
    await waitFor(() =>
      expect((screen.getByLabelText("Folder") as HTMLInputElement).value).toBe("/games/archive"),
    );
    fireEvent.change(screen.getByLabelText("Label"), { target: { value: "Archive" } });
    fireEvent.click(screen.getByRole("button", { name: "Add location" }));
    await screen.findByRole("button", { name: /Archive.*online/ });
    expect(api.add).toHaveBeenCalledWith("/games/archive", "Archive");
    fireEvent.change(screen.getByLabelText("Location label"), { target: { value: "Games HDD" } });
    fireEvent.change(screen.getByLabelText("Folder-name template"), {
      target: { value: "Library {title}" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save location" }));
    await waitFor(() =>
      expect(api.edit).toHaveBeenCalledWith("archive", "Games HDD", "Library {title}"),
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Make default" }).hasAttribute("disabled")).toBe(
        false,
      ),
    );
    fireEvent.click(screen.getByRole("button", { name: "Make default" }));
    await waitFor(() => expect(api.setDefault).toHaveBeenCalledWith("archive"));
    fireEvent.click(await screen.findByRole("button", { name: /Fast SSD.*online/ }));
    fireEvent.click(screen.getByRole("button", { name: "Remove location…" }));
    fireEvent.click(screen.getByRole("button", { name: "Keep files and remove from library" }));
    await waitFor(() => expect(api.remove).toHaveBeenCalledWith("fast"));
  });
  function withGames(count: number) {
    const location = root("fast", "Fast SSD");
    location.games = Array.from({ length: count }, (_, index) => ({
      entryId: `entry-${index}`,
      gameId: `game-${index}`,
      title: `Fixture ${index}`,
      relativeDir: `Fixture ${index}`,
      sizeBytes: 1024,
      available: true,
      moveBlocked: null,
    }));
    location.gamesBytes = count * 1024;
    return location;
  }
  async function confirmation(button: string, title: string) {
    await screen.findByRole("button", { name: /Fast SSD/ });
    fireEvent.click(screen.getByRole("button", { name: "Remove location…" }));
    fireEvent.click(screen.getByRole("button", { name: button }));
    return screen.findByRole("alertdialog", { name: title });
  }
  it("says what removing a location does to its games", async () => {
    setup([withGames(2), root("archive", "Archive", true)]);
    await screen.findByRole("button", { name: /Fast SSD/ });
    fireEvent.click(screen.getByRole("button", { name: "Remove location…" }));
    const panel = screen.getByText(/Its 2 games leave your library/);
    expect(panel.textContent).toContain("with their play time, favourites and collections");
    expect(panel.textContent).toContain("A game you also have another way stays");
    screen.getByText(/Files on disk are kept\. Add this folder again and Caskade finds them/);
  });
  it("asks once, then moves a location's games and removes it as one job", async () => {
    const { api, settings } = setup([withGames(2), root("archive", "Archive", true)]);
    const dialog = await confirmation(
      "Move its 2 games there, then remove",
      "Move its 2 games to Archive?",
    );
    expect(dialog.textContent).toContain(`${formatBytes(2048)} to move`);
    expect(dialog.textContent).toContain("Fast SSD is removed once every game has moved");
    expect(api.moveAndRemove).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("button", { name: "Move and remove" }));
    await waitFor(() => expect(api.moveAndRemove).toHaveBeenCalledWith("fast", "archive"));
    expect(api.moveAndRemove).toHaveBeenCalledTimes(1);
    await screen.findByText("Move completed. Library identity and playtime were preserved.");
    expect(api.move).not.toHaveBeenCalled();
    expect(api.remove).not.toHaveBeenCalled();
    fireEvent.click(await screen.findByRole("switch", { name: "Always use the default location" }));
    await waitFor(() =>
      expect(settings.set).toHaveBeenCalledWith("core.storage", { alwaysUseDefault: true }, 0),
    );
  });
  it("moves nothing when the question is cancelled", async () => {
    const { api } = setup([withGames(1), root("archive", "Archive", true)]);
    const dialog = await confirmation(
      "Move its game there, then remove",
      "Move its game to Archive?",
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
    expect(api.moveAndRemove).not.toHaveBeenCalled();
  });
  it("keeps the location and says what moved when the job stops", async () => {
    const { api } = setup([withGames(2), root("archive", "Archive", true)]);
    vi.mocked(api.moveAndRemove).mockRejectedValueOnce(
      new Error("Cancelled. One game moved to Archive. Fast SSD was kept."),
    );
    const dialog = await confirmation(
      "Move its 2 games there, then remove",
      "Move its 2 games to Archive?",
    );
    fireEvent.click(within(dialog).getByRole("button", { name: "Move and remove" }));
    await screen.findByText(/One game moved to Archive\. Fast SSD was kept\./);
    expect(screen.getByRole("button", { name: /Fast SSD/ })).toBeTruthy();
    expect(api.remove).not.toHaveBeenCalled();
    // Refreshed: the games that moved show where they went.
    await waitFor(() => expect(vi.mocked(api.list).mock.calls.length).toBeGreaterThan(1));
  });
  it("shows offline roots and blocks destructive/default actions while unavailable", async () => {
    const offline = root("fast", "Offline SSD", true);
    offline.status = "offline";
    offline.capabilities.freeBytes = null;
    offline.capabilities.totalBytes = null;
    setup([offline]);
    expect(await screen.findByRole("button", { name: /Offline SSD.*offline/ })).toBeTruthy();
    expect(
      (screen.getByRole("button", { name: "Make default" }) as HTMLButtonElement).disabled,
    ).toBe(true);
    expect(
      (screen.getByRole("button", { name: "Remove location…" }) as HTMLButtonElement).disabled,
    ).toBe(true);
    expect(screen.queryByText(/Copying/)).toBeNull();
  });
});
