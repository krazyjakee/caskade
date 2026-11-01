import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { SettingsApi } from "../../api/settings";
import type { RootSummary, StorageApi } from "../../api/storage";
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
  it("moves a location's games elsewhere before removing it and persists the picker preference", async () => {
    const first = root("fast", "Fast SSD");
    first.games = [
      {
        entryId: "entry-1",
        gameId: "game-1",
        title: "Fixture",
        relativeDir: "Fixture",
        sizeBytes: 1024,
        available: true,
        moveBlocked: null,
      },
    ];
    const { api, settings } = setup([first, root("archive", "Archive", true)]);
    await screen.findByRole("button", { name: /Fast SSD/ });
    expect(screen.queryByText("Fixture")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Remove location…" }));
    fireEvent.click(screen.getByRole("button", { name: "Move its game there, then remove" }));
    await waitFor(() => expect(api.move).toHaveBeenCalledWith("entry-1", "archive"));
    await waitFor(() => expect(api.remove).toHaveBeenCalledWith("fast"));
    fireEvent.click(screen.getByRole("checkbox", { name: "Always use default location" }));
    await waitFor(() =>
      expect(settings.set).toHaveBeenCalledWith("core.storage", { alwaysUseDefault: true }, 0),
    );
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
